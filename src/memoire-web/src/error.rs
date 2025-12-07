//! HTTP error responses

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

/// API error types
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("range not satisfiable")]
    RangeNotSatisfiable,

    #[error("not implemented: {0}")]
    NotImplemented(String),

    #[error("internal server error: {0}")]
    Internal(#[from] anyhow::Error),

    #[error("database error: {0}")]
    Database(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, error_name, message) = match self {
            ApiError::NotFound(msg) => (StatusCode::NOT_FOUND, "NotFound", msg),
            ApiError::BadRequest(msg) => (StatusCode::BAD_REQUEST, "BadRequest", msg),
            ApiError::Forbidden(msg) => (StatusCode::FORBIDDEN, "Forbidden", msg),
            ApiError::RangeNotSatisfiable => (
                StatusCode::RANGE_NOT_SATISFIABLE,
                "RangeNotSatisfiable",
                "requested range not satisfiable".to_string(),
            ),
            ApiError::NotImplemented(msg) => (
                StatusCode::NOT_IMPLEMENTED,
                "NotImplemented",
                msg,
            ),
            ApiError::Internal(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "InternalServerError",
                err.to_string(),
            ),
            ApiError::Database(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "DatabaseError",
                msg,
            ),
            ApiError::Io(err) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "IoError",
                err.to_string(),
            ),
        };

        let body = Json(json!({
            "error": error_name,
            "message": message,
        }));

        (status, body).into_response()
    }
}

/// Convert database errors to API errors
impl From<rusqlite::Error> for ApiError {
    fn from(err: rusqlite::Error) -> Self {
        ApiError::Database(err.to_string())
    }
}
