use aperip_nomos::{
    http::{admin_router, public_router},
    store::{StoreConfig, StoreTask, spawn_store},
};
use axum::{
    Router,
    body::Body,
    http::{HeaderName, Request, StatusCode, header},
    response::Response,
};
use http_body_util::BodyExt;
use tempfile::{TempDir, tempdir};
use tower::ServiceExt;

const ADMIN_TOKEN: &str = "test-admin-token-with-at-least-32-bytes";
const CSP: &str = "default-src 'self'; base-uri 'none'; connect-src 'self'; font-src 'self'; form-action 'self'; frame-ancestors 'none'; img-src 'self'; object-src 'none'; script-src 'self'; style-src 'self'";

async fn request(app: &Router, path: &str) -> Response {
    app.clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("router response")
}

async fn body_text(response: Response) -> String {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("response body")
        .to_bytes();
    String::from_utf8(bytes.to_vec()).expect("UTF-8 response")
}

fn assert_header(response: &Response, name: &'static str, expected: &str) {
    let name = HeaderName::from_static(name);
    assert_eq!(
        response
            .headers()
            .get(&name)
            .and_then(|value| value.to_str().ok()),
        Some(expected),
        "unexpected {name} header",
    );
}

fn assert_security_headers(response: &Response) {
    assert_header(response, "content-security-policy", CSP);
    assert_header(response, "x-content-type-options", "nosniff");
    assert_header(response, "referrer-policy", "no-referrer");
    assert_header(response, "x-frame-options", "DENY");
}

fn assert_html(response: &Response) {
    assert_eq!(response.status(), StatusCode::OK);
    assert_header(
        response,
        header::CONTENT_TYPE.as_str(),
        "text/html; charset=utf-8",
    );
    assert_header(response, header::CACHE_CONTROL.as_str(), "no-store");
    assert_security_headers(response);
}

fn assert_static(response: &Response, content_type: &str) {
    assert_eq!(response.status(), StatusCode::OK);
    assert_header(response, header::CONTENT_TYPE.as_str(), content_type);
    assert_header(
        response,
        header::CACHE_CONTROL.as_str(),
        "public, max-age=3600",
    );
    assert_security_headers(response);
}

fn test_routers() -> (TempDir, Router, Router, StoreTask) {
    let directory = tempdir().expect("temporary directory");
    let (store, task) = spawn_store(StoreConfig {
        database_path: directory.path().join("frontend.rnmdb"),
        page_key: [17; 32],
        queue_capacity: 16,
    })
    .expect("start store");
    let public = public_router(store.clone());
    let admin = admin_router(store, ADMIN_TOKEN.to_string());
    (directory, public, admin, task)
}

fn assert_ids(source: &str, ids: &[&str]) {
    for id in ids {
        assert!(
            source.contains(&format!("id=\"{id}\"")),
            "missing stable id {id}",
        );
    }
}

fn assert_safe_javascript(source: &str) {
    for forbidden in [
        "innerHTML",
        "localStorage",
        "sessionStorage",
        "document.cookie",
    ] {
        assert!(
            !source.contains(forbidden),
            "JavaScript must not contain {forbidden}",
        );
    }
}

#[tokio::test]
async fn serves_isolated_embedded_interfaces_with_hardened_headers() {
    let (_directory, public, admin, task) = test_routers();

    let public_home = request(&public, "/").await;
    assert_html(&public_home);

    let public_detail = request(&public, "/licenses/mit").await;
    assert_html(&public_detail);

    for (path, content_type) in [
        ("/assets/app.css", "text/css; charset=utf-8"),
        ("/assets/public.js", "text/javascript; charset=utf-8"),
        ("/assets/detail.js", "text/javascript; charset=utf-8"),
        ("/favicon.svg", "image/svg+xml"),
    ] {
        let response = request(&public, path).await;
        assert_static(&response, content_type);
    }

    let admin_home = request(&admin, "/").await;
    assert_html(&admin_home);
    assert!(body_text(admin_home).await.contains("id=\"token-form\""));

    for (path, content_type) in [
        ("/assets/app.css", "text/css; charset=utf-8"),
        ("/assets/admin.js", "text/javascript; charset=utf-8"),
        ("/favicon.svg", "image/svg+xml"),
    ] {
        let response = request(&admin, path).await;
        assert_static(&response, content_type);
    }

    assert_eq!(
        request(&public, "/assets/admin.js").await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(&admin, "/assets/public.js").await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(&admin, "/assets/detail.js").await.status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(&admin, "/licenses/mit").await.status(),
        StatusCode::NOT_FOUND
    );

    task.shutdown().await;
}

#[tokio::test]
async fn public_assets_use_real_api_and_safe_dom_rendering() {
    let (_directory, public, _admin, task) = test_routers();
    let catalog_html = body_text(request(&public, "/").await).await;
    let detail_html = body_text(request(&public, "/licenses/mit").await).await;
    let public_javascript = body_text(request(&public, "/assets/public.js").await).await;
    let detail_javascript = body_text(request(&public, "/assets/detail.js").await).await;

    assert_ids(
        &catalog_html,
        &[
            "license-filter",
            "license-count",
            "license-list",
            "catalog-status",
        ],
    );
    assert!(catalog_html.contains("for=\"license-filter\""));
    assert!(catalog_html.contains("<noscript"));
    assert!(catalog_html.contains("aria-live="));
    assert!(catalog_html.contains("/assets/public.js"));

    assert_ids(
        &detail_html,
        &[
            "license-title",
            "license-slug",
            "license-source",
            "license-digest",
            "license-uploaded",
            "license-body",
            "copy-license",
            "copy-status",
            "detail-status",
            "detail-content",
        ],
    );
    assert!(detail_html.contains("<noscript"));
    assert!(detail_html.contains("aria-live="));
    assert!(detail_html.contains("/assets/detail.js"));

    assert!(public_javascript.contains("/api/licenses"));
    assert!(public_javascript.contains("/licenses/"));
    assert!(public_javascript.contains("encodeURIComponent"));
    assert!(public_javascript.contains("createElement"));
    assert!(public_javascript.contains("textContent"));
    assert_safe_javascript(&public_javascript);

    assert!(detail_javascript.contains("/api/licenses/"));
    assert!(detail_javascript.contains("decodeURIComponent"));
    assert!(detail_javascript.contains("Intl.DateTimeFormat"));
    assert!(detail_javascript.contains("navigator.clipboard.writeText"));
    assert!(detail_javascript.contains("createElement"));
    assert!(detail_javascript.contains("textContent"));
    assert_safe_javascript(&detail_javascript);

    task.shutdown().await;
}

#[tokio::test]
async fn admin_assets_keep_token_in_memory_and_use_real_api() {
    let (_directory, _public, admin, task) = test_routers();
    let admin_html = body_text(request(&admin, "/").await).await;
    let admin_javascript = body_text(request(&admin, "/assets/admin.js").await).await;

    assert_ids(
        &admin_html,
        &[
            "token-gate",
            "token-form",
            "admin-token",
            "token-error",
            "manager",
            "lock-admin",
            "upload-form",
            "license-file",
            "upload-title",
            "upload-slug",
            "upload-status",
            "admin-list",
            "admin-count",
            "admin-status",
            "delete-dialog",
            "delete-license-name",
            "confirm-delete",
            "cancel-delete",
        ],
    );
    for input in ["admin-token", "license-file", "upload-title", "upload-slug"] {
        assert!(
            admin_html.contains(&format!("for=\"{input}\"")),
            "missing label for {input}",
        );
    }
    assert!(admin_html.contains("<dialog"));
    assert!(admin_html.contains("<noscript"));
    assert!(admin_html.contains("aria-live="));
    assert!(admin_html.contains("/assets/admin.js"));

    assert!(admin_javascript.contains("let adminToken = \"\""));
    assert!(admin_javascript.contains("/api/admin/licenses"));
    for method in ["GET", "POST", "DELETE"] {
        assert!(
            admin_javascript.contains(method),
            "missing administrator {method} operation",
        );
    }
    assert!(admin_javascript.contains("FormData"));
    assert!(admin_javascript.contains("createElement"));
    assert!(admin_javascript.contains("textContent"));
    assert_safe_javascript(&admin_javascript);
    for forbidden in [
        "URLSearchParams",
        "location.search",
        "location.hash",
        "?token=",
    ] {
        assert!(
            !admin_javascript.contains(forbidden),
            "administrator token must not enter URL state through {forbidden}",
        );
    }

    task.shutdown().await;
}
