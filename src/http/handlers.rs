//! HTTP request handlers
//!
//! Implements handlers for all HLS endpoints.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use std::sync::Arc;

use crate::error::HlsError;
use crate::playlist::{
    generate_master_playlist, generate_subtitle_playlist, generate_video_playlist,
};
use crate::segment::generate_init_segment;
use crate::state::AppState;

/// HTTP error type
#[derive(Debug)]
pub enum HttpError {
    StreamNotFound(String),
    SegmentNotFound(String),
    InternalError(String),
}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            HttpError::StreamNotFound(id) => {
                (StatusCode::NOT_FOUND, format!("Stream not found: {}", id))
            }
            HttpError::SegmentNotFound(msg) => (StatusCode::NOT_FOUND, msg),
            HttpError::InternalError(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };

        (status, body).into_response()
    }
}

impl From<HlsError> for HttpError {
    fn from(err: HlsError) -> Self {
        match err {
            HlsError::StreamNotFound(id) => HttpError::StreamNotFound(id),
            HlsError::SegmentNotFound {
                stream_id,
                segment_type,
                sequence,
            } => HttpError::SegmentNotFound(format!(
                "Segment not found: {}/{}/{}",
                stream_id, segment_type, sequence
            )),
            _ => HttpError::InternalError(err.to_string()),
        }
    }
}

/// Extension trait for AppState
pub trait AppStateExt {
    fn get_stream_or_error(
        &self,
        stream_id: &str,
    ) -> Result<Arc<crate::state::StreamIndex>, HttpError>;
}

impl AppStateExt for AppState {
    fn get_stream_or_error(
        &self,
        stream_id: &str,
    ) -> Result<Arc<crate::state::StreamIndex>, HttpError> {
        self.get_stream(stream_id)
            .ok_or_else(|| HttpError::StreamNotFound(stream_id.to_string()))
    }
}

/// Health check endpoint
pub async fn health_check() -> &'static str {
    "OK"
}

/// Version endpoint
pub async fn version_check() -> &'static str {
    concat!("hls-server v", env!("CARGO_PKG_VERSION"))
}

/// Master playlist endpoint
/// GET /streams/{id}/master.m3u8
pub async fn master_playlist(
    State(state): State<Arc<AppState>>,
    Path(stream_id): Path<String>,
) -> Result<Response, HttpError> {
    let index = state.get_stream_or_error(&stream_id)?;

    let playlist = generate_master_playlist(&index);

    let mut headers = HeaderMap::new();
    headers.insert(
        "Content-Type",
        HeaderValue::from_static("application/vnd.apple.mpegurl"),
    );
    headers.insert("Cache-Control", HeaderValue::from_static("no-cache"));

    Ok((headers, playlist).into_response())
}

/// Video variant playlist endpoint
/// GET /streams/{id}/video.m3u8
pub async fn video_playlist(
    State(state): State<Arc<AppState>>,
    Path(stream_id): Path<String>,
) -> Result<Response, HttpError> {
    let index = state.get_stream_or_error(&stream_id)?;

    let playlist = generate_video_playlist(&index);

    let mut headers = HeaderMap::new();
    headers.insert(
        "Content-Type",
        HeaderValue::from_static("application/vnd.apple.mpegurl"),
    );
    headers.insert("Cache-Control", HeaderValue::from_static("no-cache"));

    Ok((headers, playlist).into_response())
}

/// Audio variant playlist endpoint
/// GET /streams/{id}/audio/{track_index}.m3u8
pub async fn audio_playlist(
    State(state): State<Arc<AppState>>,
    Path((stream_id, track_index, force_aac)): Path<(String, usize, bool)>,
) -> Result<Response, HttpError> {
    let index = state.get_stream_or_error(&stream_id)?;

    let playlist =
        crate::playlist::variant::generate_audio_playlist(&index, track_index, force_aac);

    let mut headers = HeaderMap::new();
    headers.insert(
        "Content-Type",
        HeaderValue::from_static("application/vnd.apple.mpegurl"),
    );
    headers.insert("Cache-Control", HeaderValue::from_static("no-cache"));

    Ok((headers, playlist).into_response())
}

/// Subtitle variant playlist endpoint
/// GET /streams/{id}/sub/{track_index}.m3u8
pub async fn subtitle_playlist(
    State(state): State<Arc<AppState>>,
    Path((stream_id, track_index)): Path<(String, usize)>,
) -> Result<Response, HttpError> {
    let index = state.get_stream_or_error(&stream_id)?;

    let playlist = generate_subtitle_playlist(&index, track_index);

    let mut headers = HeaderMap::new();
    headers.insert(
        "Content-Type",
        HeaderValue::from_static("application/vnd.apple.mpegurl"),
    );
    headers.insert("Cache-Control", HeaderValue::from_static("no-cache"));

    Ok((headers, playlist).into_response())
}

/// Initialization segment endpoint
/// GET /streams/{id}/init.mp4
pub async fn init_segment(
    State(state): State<Arc<AppState>>,
    Path(stream_id): Path<String>,
) -> Result<Response, HttpError> {
    let index = state.get_stream_or_error(&stream_id)?;

    // Check cache first
    if let Some(data) = state.segment_cache.get(&stream_id, "init", 0) {
        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", HeaderValue::from_static("video/mp4"));
        headers.insert(
            "Cache-Control",
            HeaderValue::from_static("public, max-age=31536000"),
        );
        return Ok((headers, data).into_response());
    }

    // Generate init segment (blocking FFmpeg call — run on blocking thread pool)
    let data = tokio::task::spawn_blocking(move || generate_init_segment(&index))
        .await
        .map_err(|e| HttpError::InternalError(e.to_string()))?
        .map_err(|e| HttpError::InternalError(format!("Failed to generate init segment: {}", e)))?;

    // Cache the result
    state
        .segment_cache
        .insert(&stream_id, "init", 0, data.clone());

    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", HeaderValue::from_static("video/mp4"));
    headers.insert(
        "Cache-Control",
        HeaderValue::from_static("public, max-age=31536000"),
    );

    Ok((headers, data).into_response())
}

/// Video init segment endpoint (video track only)
/// GET /streams/{id}/video/init.mp4
pub async fn video_init_segment(
    State(state): State<Arc<AppState>>,
    Path(stream_id): Path<String>,
) -> Result<Response, HttpError> {
    let index = state.get_stream_or_error(&stream_id)?;

    // Check cache first
    if let Some(data) = state.segment_cache.get(&stream_id, "video_init", 0) {
        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", HeaderValue::from_static("video/mp4"));
        headers.insert(
            "Cache-Control",
            HeaderValue::from_static("public, max-age=31536000"),
        );
        return Ok((headers, data).into_response());
    }

    // Generate video-only init segment
    let data = tokio::task::spawn_blocking(move || {
        crate::segment::generate_video_init_segment(&index)
    })
    .await
    .map_err(|e| HttpError::InternalError(e.to_string()))?
    .map_err(|e| HttpError::InternalError(format!("Failed to generate video init segment: {}", e)))?;

    // Cache the result
    state
        .segment_cache
        .insert(&stream_id, "video_init", 0, data.clone());

    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", HeaderValue::from_static("video/mp4"));
    headers.insert(
        "Cache-Control",
        HeaderValue::from_static("public, max-age=31536000"),
    );

    Ok((headers, data).into_response())
}

/// Audio init segment endpoint (audio track only)
/// GET /streams/{id}/audio/{track_index}/init.mp4
pub async fn audio_init_segment(
    State(state): State<Arc<AppState>>,
    Path((stream_id, track_index, force_aac)): Path<(String, usize, bool)>,
) -> Result<Response, HttpError> {
    let index = state.get_stream_or_error(&stream_id)?;

    // Check cache first
    let cache_key = if force_aac {
        format!("audio_init_{}-aac", track_index)
    } else {
        format!("audio_init_{}", track_index)
    };
    if let Some(data) = state.segment_cache.get(&stream_id, &cache_key, 0) {
        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", HeaderValue::from_static("audio/mp4"));
        headers.insert(
            "Cache-Control",
            HeaderValue::from_static("public, max-age=31536000"),
        );
        return Ok((headers, data).into_response());
    }

    // Generate audio-only init segment (blocking FFmpeg call — run on blocking thread pool)
    let data = tokio::task::spawn_blocking(move || {
        crate::segment::generate_audio_init_segment(&index, track_index, force_aac)
    })
    .await
    .map_err(|e| HttpError::InternalError(e.to_string()))?
    .map_err(|e| HttpError::InternalError(format!("Failed to generate audio init segment: {}", e)))?;

    // Cache the result
    state
        .segment_cache
        .insert(&stream_id, &cache_key, 0, data.clone());

    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", HeaderValue::from_static("audio/mp4"));
    headers.insert(
        "Cache-Control",
        HeaderValue::from_static("public, max-age=31536000"),
    );

    Ok((headers, data).into_response())
}

/// Video segment endpoint
/// GET /streams/{id}/video/{sequence}.m4s
pub async fn video_segment(
    State(state): State<Arc<AppState>>,
    Path((stream_id, track_index, sequence)): Path<(String, usize, usize)>,
) -> Result<Response, HttpError> {
    // Check cache first
    let cache_key = format!("video_{}", track_index);
    if let Some(data) = state.segment_cache.get(&stream_id, &cache_key, sequence) {
        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", HeaderValue::from_static("video/mp4"));
        headers.insert(
            "Cache-Control",
            HeaderValue::from_static("public, max-age=31536000"),
        );
        return Ok((headers, data).into_response());
    }

    let index = state.get_stream_or_error(&stream_id)?;

    // Generate video segment (blocking FFmpeg call — run on blocking thread pool)
    let data = tokio::task::spawn_blocking(move || {
        crate::segment::generate_video_segment(&index, track_index, sequence, &index.source_path)
    })
    .await
    .map_err(|e| HttpError::InternalError(e.to_string()))?
    .map_err(|e| HttpError::InternalError(format!("Failed to generate video segment: {}", e)))?;

    // Cache the result
    state
        .segment_cache
        .insert(&stream_id, &cache_key, sequence, data.clone());

    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", HeaderValue::from_static("video/mp4"));
    headers.insert(
        "Cache-Control",
        HeaderValue::from_static("public, max-age=31536000"),
    );

    Ok((headers, data).into_response())
}

/// Audio segment endpoint
/// GET /streams/{id}/audio/{track_index}/{sequence}.m4s
pub async fn audio_segment(
    State(state): State<Arc<AppState>>,
    Path((stream_id, track_index, sequence, force_aac)): Path<(String, usize, usize, bool)>,
) -> Result<Response, HttpError> {
    // Check cache first
    let segment_type = if force_aac {
        format!("audio_{}-aac", track_index)
    } else {
        format!("audio_{}", track_index)
    };
    if let Some(data) = state.segment_cache.get(&stream_id, &segment_type, sequence) {
        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", HeaderValue::from_static("audio/mp4"));
        headers.insert(
            "Cache-Control",
            HeaderValue::from_static("public, max-age=31536000"),
        );
        return Ok((headers, data).into_response());
    }

    let index = state.get_stream_or_error(&stream_id)?;

    // Generate audio segment (blocking FFmpeg call — run on blocking thread pool)
    let data = tokio::task::spawn_blocking(move || {
        crate::segment::generate_audio_segment(&index, track_index, sequence, &index.source_path, force_aac)
    })
    .await
    .map_err(|e| HttpError::InternalError(e.to_string()))?
    .map_err(|e| HttpError::InternalError(format!("Failed to generate audio segment: {}", e)))?;

    // Cache the result
    state
        .segment_cache
        .insert(&stream_id, &segment_type, sequence, data.clone());

    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", HeaderValue::from_static("audio/mp4"));
    headers.insert(
        "Cache-Control",
        HeaderValue::from_static("public, max-age=31536000"),
    );

    Ok((headers, data).into_response())
}

/// Subtitle segment endpoint
/// GET /streams/{id}/sub/{track_index}/{start_seq}-{end_seq}.vtt
pub async fn subtitle_segment(
    State(state): State<Arc<AppState>>,
    Path((stream_id, track_index, start_seq, end_seq)): Path<(String, usize, usize, usize)>,
) -> Result<Response, HttpError> {
    // Check cache first
    let segment_type = format!("sub_{}_{}", track_index, end_seq);
    if let Some(data) = state
        .segment_cache
        .get(&stream_id, &segment_type, start_seq)
    {
        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", HeaderValue::from_static("text/vtt"));
        headers.insert(
            "Cache-Control",
            HeaderValue::from_static("public, max-age=31536000"),
        );
        return Ok((headers, data).into_response());
    }

    let index = state.get_stream_or_error(&stream_id)?;

    // Generate subtitle segment (blocking FFmpeg call — run on blocking thread pool)
    let data = tokio::task::spawn_blocking(move || {
        crate::segment::generate_subtitle_segment(&index, track_index, start_seq, end_seq, &index.source_path)
    })
    .await
    .map_err(|e| HttpError::InternalError(e.to_string()))?
    .map_err(|e| HttpError::InternalError(format!("Failed to generate subtitle segment: {}", e)))?;

    // Cache the result
    state
        .segment_cache
        .insert(&stream_id, &segment_type, start_seq, data.clone());

    let mut headers = HeaderMap::new();
    headers.insert("Content-Type", HeaderValue::from_static("text/vtt"));
    headers.insert(
        "Cache-Control",
        HeaderValue::from_static("public, max-age=31536000"),
    );

    Ok((headers, data).into_response())
}

/// Debug endpoint - cache statistics
pub async fn cache_stats(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let stats = state.segment_cache.stats();

    Json(serde_json::json!({
        "entry_count": stats.entry_count,
        "total_size_bytes": stats.total_size_bytes,
        "memory_limit_bytes": stats.memory_limit_bytes,
        "oldest_entry_age_secs": stats.oldest_entry_age_secs,
        "utilization": format!("{:.1}%",
            (stats.total_size_bytes as f64 / stats.memory_limit_bytes as f64) * 100.0
        )
    }))
}

/// Debug endpoint - active streams
pub async fn active_streams(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let streams: Vec<_> = state
        .streams
        .iter()
        .map(|r| {
            serde_json::json!({
                "stream_id": r.stream_id,
                "source_path": r.source_path.to_string_lossy().to_string(),
                "duration_secs": r.duration_secs,
                "video_streams": r.video_streams.len(),
                "audio_streams": r.audio_streams.len(),
                "subtitle_streams": r.subtitle_streams.len(),
                "segments": r.segments.len(),
            })
        })
        .collect();

    Json(serde_json::json!({
        "count": streams.len(),
        "streams": streams,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_check() {
        // Can't easily test async functions without tokio test runtime
        // Just verify the function exists and has correct signature
        let _fn: fn() -> _ = health_check;
    }

    #[test]
    fn test_version_check() {
        let _fn: fn() -> _ = version_check;
    }
}
