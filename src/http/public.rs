use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::Serialize;
use tower_http::trace::TraceLayer;

use crate::store::LicenseStore;

use super::{ApiError, LicenseDetail, LicenseListResponse, LicenseResponse, LicenseSummary};

#[derive(Debug, Serialize)]
pub(super) struct HealthResponse {
    pub(super) status: &'static str,
}

pub(super) fn router(store: LicenseStore) -> Router {
    Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
        .route("/api/licenses", get(list))
        .route("/api/licenses/{slug}", get(detail))
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .layer(TraceLayer::new_for_http())
        .with_state(store)
}

pub(super) async fn live() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn ready(State(store): State<LicenseStore>) -> Result<Json<HealthResponse>, ApiError> {
    if store.is_ready() {
        Ok(Json(HealthResponse { status: "ok" }))
    } else {
        Err(ApiError::service_unavailable())
    }
}

async fn list(State(store): State<LicenseStore>) -> Result<Json<LicenseListResponse>, ApiError> {
    let licenses = store.list().await?;
    Ok(Json(LicenseListResponse {
        licenses: licenses.iter().map(LicenseSummary::from).collect(),
    }))
}

async fn detail(
    State(store): State<LicenseStore>,
    Path(slug): Path<String>,
) -> Result<Json<LicenseResponse>, ApiError> {
    let license = store.get(&slug).await?;
    Ok(Json(LicenseResponse {
        license: LicenseDetail::from(license),
    }))
}

async fn not_found() -> ApiError {
    ApiError::not_found()
}

async fn method_not_allowed() -> ApiError {
    ApiError::method_not_allowed()
}
