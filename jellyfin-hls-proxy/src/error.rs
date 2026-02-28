//! Error types for the Jellyfin HLS proxy.

use thiserror::Error;

/// Main error type for the proxy.
#[derive(Error, Debug)]
pub enum ProxyError {
    /// HTTP client error.
    #[error("HTTP client error: {0}")]
    HttpClient(#[from] hyper_util::client::legacy::Error),

    /// HTTP error.
    #[error("HTTP error: {0}")]
    Http(#[from] axum::http::Error),

    /// JSON serialization/deserialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Configuration error.
    #[error("Configuration error: {0}")]
    Config(String),

    /// Backend unavailable.
    #[error("Jellyfin backend unavailable: {0}")]
    BackendUnavailable(String),

    /// Media not found.
    #[error("Media not found: {0}")]
    MediaNotFound(String),

    /// HLS generation error.
    #[error("HLS generation error: {0}")]
    Hls(String),

    /// TLS error.
    #[error("TLS error: {0}")]
    Tls(String),
}

/// Result type alias using our error type.
pub type Result<T> = std::result::Result<T, ProxyError>;
