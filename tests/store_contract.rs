use aperip_nomos::{
    domain::{LicenseDraft, LicenseMetadata},
    store::{StoreConfig, StoreError, spawn_store},
};
use std::path::{Path, PathBuf};
use tempfile::tempdir;

fn draft(filename: &str, title: &str, body: &str, uploaded_at_ms: i64) -> LicenseDraft {
    LicenseDraft::from_upload(filename, Some(title), None, body.as_bytes(), uploaded_at_ms)
        .expect("valid draft")
}

fn config(database_path: PathBuf) -> StoreConfig {
    StoreConfig {
        database_path,
        page_key: [7; 32],
        queue_capacity: 16,
    }
}

fn assert_metadata(_metadata: &LicenseMetadata) {}

#[cfg(unix)]
fn symlink_file(original: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(original, link)
}

#[cfg(windows)]
fn symlink_file(original: &Path, link: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(original, link)
}

#[test]
fn zero_queue_capacity_returns_start_error_without_panicking() {
    let directory = tempdir().expect("temporary directory");
    let mut config = config(directory.path().join("licenses.rnmdb"));
    config.queue_capacity = 0;

    let result = std::panic::catch_unwind(|| spawn_store(config));

    let error = match result {
        Ok(Err(error)) => error,
        Ok(Ok((_store, task))) => {
            drop(task);
            panic!("zero queue capacity started a store");
        }
        Err(_) => panic!("zero queue capacity panicked"),
    };
    assert_eq!(
        error.to_string(),
        "store queue capacity must be greater than zero"
    );
}

#[test]
fn store_config_debug_redacts_the_page_key() {
    let config = StoreConfig {
        database_path: PathBuf::from("licenses.rnmdb"),
        page_key: [165; 32],
        queue_capacity: 16,
    };

    let debug = format!("{config:?}");

    assert!(debug.contains("page_key: <redacted>"));
    assert!(!debug.contains("[165, 165"));
}

#[tokio::test]
async fn persists_create_list_get_and_delete_across_restart() {
    let directory = tempdir().expect("temporary directory");
    let database_path = directory.path().join("licenses.rnmdb");
    let config = config(database_path.clone());
    let (store, task) = spawn_store(config.clone()).expect("start store");

    let zulu = store
        .create(draft("zulu.txt", "Zulu License", "zulu body", 20))
        .await
        .expect("create zulu");
    let alpha = store
        .create(draft("alpha.txt", "Alpha License", "alpha body", 10))
        .await
        .expect("create alpha");
    assert_eq!(zulu.id(), 1);
    assert_eq!(alpha.id(), 2);

    let listed = store.list().await.expect("list licenses");
    assert_metadata(&listed[0]);
    assert_eq!(listed[0].id(), alpha.id());
    assert_eq!(
        listed
            .iter()
            .map(|license| license.title())
            .collect::<Vec<_>>(),
        ["Alpha License", "Zulu License"]
    );
    assert_eq!(
        store.get("alpha").await.expect("get alpha").body(),
        "alpha body"
    );
    assert!(matches!(
        store
            .create(draft("alpha.txt", "Replacement", "new body", 30))
            .await,
        Err(StoreError::DuplicateSlug)
    ));

    let deleted = store.delete("alpha").await.expect("delete alpha");
    assert_eq!(deleted.slug(), "alpha");
    assert!(matches!(
        store.get("alpha").await,
        Err(StoreError::NotFound)
    ));
    task.shutdown().await;

    let (restarted, restarted_task) = spawn_store(config).expect("restart store");
    assert!(restarted.is_ready());
    assert!(matches!(
        restarted.get("alpha").await,
        Err(StoreError::NotFound)
    ));
    let persisted = restarted.get("zulu").await.expect("persisted zulu");
    assert_eq!(persisted.sha256(), zulu.sha256());
    assert_eq!(persisted.source_filename(), "zulu.txt");
    restarted_task.shutdown().await;
}

#[tokio::test]
async fn canonical_database_alias_cannot_acquire_a_second_owner_lock() {
    let directory = tempdir().expect("temporary directory");
    let database_path = directory.path().join("licenses.rnmdb");
    let alias_path = directory.path().join("licenses-alias.rnmdb");
    let (store, task) = spawn_store(config(database_path.clone())).expect("start store");
    symlink_file(&database_path, &alias_path).expect("create database path alias");

    let error = match spawn_store(config(alias_path)) {
        Err(error) => error,
        Ok((_second_store, second_task)) => {
            second_task.shutdown().await;
            panic!("database alias acquired a concurrent owner lock");
        }
    };

    assert!(
        error.to_string().contains("another process owns"),
        "unexpected second-owner error: {error}"
    );
    drop(store);
    task.shutdown().await;
}

#[tokio::test]
async fn owner_lock_is_released_when_store_shuts_down() {
    let directory = tempdir().expect("temporary directory");
    let config = config(directory.path().join("licenses.rnmdb"));
    let (store, task) = spawn_store(config.clone()).expect("start store");
    let observer = store.clone();

    task.shutdown().await;

    assert!(!observer.is_ready());
    assert!(matches!(
        observer
            .create(draft("late.txt", "Late", "late body", 1))
            .await,
        Err(StoreError::Unavailable)
    ));
    let (_restarted, restarted_task) = spawn_store(config).expect("lock released after shutdown");
    restarted_task.shutdown().await;
}

#[tokio::test]
async fn cloned_store_can_wait_for_actor_exit() {
    let directory = tempdir().expect("temporary directory");
    let config = config(directory.path().join("licenses.rnmdb"));
    let (store, task) = spawn_store(config).expect("start store");
    let observer = store.clone();

    task.shutdown().await;
    observer.wait_until_unavailable().await;

    assert!(!observer.is_ready());
}

#[tokio::test]
async fn ping_round_trips_the_actor_queue() {
    let directory = tempdir().expect("temporary directory");
    let config = config(directory.path().join("licenses.rnmdb"));
    let (store, task) = spawn_store(config).expect("start store");
    let observer = store.clone();

    assert_eq!(store.ping().await, Ok(()));
    task.shutdown().await;

    assert_eq!(observer.ping().await, Err(StoreError::Unavailable));
}
