use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
};

use fs2::FileExt;
use rnmdb_cli::{CommandOutput, LocalSession};
use rnmdb_common::RnovError;
use rnmdb_storage::PageCryptoKey;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("failed to acquire RNMDB owner lock at {path}: {source}")]
    OwnerLock {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("another process owns the RNMDB database (lock: {path})")]
    AlreadyOwned { path: PathBuf },
    #[error("failed to open RNMDB database at {path}: {source}")]
    Open {
        path: PathBuf,
        #[source]
        source: RnovError,
    },
    #[error("RNMDB command failed: {0}")]
    Execute(#[source] RnovError),
    #[error("RNMDB checkpoint failed: {0}")]
    Checkpoint(#[source] RnovError),
}

pub struct DatabaseOwnerLock {
    file: File,
}

impl DatabaseOwnerLock {
    pub fn acquire(database_path: &Path) -> Result<Self, StorageError> {
        let mut lock_name = database_path.as_os_str().to_os_string();
        lock_name.push(".lock");
        let path = PathBuf::from(lock_name);
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|source| StorageError::OwnerLock {
                path: path.clone(),
                source,
            })?;
        match FileExt::try_lock_exclusive(&file) {
            Ok(()) => Ok(Self { file }),
            Err(source) if source.kind() == std::io::ErrorKind::WouldBlock => {
                Err(StorageError::AlreadyOwned { path })
            }
            Err(source) => Err(StorageError::OwnerLock { path, source }),
        }
    }
}

impl Drop for DatabaseOwnerLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

pub struct RnmdbSession {
    inner: LocalSession,
}

impl RnmdbSession {
    pub fn open(path: &Path, key: [u8; 32]) -> Result<Self, StorageError> {
        let inner = LocalSession::single_file_with_key(path, PageCryptoKey::from_bytes(key))
            .map_err(|source| StorageError::Open {
                path: path.to_path_buf(),
                source,
            })?;
        Ok(Self { inner })
    }

    pub fn execute(&mut self, sql: &str) -> Result<CommandOutput, StorageError> {
        self.inner.execute(sql).map_err(StorageError::Execute)
    }

    pub fn checkpoint(&mut self) -> Result<(), StorageError> {
        self.inner.checkpoint().map_err(StorageError::Checkpoint)
    }
}
