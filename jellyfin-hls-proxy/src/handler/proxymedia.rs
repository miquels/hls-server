//! Handler for proxymedia HLS endpoint.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::handler::playback::AppState;

/// Query parameters for proxymedia requests.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyMediaParams {
    /// Audio stream index.
    pub audio: Option<i32>,
    /// Video stream index.
    pub video: Option<i32>,
    /// Subtitle stream index.
    pub subtitle: Option<i32>,
    /// Whether to transcode audio.
    pub transcode_audio: Option<bool>,
}

/// Handle proxymedia HLS requests.
pub async fn handle_proxymedia(
    State(_state): State<Arc<AppState>>,
    Path(_path): Path<String>,
    _query: Query<ProxyMediaParams>,
) -> impl IntoResponse {
    // TODO: Implement HLS generation using hls-vod-lib
    // For now, return a placeholder response

    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "error": "HLS generation not yet implemented",
            "message": "This endpoint will serve HLS playlists once implemented"
        })),
    )
        .into_response()
}
