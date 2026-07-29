use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use rnmdb_cli::CommandOutput;
use rnmdb_types::SqlValue;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

use crate::{
    domain::{LicenseDocument, LicenseDraft},
    storage::{DatabaseOwnerLock, RnmdbSession, StorageError},
};

const SCHEMA_STATEMENTS: [&str; 3] = [
    "CREATE TABLE IF NOT EXISTS licenses (id INT64 NOT NULL, slug TEXT NOT NULL, title TEXT NOT NULL, body_base64 TEXT NOT NULL, source_filename TEXT NOT NULL, sha256 TEXT NOT NULL, uploaded_at_ms INT64 NOT NULL);",
    "CREATE UNIQUE INDEX IF NOT EXISTS licenses_id_uq ON licenses (id);",
    "CREATE UNIQUE INDEX IF NOT EXISTS licenses_slug_uq ON licenses (slug);",
];
const LOAD_LICENSES_SQL: &str = "SELECT id, slug, title, body_base64, source_filename, sha256, uploaded_at_ms FROM licenses ORDER BY id;";

#[derive(Clone, Debug)]
pub struct StoreConfig {
    pub database_path: PathBuf,
    pub page_key: [u8; 32],
    pub queue_capacity: usize,
}

#[derive(Debug, Error)]
pub enum StoreStartError {
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

    pub async fn list(&self) -> Result<Vec<LicenseDocument>, StoreError> {
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

    pub fn is_ready(&self) -> bool {
        self.shared.ready.load(Ordering::Acquire) && self.shared.alive.load(Ordering::Acquire)
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
        response: oneshot::Sender<Result<Vec<LicenseDocument>, StoreError>>,
    },
    Get {
        slug: String,
        response: oneshot::Sender<Result<LicenseDocument, StoreError>>,
    },
    Delete {
        slug: String,
        response: oneshot::Sender<Result<LicenseDocument, StoreError>>,
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

pub fn spawn_store(config: StoreConfig) -> Result<(LicenseStore, StoreTask), StoreStartError> {
    let parent = config
        .database_path
        .parent()
        .ok_or_else(|| StoreStartError::MissingParent(config.database_path.clone()))?;
    fs::create_dir_all(parent).map_err(|source| StoreStartError::CreateDirectory {
        path: parent.to_path_buf(),
        source,
    })?;
    let owner_lock = DatabaseOwnerLock::acquire(&config.database_path)?;
    let state = open_database(&config)?;
    let shared = Arc::new(SharedState {
        ready: AtomicBool::new(true),
        alive: AtomicBool::new(true),
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
            let mut core = StoreCore {
                config,
                shared: Arc::clone(&thread_shared),
                state: Some(state),
                _owner_lock: owner_lock,
            };
            while let Some(command) = receiver.blocking_recv() {
                if core.execute(command) {
                    break;
                }
            }
            thread_shared.ready.store(false, Ordering::Release);
            thread_shared.alive.store(false, Ordering::Release);
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
        let document = draft.into_document(id);
        if self.persist_create(&document).is_err() {
            self.recover();
            return Err(StoreError::StorageUnavailable);
        }
        let state = self.state.as_mut().ok_or(StoreError::StorageUnavailable)?;
        state.next_id = id.checked_add(1).ok_or(StoreError::StorageUnavailable)?;
        state
            .licenses
            .insert(document.slug().to_string(), document.clone());
        Ok(document)
    }

    fn list(&self) -> Result<Vec<LicenseDocument>, StoreError> {
        let state = self.state.as_ref().ok_or(StoreError::StorageUnavailable)?;
        let mut licenses = state.licenses.values().cloned().collect::<Vec<_>>();
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
            self.recover();
            return Err(StoreError::StorageUnavailable);
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
        let statement = format!("DELETE FROM licenses WHERE slug = {};", sql_text_literal(slug));
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

    fn recover(&mut self) {
        self.shared.ready.store(false, Ordering::Release);
        self.state.take();
        match open_database(&self.config) {
            Ok(state) => {
                self.state = Some(state);
                self.shared.ready.store(true, Ordering::Release);
            }
            Err(error) => tracing::error!(error = %error, "RNMDB recovery failed"),
        }
    }
}

async fn receive<T>(
    receiver: oneshot::Receiver<Result<T, StoreError>>,
) -> Result<T, StoreError> {
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
