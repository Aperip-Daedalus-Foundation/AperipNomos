use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex, OnceLock},
};

use ammonia::{Builder, UrlRelative};
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd, html::push_html};
use thiserror::Error;
use tokio::sync::{Mutex as AsyncMutex, OnceCell, Semaphore};

#[cfg(test)]
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

const MAX_CACHE_ENTRIES: usize = 32;
const MAX_CACHE_BYTES: usize = 16 * 1024 * 1024;
const MAX_RENDERED_BYTES: usize = 8 * 1024 * 1024;

#[derive(Default)]
struct RenderCache {
    entries: HashMap<String, String>,
    order: VecDeque<String>,
    bytes: usize,
}

static RENDER_CACHE: OnceLock<Mutex<RenderCache>> = OnceLock::new();
static RENDER_FLIGHTS: OnceLock<AsyncMutex<HashMap<String, Arc<RenderCell>>>> = OnceLock::new();
static RENDER_LIMIT: Semaphore = Semaphore::const_new(2);

#[cfg(test)]
static RENDER_EXECUTIONS: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
static TEST_LOCK: AsyncMutex<()> = AsyncMutex::const_new(());
#[cfg(test)]
static ACTIVE_RENDERS: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
static MAX_ACTIVE_RENDERS: AtomicUsize = AtomicUsize::new(0);
#[cfg(test)]
static RENDER_DELAY_MS: AtomicUsize = AtomicUsize::new(0);

type RenderCell = OnceCell<Result<String, MarkdownRenderError>>;

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub(crate) enum MarkdownRenderError {
    #[error("the rendered Markdown exceeds the output limit")]
    OutputTooLarge,
    #[error("the Markdown rendering worker failed")]
    WorkerFailed,
}

pub fn render_license_markdown(markdown: &str) -> String {
    let parser = Parser::new_ext(markdown, markdown_options()).filter_map(safe_event);
    let mut rendered = String::with_capacity(markdown.len());
    push_html(&mut rendered, parser);
    sanitizer().clean(&rendered).to_string()
}

pub(crate) async fn render_cached(
    digest: &str,
    markdown: &str,
) -> Result<String, MarkdownRenderError> {
    if let Some(rendered) = cached(digest) {
        return Ok(rendered);
    }

    let key = digest.to_string();
    let cell = {
        let mut active = flights().lock().await;
        Arc::clone(
            active
                .entry(key.clone())
                .or_insert_with(|| Arc::new(OnceCell::new())),
        )
    };
    let render_key = key.clone();
    let source = markdown.to_string();
    let result = cell
        .get_or_init(|| async move { render_once(&render_key, source).await })
        .await
        .clone();
    let mut active = flights().lock().await;
    if active
        .get(&key)
        .is_some_and(|current| Arc::ptr_eq(current, &cell))
    {
        active.remove(&key);
    }
    result
}

fn cache() -> &'static Mutex<RenderCache> {
    RENDER_CACHE.get_or_init(|| Mutex::new(RenderCache::default()))
}

fn flights() -> &'static AsyncMutex<HashMap<String, Arc<RenderCell>>> {
    RENDER_FLIGHTS.get_or_init(|| AsyncMutex::new(HashMap::new()))
}

fn cached(digest: &str) -> Option<String> {
    cache()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(digest)
}

async fn render_once(digest: &str, markdown: String) -> Result<String, MarkdownRenderError> {
    let permit = RENDER_LIMIT
        .acquire()
        .await
        .map_err(|_| MarkdownRenderError::WorkerFailed)?;
    if let Some(rendered) = cached(digest) {
        return Ok(rendered);
    }
    let key = digest.to_string();
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        #[cfg(test)]
        begin_test_render();
        let rendered = render_license_markdown(&markdown);
        if rendered.len() > MAX_RENDERED_BYTES {
            #[cfg(test)]
            end_test_render();
            return Err(MarkdownRenderError::OutputTooLarge);
        }
        cache()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(&key, rendered.clone());
        #[cfg(test)]
        end_test_render();
        Ok(rendered)
    })
    .await
    .map_err(|_| MarkdownRenderError::WorkerFailed)?
}

#[cfg(test)]
fn begin_test_render() {
    RENDER_EXECUTIONS.fetch_add(1, Ordering::SeqCst);
    let active = ACTIVE_RENDERS.fetch_add(1, Ordering::SeqCst) + 1;
    MAX_ACTIVE_RENDERS.fetch_max(active, Ordering::SeqCst);
    let delay = RENDER_DELAY_MS.load(Ordering::SeqCst);
    if delay > 0 {
        std::thread::sleep(Duration::from_millis(delay as u64));
    }
}

#[cfg(test)]
fn end_test_render() {
    ACTIVE_RENDERS.fetch_sub(1, Ordering::SeqCst);
}

impl RenderCache {
    fn get(&self, digest: &str) -> Option<String> {
        self.entries.get(digest).cloned()
    }

    fn insert(&mut self, digest: &str, rendered: String) {
        if rendered.len() > MAX_CACHE_BYTES || self.entries.contains_key(digest) {
            return;
        }
        while self.entries.len() >= MAX_CACHE_ENTRIES
            || self.bytes.saturating_add(rendered.len()) > MAX_CACHE_BYTES
        {
            if !self.remove_oldest() {
                return;
            }
        }
        self.bytes += rendered.len();
        self.order.push_back(digest.to_string());
        self.entries.insert(digest.to_string(), rendered);
    }

    fn remove_oldest(&mut self) -> bool {
        let Some(digest) = self.order.pop_front() else {
            return false;
        };
        if let Some(rendered) = self.entries.remove(&digest) {
            self.bytes = self.bytes.saturating_sub(rendered.len());
        }
        true
    }
}

fn markdown_options() -> Options {
    Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES
}

fn safe_event(event: Event<'_>) -> Option<Event<'_>> {
    match event {
        Event::Html(source) | Event::InlineHtml(source) => Some(Event::Text(source)),
        Event::Start(Tag::Heading {
            level,
            id,
            classes,
            attrs,
        }) => Some(Event::Start(Tag::Heading {
            level: nested_heading(level),
            id,
            classes,
            attrs,
        })),
        Event::End(TagEnd::Heading(level)) => {
            Some(Event::End(TagEnd::Heading(nested_heading(level))))
        }
        Event::Start(Tag::Image { .. }) | Event::End(TagEnd::Image) => None,
        other => Some(other),
    }
}

fn nested_heading(level: HeadingLevel) -> HeadingLevel {
    match level {
        HeadingLevel::H1 => HeadingLevel::H2,
        HeadingLevel::H2 => HeadingLevel::H3,
        HeadingLevel::H3 => HeadingLevel::H4,
        HeadingLevel::H4 => HeadingLevel::H5,
        HeadingLevel::H5 | HeadingLevel::H6 => HeadingLevel::H6,
    }
}

fn sanitizer() -> Builder<'static> {
    let tags = [
        "a",
        "blockquote",
        "br",
        "code",
        "del",
        "em",
        "h2",
        "h3",
        "h4",
        "h5",
        "h6",
        "hr",
        "li",
        "ol",
        "p",
        "pre",
        "strong",
        "table",
        "tbody",
        "td",
        "th",
        "thead",
        "tr",
        "ul",
    ]
    .into_iter()
    .collect::<HashSet<_>>();
    let attributes = HashMap::from([
        ("a", ["href", "title"].into_iter().collect()),
        ("code", ["class"].into_iter().collect()),
        ("ol", ["start"].into_iter().collect()),
    ]);
    let schemes = ["http", "https", "mailto"]
        .into_iter()
        .collect::<HashSet<_>>();
    let mut builder = Builder::default();
    builder
        .tags(tags)
        .generic_attributes(HashSet::new())
        .tag_attributes(attributes)
        .url_schemes(schemes)
        .url_relative(UrlRelative::Deny)
        .link_rel(Some("noopener noreferrer"));
    builder
}

#[cfg(test)]
mod tests {
    use super::{
        ACTIVE_RENDERS, MAX_ACTIVE_RENDERS, MAX_RENDERED_BYTES, MarkdownRenderError, Ordering,
        RENDER_DELAY_MS, RENDER_EXECUTIONS, TEST_LOCK, render_cached,
    };

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_cache_misses_share_one_render() {
        let _guard = TEST_LOCK.lock().await;
        let before = RENDER_EXECUTIONS.load(Ordering::SeqCst);
        let tasks = (0..16).map(|_| {
            tokio::spawn(async {
                render_cached("single-flight-test-digest", "# Shared\n\nBody")
                    .await
                    .expect("render")
            })
        });
        let rendered = futures_for_test(tasks).await;

        assert!(rendered.iter().all(|value| value == &rendered[0]));
        assert_eq!(RENDER_EXECUTIONS.load(Ordering::SeqCst) - before, 1);
    }

    #[tokio::test]
    async fn rendered_output_is_bounded() {
        let _guard = TEST_LOCK.lock().await;
        let source = "&".repeat((MAX_RENDERED_BYTES / 5) + 1024);
        let result = render_cached("oversized-render-test-digest", &source).await;

        assert_eq!(result, Err(MarkdownRenderError::OutputTooLarge));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn cancelled_request_keeps_its_render_permit_and_populates_cache() {
        let _guard = TEST_LOCK.lock().await;
        RENDER_DELAY_MS.store(150, Ordering::SeqCst);
        MAX_ACTIVE_RENDERS.store(0, Ordering::SeqCst);
        let initial = tokio::spawn(async {
            render_cached("cancelled-render-test-digest", "# Cancelled\n\nBody").await
        });
        while ACTIVE_RENDERS.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
        initial.abort();

        let tasks = (0..6).map(|index| {
            tokio::spawn(async move {
                render_cached(&format!("post-cancel-digest-{index}"), "# Other\n\nBody").await
            })
        });
        for task in tasks {
            task.await.expect("task").expect("render");
        }
        let executions = RENDER_EXECUTIONS.load(Ordering::SeqCst);
        render_cached("cancelled-render-test-digest", "# Cancelled\n\nBody")
            .await
            .expect("cached render");

        RENDER_DELAY_MS.store(0, Ordering::SeqCst);
        assert!(MAX_ACTIVE_RENDERS.load(Ordering::SeqCst) <= 2);
        assert_eq!(RENDER_EXECUTIONS.load(Ordering::SeqCst), executions);
    }

    async fn futures_for_test(
        tasks: impl IntoIterator<Item = tokio::task::JoinHandle<String>>,
    ) -> Vec<String> {
        let mut values = Vec::new();
        for task in tasks {
            values.push(task.await.expect("task"));
        }
        values
    }
}
