mod admin;
mod assets;
mod error;
mod public;

use std::{future::Future, time::Duration};

use axum::Router;
use serde::Serialize;

use crate::{
    domain::{LicenseDocument, LicenseMetadata},
    markdown::render_cached,
    store::LicenseStore,
};

pub use error::{ApiError, ErrorBody, ErrorResponse};

const STORE_OPERATION_TIMEOUT: Duration = Duration::from_secs(5);
const STORE_HEALTH_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Serialize)]
pub struct LicenseListResponse {
    pub licenses: Vec<LicenseSummary>,
}

#[derive(Debug, Serialize)]
pub struct LicenseResponse {
    pub license: LicenseDetail,
}

#[derive(Debug, Serialize)]
pub struct LicenseSummary {
    pub id: i64,
    pub slug: String,
    pub title: String,
    pub source_filename: String,
    pub sha256: String,
    pub uploaded_at_ms: i64,
}

#[derive(Debug, Serialize)]
pub struct LicenseDetail {
    pub id: i64,
    pub slug: String,
    pub title: String,
    pub body: String,
    pub body_format: &'static str,
    pub rendered_html: Option<String>,
    pub source_filename: String,
    pub sha256: String,
    pub uploaded_at_ms: i64,
}

impl From<&LicenseMetadata> for LicenseSummary {
    fn from(document: &LicenseMetadata) -> Self {
        Self {
            id: document.id(),
            slug: document.slug().to_string(),
            title: document.title().to_string(),
            source_filename: document.source_filename().to_string(),
            sha256: document.sha256().to_string(),
            uploaded_at_ms: document.uploaded_at_ms(),
        }
    }
}

impl LicenseDetail {
    async fn from_document(document: LicenseDocument) -> Result<Self, ApiError> {
        let rendered_html = render_document_markdown(
            document.source_filename(),
            document.sha256(),
            document.body(),
        )
        .await?;
        Ok(Self {
            id: document.id(),
            slug: document.slug().to_string(),
            title: document.title().to_string(),
            body: document.body().to_string(),
            body_format: if rendered_html.is_some() {
                "markdown"
            } else {
                "plain_text"
            },
            rendered_html,
            source_filename: document.source_filename().to_string(),
            sha256: document.sha256().to_string(),
            uploaded_at_ms: document.uploaded_at_ms(),
        })
    }
}

async fn render_document_markdown(
    filename: &str,
    digest: &str,
    body: &str,
) -> Result<Option<String>, ApiError> {
    if !is_markdown_filename(filename) {
        return Ok(None);
    }
    render_cached(digest, body)
        .await
        .map(Some)
        .map_err(|_| ApiError::service_unavailable())
}

fn is_markdown_filename(filename: &str) -> bool {
    filename.rsplit_once('.').is_some_and(|(_, extension)| {
        extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
    })
}

pub fn public_router(store: LicenseStore) -> Router {
    public::router(store)
}

pub fn admin_router(store: LicenseStore, admin_token: String) -> Router {
    admin::router(store, admin_token)
}

pub(crate) async fn store_operation<T, F>(operation: F) -> Result<T, ApiError>
where
    F: Future<Output = Result<T, crate::store::StoreError>>,
{
    store_operation_with_timeout(STORE_OPERATION_TIMEOUT, operation).await
}

pub(crate) async fn store_health(store: &LicenseStore) -> Result<(), ApiError> {
    store_operation_with_timeout(STORE_HEALTH_TIMEOUT, store.ping()).await
}

async fn store_operation_with_timeout<T, F>(duration: Duration, operation: F) -> Result<T, ApiError>
where
    F: Future<Output = Result<T, crate::store::StoreError>>,
{
    match tokio::time::timeout(duration, operation).await {
        Ok(result) => result.map_err(ApiError::from),
        Err(_) => Err(ApiError::service_unavailable()),
    }
}

#[cfg(test)]
mod tests {
    use std::{future, time::Duration};

    use axum::{http::StatusCode, response::IntoResponse};

    use crate::store::StoreError;

    use super::{is_markdown_filename, store_operation_with_timeout};

    #[test]
    fn markdown_format_is_derived_from_the_source_extension() {
        assert!(is_markdown_filename("AHCL-1.0.md"));
        assert!(is_markdown_filename("LICENSE.MARKDOWN"));
        assert!(!is_markdown_filename("LICENSE.txt"));
        assert!(!is_markdown_filename("LICENSE"));
        assert!(!is_markdown_filename("archive.md.txt"));
    }

    #[tokio::test]
    async fn stalled_store_operation_times_out() {
        let result = store_operation_with_timeout::<(), _>(
            Duration::from_millis(1),
            future::pending::<Result<(), StoreError>>(),
        )
        .await;

        assert_eq!(
            result
                .expect_err("stalled operation must fail")
                .into_response()
                .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }
}
