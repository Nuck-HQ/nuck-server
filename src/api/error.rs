//! The uniform API error response (ADR 0013).

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use nuck_protocol::api::ApiError;

/// A Nuck API error, rendered as the shared [`ApiError`] body.
pub struct ApiErrorResponse {
    status: StatusCode,
    body: ApiError,
}

impl ApiErrorResponse {
    /// Build an error with an explicit status and `errcode`.
    #[must_use]
    pub fn new(status: StatusCode, errcode: &str, error: impl Into<String>) -> Self {
        Self { status, body: ApiError::new(errcode, error) }
    }

    /// 403 `M_FORBIDDEN`.
    #[must_use]
    pub fn forbidden(error: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "M_FORBIDDEN", error)
    }

    /// 400 `M_INVALID_PARAM`.
    #[must_use]
    pub fn bad_request(error: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "M_INVALID_PARAM", error)
    }

    /// 404 `M_NOT_FOUND`.
    #[must_use]
    pub fn not_found(error: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "M_NOT_FOUND", error)
    }

    /// 501 `M_UNRECOGNIZED` — a well-formed request this server cannot yet serve.
    #[must_use]
    pub fn not_implemented(error: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_IMPLEMENTED, "M_UNRECOGNIZED", error)
    }

    /// 401 `M_UNAUTHORIZED`.
    #[must_use]
    pub fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "M_UNAUTHORIZED", "missing or invalid token")
    }

    /// 503 `M_UNKNOWN` — a required component is not configured.
    #[must_use]
    pub fn unavailable(error: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, "M_UNKNOWN", error)
    }

    /// 500 `M_UNKNOWN`.
    #[must_use]
    pub fn internal(error: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "M_UNKNOWN", error)
    }
}

impl IntoResponse for ApiErrorResponse {
    fn into_response(self) -> Response {
        (self.status, Json(self.body)).into_response()
    }
}
