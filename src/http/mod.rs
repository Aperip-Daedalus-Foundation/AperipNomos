mod admin;
mod error;
mod public;

use axum::Router;
use serde::Serialize;

use crate::{domain::LicenseDocument, store::LicenseStore};

pub use error::{ApiError, ErrorBody, ErrorResponse};

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
    pub source_filename: String,
    pub sha256: String,
    pub uploaded_at_ms: i64,
}

impl From<&LicenseDocument> for LicenseSummary {
    fn from(document: &LicenseDocument) -> Self {
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

impl From<LicenseDocument> for LicenseDetail {
    fn from(document: LicenseDocument) -> Self {
        Self {
            id: document.id(),
            slug: document.slug().to_string(),
            title: document.title().to_string(),
            body: document.body().to_string(),
            source_filename: document.source_filename().to_string(),
            sha256: document.sha256().to_string(),
            uploaded_at_ms: document.uploaded_at_ms(),
        }
    }
}

pub fn public_router(store: LicenseStore) -> Router {
    public::router(store)
}

pub fn admin_router(store: LicenseStore, admin_token: String) -> Router {
    admin::router(store, admin_token)
}
