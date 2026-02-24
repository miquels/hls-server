use crate::state::AppState;
use axum::{
    extract::State,
    http::{header, HeaderMap, HeaderValue, StatusCode},
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

/// Master playlist endpoint
/// Master playlist logic
pub async fn master_playlist(
    state: &AppState,
    stream_id: &str,
    prefix: &str,
) -> Result<Response, HttpError> {
    let playlist = hls_vod_lib::generate_main_playlist(stream_id, prefix)?;

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
    let playlist_id = "v/media.m3u8";
    let playlist = hls_vod_lib::generate_track_playlist(stream_id, playlist_id)?;

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
    let suffix = if force_aac { "-aac" } else { "" };
    let playlist_id = format!("a/{}{}/media.m3u8", track_index, suffix);
    let playlist = hls_vod_lib::generate_track_playlist(stream_id, &playlist_id)?;

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
    let playlist_id = format!("s/{}/media.m3u8", track_index);
    let playlist = hls_vod_lib::generate_track_playlist(stream_id, &playlist_id)?;

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
    let segment_id = "v/init.mp4";
    let bytes = hls_vod_lib::generate_segment(stream_id, segment_id)?;

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
    let suffix = if force_aac { "-aac" } else { "" };
    let segment_id = format!("a/{}{}/init.mp4", track_index, suffix);
    let bytes = hls_vod_lib::generate_segment(stream_id, &segment_id)?;

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
    let segment_id = format!("v/{}.m4s", sequence);
    let bytes = hls_vod_lib::generate_segment(stream_id, &segment_id)?;

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
    let suffix = if force_aac { "-aac" } else { "" };
    let segment_id = format!("a/{}{}/{}.m4s", track_index, suffix, sequence);
    let bytes = hls_vod_lib::generate_segment(stream_id, &segment_id)?;

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
    let segment_id = format!("s/{}/{}-{}.vtt", track_index, start_seq, end_seq);

    let bytes = hls_vod_lib::generate_segment(stream_id, &segment_id)?;

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
