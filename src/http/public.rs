use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::Serialize;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::store::LicenseStore;

use super::{
    ApiError, LicenseDetail, LicenseListResponse, LicenseResponse, LicenseSummary, store_health,
    store_operation,
};

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
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(
                    DefaultMakeSpan::new()
                        .level(Level::INFO)
                        .include_headers(false),
                )
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(store)
}

pub(super) async fn live() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn ready(State(store): State<LicenseStore>) -> Result<Json<HealthResponse>, ApiError> {
    store_health(&store).await?;
    Ok(Json(HealthResponse { status: "ok" }))
}

async fn list(State(store): State<LicenseStore>) -> Result<Json<LicenseListResponse>, ApiError> {
    let licenses = store_operation(store.list()).await?;
    Ok(Json(LicenseListResponse {
        licenses: licenses.iter().map(LicenseSummary::from).collect(),
    }))
}

async fn detail(
    State(store): State<LicenseStore>,
    Path(slug): Path<String>,
) -> Result<Json<LicenseResponse>, ApiError> {
    let license = store_operation(store.get(&slug)).await?;
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
