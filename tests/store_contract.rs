use aperip_nomos::{
    domain::LicenseDraft,
    store::{StoreConfig, StoreError, spawn_store},
};
use tempfile::tempdir;

fn draft(filename: &str, title: &str, body: &str, uploaded_at_ms: i64) -> LicenseDraft {
    LicenseDraft::from_upload(
        filename,
        Some(title),
        None,
        body.as_bytes(),
        uploaded_at_ms,
    )
    .expect("valid draft")
}

#[tokio::test]
async fn persists_create_list_get_and_delete_across_restart() {
    let directory = tempdir().expect("temporary directory");
    let database_path = directory.path().join("licenses.rnmdb");
    let config = StoreConfig {
        database_path: database_path.clone(),
        page_key: [7; 32],
        queue_capacity: 16,
    };
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
    assert!(matches!(store.get("alpha").await, Err(StoreError::NotFound)));
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
