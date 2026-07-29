use aperip_nomos::{
    http::{admin_router, public_router},
    store::{StoreConfig, spawn_store},
};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use serde_json::Value;
use tempfile::tempdir;
use tower::ServiceExt;

const ADMIN_TOKEN: &str = "test-admin-token-with-at-least-32-bytes";

async fn json(response: axum::response::Response) -> Value {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("response body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("JSON response")
}

async fn request(app: &Router, request: Request<Body>) -> axum::response::Response {
    app.clone().oneshot(request).await.expect("router response")
}

fn multipart_upload(token: Option<&str>) -> Request<Body> {
    let boundary = "aperip-nomos-test-boundary";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nMIT License\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"MIT.md\"\r\nContent-Type: text/markdown\r\n\r\nPermission is hereby granted.\r\n--{boundary}--\r\n"
    );
    let mut builder = Request::builder()
        .method("POST")
        .uri("/api/admin/licenses")
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        );
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder.body(Body::from(body)).expect("upload request")
}

#[tokio::test]
async fn isolates_admin_mutations_and_publishes_uploaded_license() {
    let directory = tempdir().expect("temporary directory");
    let (store, task) = spawn_store(StoreConfig {
        database_path: directory.path().join("http.rnmdb"),
        page_key: [11; 32],
        queue_capacity: 32,
    })
    .expect("start store");
    let public = public_router(store.clone());
    let admin = admin_router(store, ADMIN_TOKEN.to_string());

    let initial = request(
        &public,
        Request::builder()
            .uri("/api/licenses")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(initial.status(), StatusCode::OK);
    assert_eq!(json(initial).await["licenses"], serde_json::json!([]));

    let public_mutation = request(&public, multipart_upload(Some(ADMIN_TOKEN))).await;
    assert_eq!(public_mutation.status(), StatusCode::NOT_FOUND);

    let missing_token = request(&admin, multipart_upload(None)).await;
    assert_eq!(missing_token.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(json(missing_token).await["error"]["code"], "unauthorized");

    let invalid_token = request(&admin, multipart_upload(Some("wrong-token"))).await;
    assert_eq!(invalid_token.status(), StatusCode::UNAUTHORIZED);

    let created = request(&admin, multipart_upload(Some(ADMIN_TOKEN))).await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let created_json = json(created).await;
    assert_eq!(created_json["license"]["slug"], "mit");
    assert_eq!(created_json["license"]["title"], "MIT License");
    assert_eq!(created_json["license"]["body_format"], "markdown");
    assert_eq!(
        created_json["license"]["rendered_html"],
        "<p>Permission is hereby granted.</p>\n"
    );

    let duplicate = request(&admin, multipart_upload(Some(ADMIN_TOKEN))).await;
    assert_eq!(duplicate.status(), StatusCode::CONFLICT);
    assert_eq!(json(duplicate).await["error"]["code"], "duplicate_slug");

    let detail = request(
        &public,
        Request::builder()
            .uri("/api/licenses/mit")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(detail.status(), StatusCode::OK);
    let detail_json = json(detail).await;
    assert_eq!(
        detail_json["license"]["body"],
        "Permission is hereby granted."
    );
    assert_eq!(detail_json["license"]["body_format"], "markdown");
    assert_eq!(
        detail_json["license"]["rendered_html"],
        "<p>Permission is hereby granted.</p>\n"
    );

    let deleted = request(
        &admin,
        Request::builder()
            .method("DELETE")
            .uri("/api/admin/licenses/mit")
            .header(header::AUTHORIZATION, format!("Bearer {ADMIN_TOKEN}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);

    let missing = request(
        &public,
        Request::builder()
            .uri("/api/licenses/mit")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert_eq!(json(missing).await["error"]["code"], "not_found");

    task.shutdown().await;
}
