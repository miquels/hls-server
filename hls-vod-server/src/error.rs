//! Server-specific error types

use hls_vod_lib::HlsError;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, ServerError>;

#[derive(Debug, Error)]
pub enum ServerError {
    #[error("Library error: {0}")]
    Library(#[from] HlsError),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Internal server error: {0}")]
    Internal(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl axum::response::IntoResponse for ServerError {
    fn into_response(self) -> axum::response::Response {
        let (status, body) = match self {
            ServerError::Library(HlsError::StreamNotFound(id)) => (
                axum::http::StatusCode::NOT_FOUND,
                format!("Stream not found: {}", id),
            ),
            ServerError::Library(HlsError::SegmentNotFound { .. }) => {
                (axum::http::StatusCode::NOT_FOUND, self.to_string())
            }
            _ => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                self.to_string(),
            ),
        };

        (status, body).into_response()
    }
}
