use axum::{
    Json,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;

use crate::{domain::LicenseValidationError, store::StoreError};

#[derive(Clone, Copy, Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: &'static str,
    authenticate: bool,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: ErrorBody,
}

#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub code: &'static str,
    pub message: &'static str,
}

impl ApiError {
    pub(crate) const fn bad_request(code: &'static str, message: &'static str) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }

    pub(crate) const fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            code: "unauthorized",
            message: "a valid bearer token is required",
            authenticate: true,
        }
    }

    pub(crate) const fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", "resource not found")
    }

    pub(crate) const fn method_not_allowed() -> Self {
        Self::new(
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
            "method not allowed",
        )
    }

    pub(crate) const fn service_unavailable() -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "service_unavailable",
            "service temporarily unavailable",
        )
    }

    const fn new(status: StatusCode, code: &'static str, message: &'static str) -> Self {
        Self {
            status,
            code,
            message,
            authenticate: false,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = (
            self.status,
            Json(ErrorResponse {
                error: ErrorBody {
                    code: self.code,
                    message: self.message,
                },
            }),
        )
            .into_response();
        if self.authenticate {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        response
    }
}

impl From<LicenseValidationError> for ApiError {
    fn from(error: LicenseValidationError) -> Self {
        let (code, message) = match error {
            LicenseValidationError::InvalidFilename => {
                ("invalid_filename", "the uploaded filename is invalid")
            }
            LicenseValidationError::InvalidTitle => {
                ("invalid_title", "the license title is invalid")
            }
            LicenseValidationError::InvalidSlug => ("invalid_slug", "the license slug is invalid"),
            LicenseValidationError::EmptyBody => ("empty_file", "the license file is empty"),
            LicenseValidationError::InvalidUtf8 => {
                ("invalid_utf8", "the license file is not valid UTF-8")
            }
            LicenseValidationError::BodyTooLarge => {
                ("file_too_large", "the license file exceeds 1 MiB")
            }
        };
        Self::bad_request(code, message)
    }
}

impl From<StoreError> for ApiError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::DuplicateSlug => Self::new(
                StatusCode::CONFLICT,
                "duplicate_slug",
                "a license with this slug already exists",
            ),
            StoreError::NotFound => Self::not_found(),
            StoreError::QueueFull | StoreError::Unavailable | StoreError::StorageUnavailable => {
                Self::service_unavailable()
            }
        }
    }
}
