use crate::state::AppState;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use hls_vod_lib::HlsError;
use std::sync::Arc;

/// Custom error response for HLS operations
#[derive(Debug)]
pub enum HttpError {
    StreamNotFound(String),
    SegmentNotFound(String),
    InvalidFormat(String),
    InternalError(String),
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            HttpError::StreamNotFound(m) => (StatusCode::NOT_FOUND, m),
            HttpError::SegmentNotFound(m) => (StatusCode::NOT_FOUND, m),
            HttpError::InvalidFormat(m) => (StatusCode::BAD_REQUEST, m),
            HttpError::InternalError(m) => (StatusCode::INTERNAL_SERVER_ERROR, m),
        };

        (status, message).into_response()
    }
}

impl From<HlsError> for HttpError {
    fn from(err: HlsError) -> Self {
        match err {
            HlsError::StreamNotFound(m) => HttpError::StreamNotFound(m),
            HlsError::SegmentNotFound { .. } => HttpError::SegmentNotFound(err.to_string()),
            HlsError::Muxing(m) => HttpError::InternalError(m),
            HlsError::Transcode(m) => HttpError::InternalError(m),
            HlsError::Ffmpeg(e) => HttpError::InternalError(e.to_string()),
            HlsError::Io(e) => HttpError::InternalError(e.to_string()),
            _ => HttpError::InternalError(err.to_string()),
        }
    }
}

/// Health check endpoint
pub async fn health_check() -> (StatusCode, &'static str) {
    (StatusCode::OK, "OK")
}

/// Version information endpoint
pub async fn version_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "online",
        "version": env!("CARGO_PKG_VERSION"),
        "ffmpeg": hls_vod_lib::ffmpeg_version_info()
    }))
}

/// Debug endpoint: cache statistics
pub async fn cache_stats(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let stats = state.cache_stats();
    Json(serde_json::json!({
        "size": stats.entry_count,
        "memory_usage": stats.total_size_bytes,
        "capacity": stats.memory_limit_bytes,
    }))
}

/// Debug endpoint: active streams
pub async fn active_streams(
    State(_state): State<Arc<AppState>>,
) -> Json<Vec<hls_vod_lib::ActiveStreamInfo>> {
    let streams = hls_vod_lib::active_streams();
    Json(streams)
}
