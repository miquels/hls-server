use axum::{
    extract::State,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use std::sync::Arc;
use tracing::debug;

use crate::state::AppState;
use bytes::Bytes;
use hls_vod_lib::HlsError;
use hls_vod_lib::MediaInfo;

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

/// Helper extension trait for AppState
pub trait AppStateExt {
    fn get_media_or_error(&self, stream_id: &str) -> Result<Arc<MediaInfo>, HttpError>;
}

impl AppStateExt for AppState {
    fn get_media_or_error(&self, stream_id: &str) -> Result<Arc<MediaInfo>, HttpError> {
        self.get_stream(stream_id)
            .ok_or_else(|| HttpError::StreamNotFound(format!("Stream {} not found", stream_id)))
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

/// Master playlist endpoint
/// Master playlist logic
pub async fn master_playlist(
    state: &AppState,
    stream_id: &str,
    prefix: &str,
) -> Result<Response, HttpError> {
    let media = state.get_media_or_error(stream_id)?;

    let playlist = hls_vod_lib::generate_main_playlist(&media, prefix)?;

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/vnd.apple.mpegurl"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));

    Ok((headers, playlist).into_response())
}

/// Video variant playlist endpoint
/// Video variant playlist logic
pub async fn video_playlist(state: &AppState, stream_id: &str) -> Result<Response, HttpError> {
    let media = state.get_media_or_error(stream_id)?;

    let playlist_id = "v/media.m3u8";
    let playlist = hls_vod_lib::generate_track_playlist(&media, playlist_id)?;

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/vnd.apple.mpegurl"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));

    Ok((headers, playlist).into_response())
}

/// Audio variant playlist endpoint
/// Audio variant playlist logic
pub async fn audio_playlist(
    state: &AppState,
    stream_id: &str,
    track_index: usize,
    force_aac: bool,
) -> Result<Response, HttpError> {
    let media = state.get_media_or_error(stream_id)?;

    let playlist_id = if force_aac {
        format!("a/{}-aac.m3u8", track_index)
    } else {
        format!("a/{}.m3u8", track_index)
    };

    let playlist = hls_vod_lib::generate_track_playlist(&media, &playlist_id)?;

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/vnd.apple.mpegurl"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));

    Ok((headers, playlist).into_response())
}

/// Subtitle variant playlist endpoint
/// Subtitle variant playlist logic
pub async fn subtitle_playlist(
    state: &AppState,
    stream_id: &str,
    track_index: usize,
) -> Result<Response, HttpError> {
    let media = state.get_media_or_error(stream_id)?;

    let playlist_id = format!("s/{}.m3u8", track_index);
    let playlist = hls_vod_lib::generate_track_playlist(&media, &playlist_id)?;

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/vnd.apple.mpegurl"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));

    Ok((headers, playlist).into_response())
}

/// Video init segment endpoint (video track only)
/// Video init segment logic
pub async fn video_init_segment(state: &AppState, stream_id: &str) -> Result<Response, HttpError> {
    let media = state.get_media_or_error(stream_id)?;

    let segment_id = "v/init.mp4";
    let bytes = hls_vod_lib::generate_segment(&media, segment_id)?;

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("video/mp4"));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("max-age=3600"),
    );

    Ok((headers, bytes).into_response())
}

/// Audio init segment endpoint
/// Audio init segment logic
pub async fn audio_init_segment(
    state: &AppState,
    stream_id: &str,
    track_index: usize,
    force_aac: bool,
) -> Result<Response, HttpError> {
    let media = state.get_media_or_error(stream_id)?;

    let segment_id = if force_aac {
        format!("a/{}-aac/init.mp4", track_index)
    } else {
        format!("a/{}/init.mp4", track_index)
    };

    let bytes = hls_vod_lib::generate_segment(&media, &segment_id)?;

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("video/mp4"));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("max-age=3600"),
    );

    Ok((headers, bytes).into_response())
}

/// Video media segment endpoint
/// Video media segment logic
pub async fn video_segment(
    state: &AppState,
    stream_id: &str,
    sequence: usize,
) -> Result<Response, HttpError> {
    let media = state.get_media_or_error(stream_id)?;

    // Cache lookup
    if let Some(bytes) = state.segment_cache.get(stream_id, "v", sequence) {
        debug!("Cache hit for video segment: v:{}", sequence);
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("video/iso.segment"),
        );
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("max-age=3600"),
        );
        return Ok((headers, bytes).into_response());
    }

    let segment_id = format!("v/{}.m4s", sequence);
    let bytes = hls_vod_lib::generate_segment(&media, &segment_id)?;

    // Update cache
    state
        .segment_cache
        .insert(stream_id, "v", sequence, Bytes::from(bytes.clone()));

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("video/iso.segment"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("max-age=3600"),
    );

    Ok((headers, bytes).into_response())
}

/// Audio media segment endpoint
/// Audio media segment logic
pub async fn audio_segment(
    state: &AppState,
    stream_id: &str,
    track_index: usize,
    sequence: usize,
    force_aac: bool,
) -> Result<Response, HttpError> {
    let media = state.get_media_or_error(stream_id)?;

    // Cache lookup
    let segment_type = if force_aac {
        format!("a:{}-aac", track_index)
    } else {
        format!("a:{}", track_index)
    };

    if let Some(bytes) = state.segment_cache.get(stream_id, &segment_type, sequence) {
        debug!("Cache hit for audio segment: {}:{}", segment_type, sequence);
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("video/iso.segment"),
        );
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("max-age=3600"),
        );
        return Ok((headers, bytes).into_response());
    }

    let segment_id = if force_aac {
        format!("a/{}-aac/{}.m4s", track_index, sequence)
    } else {
        format!("a/{}/{}.m4s", track_index, sequence)
    };

    let bytes = hls_vod_lib::generate_segment(&media, &segment_id)?;

    // Update cache
    state.segment_cache.insert(
        stream_id,
        &segment_type,
        sequence,
        Bytes::from(bytes.clone()),
    );

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("video/iso.segment"),
    );
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("max-age=3600"),
    );

    Ok((headers, bytes).into_response())
}

/// Subtitle media segment endpoint
/// Subtitle media segment logic
pub async fn subtitle_segment(
    state: &AppState,
    stream_id: &str,
    track_index: usize,
    start_seq: usize,
    end_seq: usize,
) -> Result<Response, HttpError> {
    let media = state.get_media_or_error(stream_id)?;

    let segment_id = format!("s/{}/{}-{}.vtt", track_index, start_seq, end_seq);

    let bytes = hls_vod_lib::generate_segment(&media, &segment_id)?;

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/vtt"));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("max-age=3600"),
    );

    Ok((headers, bytes).into_response())
}

/// Debug endpoint: cache statistics
pub async fn cache_stats(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let stats = state.segment_cache.stats();
    Json(serde_json::json!({
        "size": stats.entry_count,
        "memory_usage": stats.total_size_bytes,
        "capacity": stats.memory_limit_bytes,
    }))
}

/// Debug endpoint: active streams
pub async fn active_streams(State(state): State<Arc<AppState>>) -> Json<Vec<ActiveStreamInfo>> {
    let streams = state
        .streams
        .iter()
        .map(|r| ActiveStreamInfo {
            stream_id: r.index.stream_id.clone(),
            path: r.index.source_path.to_string_lossy().to_string(),
            duration: r.index.duration_secs,
        })
        .collect();
    Json(streams)
}

#[derive(Serialize)]
pub struct ActiveStreamInfo {
    pub stream_id: String,
    pub path: String,
    pub duration: f64,
}
