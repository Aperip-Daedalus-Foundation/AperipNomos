use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, Request, State, multipart::Field},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get},
};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tower_http::trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::{
    domain::{LicenseDraft, LicenseValidationError, MAX_LICENSE_BYTES},
    store::LicenseStore,
};

use super::{
    ApiError, LicenseDetail, LicenseListResponse, LicenseResponse, LicenseSummary, assets, public,
    render_document_markdown, store_health, store_operation,
};

const MULTIPART_FRAMING_ALLOWANCE: usize = 64 * 1024;
const MULTIPART_BODY_LIMIT: usize = MAX_LICENSE_BYTES + MULTIPART_FRAMING_ALLOWANCE;

#[derive(Clone)]
struct AdminState {
    store: LicenseStore,
    token_digest: [u8; 32],
}

#[derive(Default)]
struct Upload {
    filename: Option<String>,
    file: Option<Vec<u8>>,
    title: Option<String>,
    slug: Option<String>,
}

pub(super) fn router(store: LicenseStore, admin_token: String) -> Router {
    let state = AdminState {
        store,
        token_digest: Sha256::digest(admin_token.as_bytes()).into(),
    };
    let protected = Router::new()
        .route("/api/admin/licenses", get(list).post(create))
        .route("/api/admin/licenses/{slug}", delete(remove))
        .route_layer(middleware::from_fn_with_state(state.clone(), authorize));

    Router::new()
        .route("/", get(assets::admin_home))
        .route("/assets/app.css", get(assets::stylesheet))
        .route("/assets/admin.js", get(assets::admin_script))
        .route("/favicon.svg", get(assets::favicon))
        .route("/health/live", get(public::live))
        .route("/health/ready", get(ready))
        .merge(protected)
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .layer(DefaultBodyLimit::max(MULTIPART_BODY_LIMIT))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(
                    DefaultMakeSpan::new()
                        .level(Level::INFO)
                        .include_headers(false),
                )
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(state)
}

async fn authorize(
    State(state): State<AdminState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let authorized = bearer_token(request.headers())
        .map(|token| Sha256::digest(token.as_bytes()))
        .is_some_and(|digest| bool::from(digest.as_slice().ct_eq(&state.token_digest)));
    if authorized {
        next.run(request).await
    } else {
        ApiError::unauthorized().into_response()
    }
}

fn bearer_token(headers: &axum::http::HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    (scheme.eq_ignore_ascii_case("Bearer") && !token.is_empty() && !token.contains(' '))
        .then_some(token)
}

async fn ready(State(state): State<AdminState>) -> Result<Json<public::HealthResponse>, ApiError> {
    store_health(&state.store).await?;
    Ok(Json(public::HealthResponse { status: "ok" }))
}

async fn list(State(state): State<AdminState>) -> Result<Json<LicenseListResponse>, ApiError> {
    let licenses = store_operation(state.store.list()).await?;
    Ok(Json(LicenseListResponse {
        licenses: licenses.iter().map(LicenseSummary::from).collect(),
    }))
}

async fn create(
    State(state): State<AdminState>,
    multipart: Result<Multipart, axum::extract::multipart::MultipartRejection>,
) -> Result<(StatusCode, Json<LicenseResponse>), ApiError> {
    let upload = parse_upload(multipart.map_err(|_| invalid_multipart())?).await?;
    let filename = upload.filename.ok_or_else(missing_file)?;
    let bytes = upload.file.ok_or_else(missing_file)?;
    let uploaded_at_ms = current_time_ms()?;
    let draft = LicenseDraft::from_upload(
        &filename,
        upload.title.as_deref(),
        upload.slug.as_deref(),
        &bytes,
        uploaded_at_ms,
    )?;
    render_document_markdown(draft.source_filename(), draft.sha256(), draft.body()).await?;
    let license = store_operation(state.store.create(draft)).await?;
    Ok((
        StatusCode::CREATED,
        Json(LicenseResponse {
            license: LicenseDetail::from_document(license).await?,
        }),
    ))
}

async fn parse_upload(mut multipart: Multipart) -> Result<Upload, ApiError> {
    let mut upload = Upload::default();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| invalid_multipart())?
    {
        match field.name() {
            Some("file") if upload.file.is_none() => {
                let filename = field
                    .file_name()
                    .map(str::to_string)
                    .ok_or_else(missing_file)?;
                upload.file = Some(read_file(field).await?);
                upload.filename = Some(filename);
            }
            Some("title") if upload.title.is_none() => {
                upload.title = Some(field.text().await.map_err(|_| invalid_multipart())?);
            }
            Some("slug") if upload.slug.is_none() => {
                upload.slug = Some(field.text().await.map_err(|_| invalid_multipart())?);
            }
            _ => return Err(invalid_multipart()),
        }
    }
    Ok(upload)
}

async fn read_file(mut field: Field<'_>) -> Result<Vec<u8>, ApiError> {
    let mut bytes = Vec::new();
    while let Some(chunk) = field.chunk().await.map_err(|_| invalid_multipart())? {
        let length = bytes
            .len()
            .checked_add(chunk.len())
            .ok_or(LicenseValidationError::BodyTooLarge)?;
        if length > MAX_LICENSE_BYTES {
            return Err(LicenseValidationError::BodyTooLarge.into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

async fn remove(
    State(state): State<AdminState>,
    Path(slug): Path<String>,
) -> Result<StatusCode, ApiError> {
    store_operation(state.store.delete(&slug)).await?;
    Ok(StatusCode::NO_CONTENT)
}

fn current_time_ms() -> Result<i64, ApiError> {
    let milliseconds = time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000;
    i64::try_from(milliseconds).map_err(|_| ApiError::service_unavailable())
}

fn invalid_multipart() -> ApiError {
    ApiError::bad_request("invalid_multipart", "the multipart upload is invalid")
}

fn missing_file() -> ApiError {
    ApiError::bad_request("missing_file", "the multipart upload requires a file")
}

async fn not_found() -> ApiError {
    ApiError::not_found()
}

async fn method_not_allowed() -> ApiError {
    ApiError::method_not_allowed()
}
