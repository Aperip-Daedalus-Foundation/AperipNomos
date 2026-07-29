use std::{
    collections::BTreeMap,
    fmt, fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use rnmdb_cli::CommandOutput;
use rnmdb_types::SqlValue;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot, watch};

use crate::{
    domain::{LicenseDocument, LicenseDraft, LicenseMetadata},
    storage::{DatabaseOwnerLock, RnmdbSession, StorageError, canonical_database_path},
};

const SCHEMA_STATEMENTS: [&str; 3] = [
    "CREATE TABLE IF NOT EXISTS licenses (id INT64 NOT NULL, slug TEXT NOT NULL, title TEXT NOT NULL, body_base64 TEXT NOT NULL, source_filename TEXT NOT NULL, sha256 TEXT NOT NULL, uploaded_at_ms INT64 NOT NULL);",
    "CREATE UNIQUE INDEX IF NOT EXISTS licenses_id_uq ON licenses (id);",
    "CREATE UNIQUE INDEX IF NOT EXISTS licenses_slug_uq ON licenses (slug);",
];
const LOAD_LICENSES_SQL: &str = "SELECT id, slug, title, body_base64, source_filename, sha256, uploaded_at_ms FROM licenses ORDER BY id;";

#[derive(Clone)]
pub struct StoreConfig {
    pub database_path: PathBuf,
    pub page_key: [u8; 32],
    pub queue_capacity: usize,
}

struct Redacted;

impl fmt::Debug for Redacted {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<redacted>")
    }
}

impl fmt::Debug for StoreConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StoreConfig")
            .field("database_path", &self.database_path)
            .field("page_key", &Redacted)
            .field("queue_capacity", &self.queue_capacity)
            .finish()
    }
}

#[derive(Debug, Error)]
pub enum StoreStartError {
    #[error("store queue capacity must be greater than zero")]
    InvalidQueueCapacity,
    #[error("database path has no parent: {0}")]
    MissingParent(PathBuf),
    #[error("failed to create database directory {path}: {source}")]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error("persisted license data is invalid: {0}")]
    Corrupt(String),
    #[error("failed to start RNMDB actor thread: {0}")]
    Thread(#[source] std::io::Error),
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum StoreError {
    #[error("RNMDB queue is full")]
    QueueFull,
    #[error("RNMDB actor is unavailable")]
    Unavailable,
    #[error("a license with this slug already exists")]
    DuplicateSlug,
    #[error("license was not found")]
    NotFound,
    #[error("persistent storage is unavailable")]
    StorageUnavailable,
}

struct SharedState {
    ready: AtomicBool,
    alive: AtomicBool,
    liveness: watch::Sender<bool>,
}

struct ActorHealthGuard {
    shared: Arc<SharedState>,
}

impl ActorHealthGuard {
    fn new(shared: Arc<SharedState>) -> Self {
        Self { shared }
    }
}

impl Drop for ActorHealthGuard {
    fn drop(&mut self) {
        self.shared.ready.store(false, Ordering::Release);
        self.shared.alive.store(false, Ordering::Release);
        self.shared.liveness.send_replace(false);
    }
}

#[derive(Clone)]
pub struct LicenseStore {
    sender: mpsc::Sender<StoreCommand>,
    shared: Arc<SharedState>,
}

impl LicenseStore {
    pub async fn create(&self, draft: LicenseDraft) -> Result<LicenseDocument, StoreError> {
        let (response, receiver) = oneshot::channel();
        self.send(StoreCommand::Create { draft, response })?;
        receive(receiver).await
    }

    pub async fn list(&self) -> Result<Vec<LicenseMetadata>, StoreError> {
        let (response, receiver) = oneshot::channel();
        self.send(StoreCommand::List { response })?;
        receive(receiver).await
    }

    pub async fn get(&self, slug: &str) -> Result<LicenseDocument, StoreError> {
        let (response, receiver) = oneshot::channel();
        self.send(StoreCommand::Get {
            slug: slug.to_string(),
            response,
        })?;
        receive(receiver).await
    }

    pub async fn delete(&self, slug: &str) -> Result<LicenseDocument, StoreError> {
        let (response, receiver) = oneshot::channel();
        self.send(StoreCommand::Delete {
            slug: slug.to_string(),
            response,
        })?;
        receive(receiver).await
    }

    pub async fn ping(&self) -> Result<(), StoreError> {
        let (response, receiver) = oneshot::channel();
        self.send(StoreCommand::Ping { response })?;
        receive(receiver).await
    }

    pub fn is_ready(&self) -> bool {
        self.shared.ready.load(Ordering::Acquire) && self.shared.alive.load(Ordering::Acquire)
    }

    pub async fn wait_until_unavailable(&self) {
        let mut liveness = self.shared.liveness.subscribe();
        loop {
            if !*liveness.borrow_and_update() {
                return;
            }
            if liveness.changed().await.is_err() {
                return;
            }
        }
    }

    fn send(&self, command: StoreCommand) -> Result<(), StoreError> {
        if !self.shared.alive.load(Ordering::Acquire) {
            return Err(StoreError::Unavailable);
        }
        self.sender.try_send(command).map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => StoreError::QueueFull,
            mpsc::error::TrySendError::Closed(_) => StoreError::Unavailable,
        })
    }
}

pub struct StoreTask {
    sender: mpsc::Sender<StoreCommand>,
    shared: Arc<SharedState>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl StoreTask {
    pub async fn shutdown(mut self) {
        self.shared.ready.store(false, Ordering::Release);
        let _ = self.sender.send(StoreCommand::Shutdown).await;
        if let Some(join) = self.join.take() {
            let _ = tokio::task::spawn_blocking(move || join.join()).await;
        }
    }
}

enum StoreCommand {
    Create {
        draft: LicenseDraft,
        response: oneshot::Sender<Result<LicenseDocument, StoreError>>,
    },
    List {
        response: oneshot::Sender<Result<Vec<LicenseMetadata>, StoreError>>,
    },
    Get {
        slug: String,
        response: oneshot::Sender<Result<LicenseDocument, StoreError>>,
    },
    Delete {
        slug: String,
        response: oneshot::Sender<Result<LicenseDocument, StoreError>>,
    },
    Ping {
        response: oneshot::Sender<Result<(), StoreError>>,
    },
    Shutdown,
}

struct DatabaseState {
    session: RnmdbSession,
    licenses: BTreeMap<String, LicenseDocument>,
    next_id: i64,
}

struct StoreCore {
    config: StoreConfig,
    shared: Arc<SharedState>,
    state: Option<DatabaseState>,
    _owner_lock: DatabaseOwnerLock,
}

pub fn spawn_store(mut config: StoreConfig) -> Result<(LicenseStore, StoreTask), StoreStartError> {
    if config.queue_capacity == 0 {
        return Err(StoreStartError::InvalidQueueCapacity);
    }
    if config.database_path.file_name().is_none() {
        return Err(StoreStartError::MissingParent(config.database_path));
    }
    let parent = config
        .database_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|source| StoreStartError::CreateDirectory {
        path: parent.to_path_buf(),
        source,
    })?;
    config.database_path = canonical_database_path(&config.database_path)?;
    let owner_lock = DatabaseOwnerLock::acquire(&config.database_path)?;
    let state = open_database(&config)?;
    let (liveness, _receiver) = watch::channel(true);
    let shared = Arc::new(SharedState {
        ready: AtomicBool::new(true),
        alive: AtomicBool::new(true),
        liveness,
    });
    let (sender, mut receiver) = mpsc::channel(config.queue_capacity);
    let handle = LicenseStore {
        sender: sender.clone(),
        shared: Arc::clone(&shared),
    };
    let thread_shared = Arc::clone(&shared);
    let join = std::thread::Builder::new()
        .name("aperip-nomos-rnmdb".to_string())
        .spawn(move || {
            let _health_guard = ActorHealthGuard::new(Arc::clone(&thread_shared));
            let mut core = StoreCore {
                config,
                shared: Arc::clone(&thread_shared),
                state: Some(state),
                _owner_lock: owner_lock,
            };
            while let Some(command) = receiver.blocking_recv() {
                if core.execute(command) || !thread_shared.alive.load(Ordering::Acquire) {
                    break;
                }
            }
        })
        .map_err(StoreStartError::Thread)?;
    Ok((
        handle,
        StoreTask {
            sender,
            shared,
            join: Some(join),
        },
    ))
}

impl StoreCore {
    fn execute(&mut self, command: StoreCommand) -> bool {
        match command {
            StoreCommand::Create { draft, response } => {
                let _ = response.send(self.create(draft));
            }
            StoreCommand::List { response } => {
                let _ = response.send(self.list());
            }
            StoreCommand::Get { slug, response } => {
                let _ = response.send(self.get(&slug));
            }
            StoreCommand::Delete { slug, response } => {
                let _ = response.send(self.delete(&slug));
            }
            StoreCommand::Ping { response } => {
                let _ = response.send(Ok(()));
            }
            StoreCommand::Shutdown => return true,
        }
        false
    }

    fn create(&mut self, draft: LicenseDraft) -> Result<LicenseDocument, StoreError> {
        let state = self.state.as_ref().ok_or(StoreError::StorageUnavailable)?;
        if state.licenses.contains_key(draft.slug()) {
            return Err(StoreError::DuplicateSlug);
        }
        let id = state.next_id;
        let next_id = id.checked_add(1).ok_or(StoreError::StorageUnavailable)?;
        let document = draft.into_document(id);
        if self.persist_create(&document).is_err() {
            self.recover_after_create_error(&document)?;
            return Ok(document);
        }
        let state = self.state.as_mut().ok_or(StoreError::StorageUnavailable)?;
        state.next_id = next_id;
        state
            .licenses
            .insert(document.slug().to_string(), document.clone());
        Ok(document)
    }

    fn list(&self) -> Result<Vec<LicenseMetadata>, StoreError> {
        let state = self.state.as_ref().ok_or(StoreError::StorageUnavailable)?;
        let mut licenses = state
            .licenses
            .values()
            .map(LicenseMetadata::from)
            .collect::<Vec<_>>();
        licenses.sort_by(|left, right| {
            left.title()
                .cmp(right.title())
                .then_with(|| left.slug().cmp(right.slug()))
        });
        Ok(licenses)
    }

    fn get(&self, slug: &str) -> Result<LicenseDocument, StoreError> {
        self.state
            .as_ref()
            .and_then(|state| state.licenses.get(slug))
            .cloned()
            .ok_or(StoreError::NotFound)
    }

    fn delete(&mut self, slug: &str) -> Result<LicenseDocument, StoreError> {
        let document = self.get(slug)?;
        if self.persist_delete(slug).is_err() {
            self.recover_after_delete_error(slug)?;
            return Ok(document);
        }
        let removed = self
            .state
            .as_mut()
            .and_then(|state| state.licenses.remove(slug))
            .ok_or(StoreError::StorageUnavailable)?;
        debug_assert_eq!(removed, document);
        Ok(removed)
    }

    fn persist_create(&mut self, document: &LicenseDocument) -> Result<(), StorageError> {
        let statement = insert_statement(document);
        self.transaction(&statement)
    }

    fn persist_delete(&mut self, slug: &str) -> Result<(), StorageError> {
        let statement = format!(
            "DELETE FROM licenses WHERE slug = {};",
            sql_text_literal(slug)
        );
        self.transaction(&statement)
    }

    fn transaction(&mut self, statement: &str) -> Result<(), StorageError> {
        let state = self.state.as_mut().expect("store state must be open");
        state.session.execute("BEGIN;")?;
        if let Err(error) = state.session.execute(statement) {
            let _ = state.session.execute("ROLLBACK;");
            return Err(error);
        }
        if let Err(error) = state.session.execute("COMMIT;") {
            let _ = state.session.execute("ROLLBACK;");
            return Err(error);
        }
        Ok(())
    }

    fn recover_after_create_error(&mut self, document: &LicenseDocument) -> Result<(), StoreError> {
        self.recover()?;
        let durable = self
            .state
            .as_ref()
            .and_then(|state| state.licenses.get(document.slug()))
            .is_some_and(|persisted| persisted == document);
        durable.then_some(()).ok_or(StoreError::StorageUnavailable)
    }

    fn recover_after_delete_error(&mut self, slug: &str) -> Result<(), StoreError> {
        self.recover()?;
        let durable = self
            .state
            .as_ref()
            .is_some_and(|state| !state.licenses.contains_key(slug));
        durable.then_some(()).ok_or(StoreError::StorageUnavailable)
    }

    fn recover(&mut self) -> Result<(), StoreError> {
        self.shared.ready.store(false, Ordering::Release);
        self.state.take();
        match open_database(&self.config) {
            Ok(state) => {
                self.state = Some(state);
                self.shared.ready.store(true, Ordering::Release);
                Ok(())
            }
            Err(error) => {
                tracing::error!(error = %error, "RNMDB recovery failed");
                self.shared.alive.store(false, Ordering::Release);
                Err(StoreError::StorageUnavailable)
            }
        }
    }
}

async fn receive<T>(receiver: oneshot::Receiver<Result<T, StoreError>>) -> Result<T, StoreError> {
    receiver.await.unwrap_or(Err(StoreError::Unavailable))
}

fn open_database(config: &StoreConfig) -> Result<DatabaseState, StoreStartError> {
    let mut session = RnmdbSession::open(&config.database_path, config.page_key)?;
    for statement in SCHEMA_STATEMENTS {
        session.execute(statement)?;
    }
    session.checkpoint()?;
    let output = session.execute(LOAD_LICENSES_SQL)?;
    let CommandOutput::Rows(batch) = output else {
        return Err(StoreStartError::Corrupt(
            "license scan did not return rows".to_string(),
        ));
    };
    let mut licenses = BTreeMap::new();
    let mut next_id = 1_i64;
    for row in batch.rows() {
        let document = decode_document(row.values())?;
        if licenses
            .insert(document.slug().to_string(), document.clone())
            .is_some()
        {
            return Err(StoreStartError::Corrupt(
                "duplicate persisted slug".to_string(),
            ));
        }
        next_id = next_id.max(
            document
                .id()
                .checked_add(1)
                .ok_or_else(|| StoreStartError::Corrupt("license id overflow".to_string()))?,
        );
    }
    Ok(DatabaseState {
        session,
        licenses,
        next_id,
    })
}

fn decode_document(values: &[SqlValue]) -> Result<LicenseDocument, StoreStartError> {
    let [
        SqlValue::Int64(id),
        SqlValue::Text(slug),
        SqlValue::Text(title),
        SqlValue::Text(body_base64),
        SqlValue::Text(source_filename),
        SqlValue::Text(sha256),
        SqlValue::Int64(uploaded_at_ms),
    ] = values
    else {
        return Err(StoreStartError::Corrupt(
            "license row has an unexpected shape".to_string(),
        ));
    };
    let body = STANDARD
        .decode(body_base64)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .ok_or_else(|| StoreStartError::Corrupt("license body is invalid".to_string()))?;
    LicenseDocument::rehydrate(
        *id,
        slug.clone(),
        title.clone(),
        body,
        source_filename.clone(),
        sha256.clone(),
        *uploaded_at_ms,
    )
    .ok_or_else(|| StoreStartError::Corrupt("license fields are invalid".to_string()))
}

fn insert_statement(document: &LicenseDocument) -> String {
    format!(
        "INSERT INTO licenses (id, slug, title, body_base64, source_filename, sha256, uploaded_at_ms) VALUES ({}, {}, {}, {}, {}, {}, {});",
        document.id(),
        sql_text_literal(document.slug()),
        sql_text_literal(document.title()),
        sql_text_literal(&STANDARD.encode(document.body().as_bytes())),
        sql_text_literal(document.source_filename()),
        sql_text_literal(document.sha256()),
        document.uploaded_at_ms(),
    )
}

fn sql_text_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn core(config: StoreConfig) -> StoreCore {
        fs::create_dir_all(config.database_path.parent().expect("database path parent"))
            .expect("create database directory");
        let owner_lock = DatabaseOwnerLock::acquire(&config.database_path).expect("owner lock");
        let state = open_database(&config).expect("open database");
        StoreCore {
            config,
            shared: Arc::new(SharedState {
                ready: AtomicBool::new(true),
                alive: AtomicBool::new(true),
                liveness: watch::channel(true).0,
            }),
            state: Some(state),
            _owner_lock: owner_lock,
        }
    }

    fn document(id: i64, slug: &str) -> LicenseDocument {
        LicenseDraft::from_upload(
            &format!("{slug}.txt"),
            Some("Test License"),
            Some(slug),
            b"test body",
            42,
        )
        .expect("valid draft")
        .into_document(id)
    }

    #[test]
    fn recovery_treats_durable_create_as_success() {
        let directory = tempdir().expect("temporary directory");
        let mut core = core(StoreConfig {
            database_path: directory.path().join("licenses.rnmdb"),
            page_key: [7; 32],
            queue_capacity: 4,
        });
        let document = document(1, "created");
        core.persist_create(&document).expect("commit create");

        core.recover_after_create_error(&document)
            .expect("durable create reconciled");

        assert_eq!(core.get("created"), Ok(document));
    }

    #[test]
    fn recovery_treats_durable_delete_as_success() {
        let directory = tempdir().expect("temporary directory");
        let mut core = core(StoreConfig {
            database_path: directory.path().join("licenses.rnmdb"),
            page_key: [7; 32],
            queue_capacity: 4,
        });
        let document = document(1, "deleted");
        core.persist_create(&document).expect("commit fixture");
        core.recover_after_create_error(&document)
            .expect("load fixture");
        core.persist_delete("deleted").expect("commit delete");

        core.recover_after_delete_error("deleted")
            .expect("durable delete reconciled");

        assert_eq!(core.get("deleted"), Err(StoreError::NotFound));
    }

    #[test]
    fn failed_recovery_marks_the_actor_unavailable() {
        let directory = tempdir().expect("temporary directory");
        let mut core = core(StoreConfig {
            database_path: directory.path().join("licenses.rnmdb"),
            page_key: [7; 32],
            queue_capacity: 4,
        });
        core.config.page_key = [9; 32];

        assert_eq!(core.recover(), Err(StoreError::StorageUnavailable));
        assert!(!core.shared.ready.load(Ordering::Acquire));
        assert!(!core.shared.alive.load(Ordering::Acquire));
    }

    #[test]
    fn actor_health_guard_clears_flags_during_unwind() {
        let shared = Arc::new(SharedState {
            ready: AtomicBool::new(true),
            alive: AtomicBool::new(true),
            liveness: watch::channel(true).0,
        });
        let panic_shared = Arc::clone(&shared);
        let mut liveness = shared.liveness.subscribe();

        let result = std::panic::catch_unwind(move || {
            let _guard = ActorHealthGuard::new(panic_shared);
            panic!("test actor panic");
        });

        assert!(result.is_err());
        assert!(!shared.ready.load(Ordering::Acquire));
        assert!(!shared.alive.load(Ordering::Acquire));
        assert!(!*liveness.borrow_and_update());
    }
}
