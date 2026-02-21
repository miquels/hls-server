//! Stream management handlers
//!
//! Handles stream creation, listing, and deletion.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::state::AppState;
use crate::index::scanner::scan_file;
use super::handlers::{HttpError, AppStateExt};

/// Request to create a new stream
#[derive(Debug, Deserialize)]
pub struct CreateStreamRequest {
    /// Path to the media file
    pub path: String,
    /// Optional custom stream ID (generated if not provided)
    pub stream_id: Option<String>,
}

/// Response after creating a stream
#[derive(Debug, Serialize)]
pub struct CreateStreamResponse {
    /// Unique stream identifier
    pub stream_id: String,
    /// Path to master playlist
    pub master_playlist: String,
    /// Path to video playlist
    pub video_playlist: String,
    /// Duration in seconds
    pub duration_secs: f64,
    /// Number of video streams
    pub video_count: usize,
    /// Number of audio streams
    pub audio_count: usize,
    /// Number of subtitle streams
    pub subtitle_count: usize,
    /// Number of segments
    pub segment_count: usize,
}

/// List of active streams
#[derive(Debug, Serialize)]
pub struct StreamListResponse {
    pub count: usize,
    pub streams: Vec<StreamInfo>,
}

#[derive(Debug, Serialize)]
pub struct StreamInfo {
    pub stream_id: String,
    pub source_path: String,
    pub duration_secs: f64,
    pub video_count: usize,
    pub audio_count: usize,
    pub subtitle_count: usize,
    pub segment_count: usize,
}

/// Query parameters for stream listing
#[derive(Debug, Deserialize)]
pub struct StreamListQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

/// Create a new stream from a media file
/// POST /streams
pub async fn create_stream(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CreateStreamRequest>,
) -> Response {
    // Check if file exists
    let file_path = std::path::Path::new(&request.path);
    if !file_path.exists() {
        return HttpError::StreamNotFound(format!(
            "File not found: {}",
            request.path
        )).into_response();
    }

    // Check if file is already registered
    let canonical_path = match file_path.canonicalize() {
        Ok(p) => p,
        Err(e) => return HttpError::InternalError(format!("Failed to resolve path: {}", e)).into_response(),
    };
    
    if let Some(existing) = state.get_stream_by_path(canonical_path.to_string_lossy().as_ref()) {
        // Return existing stream
        let response = CreateStreamResponse {
            stream_id: existing.stream_id.clone(),
            master_playlist: format!("/streams/{}/master.m3u8", existing.stream_id),
            video_playlist: format!("/streams/{}/video.m3u8", existing.stream_id),
            duration_secs: existing.duration_secs,
            video_count: existing.video_streams.len(),
            audio_count: existing.audio_streams.len(),
            subtitle_count: existing.subtitle_streams.len(),
            segment_count: existing.segments.len(),
        };
        return (StatusCode::OK, Json(response)).into_response();
    }

    // Scan and index the file
    let index = match scan_file(&canonical_path) {
        Ok(i) => i,
        Err(e) => return HttpError::InternalError(format!("Failed to index file: {}", e)).into_response(),
    };

    let stream_id = index.stream_id.clone();
    let duration_secs = index.duration_secs;
    let video_count = index.video_streams.len();
    let audio_count = index.audio_streams.len();
    let subtitle_count = index.subtitle_streams.len();
    let segment_count = index.segments.len();

    // Register the stream
    state.register_stream(index);

    let response = CreateStreamResponse {
        stream_id: stream_id.clone(),
        master_playlist: format!("/streams/{}/master.m3u8", stream_id),
        video_playlist: format!("/streams/{}/video.m3u8", stream_id),
        duration_secs,
        video_count,
        audio_count,
        subtitle_count,
        segment_count,
    };

    (StatusCode::CREATED, Json(response)).into_response()
}

/// List all active streams
/// GET /streams
pub async fn list_streams(
    State(state): State<Arc<AppState>>,
    Query(query): Query<StreamListQuery>,
) -> Json<StreamListResponse> {
    let streams: Vec<_> = state.streams.iter()
        .map(|r| StreamInfo {
            stream_id: r.stream_id.clone(),
            source_path: r.source_path.to_string_lossy().to_string(),
            duration_secs: r.duration_secs,
            video_count: r.video_streams.len(),
            audio_count: r.audio_streams.len(),
            subtitle_count: r.subtitle_streams.len(),
            segment_count: r.segments.len(),
        })
        .collect();

    let total = streams.len();
    
    // Apply pagination
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(total);
    let streams: Vec<_> = streams.into_iter()
        .skip(offset)
        .take(limit)
        .collect();

    Json(StreamListResponse {
        count: streams.len(),
        streams,
    })
}

/// Get stream details
/// GET /streams/:id
pub async fn get_stream(
    State(state): State<Arc<AppState>>,
    Path(stream_id): Path<String>,
) -> Response {
    match state.get_stream_or_error(&stream_id) {
        Ok(index) => Json(StreamInfo {
            stream_id: index.stream_id.clone(),
            source_path: index.source_path.to_string_lossy().to_string(),
            duration_secs: index.duration_secs,
            video_count: index.video_streams.len(),
            audio_count: index.audio_streams.len(),
            subtitle_count: index.subtitle_streams.len(),
            segment_count: index.segments.len(),
        }).into_response(),
        Err(e) => e.into_response(),
    }
}

/// Delete a stream
/// DELETE /streams/:id
pub async fn delete_stream(
    State(state): State<Arc<AppState>>,
    Path(stream_id): Path<String>,
) -> impl IntoResponse {
    // Check if stream exists
    match state.get_stream_or_error(&stream_id) {
        Ok(_) => {
            // Remove from state
            state.remove_stream(&stream_id);
            // Clear cache
            state.segment_cache.remove_stream(&stream_id);
            (StatusCode::NO_CONTENT).into_response()
        }
        Err(e) => e.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_stream_request() {
        let json = r#"{"path": "/test/video.mp4"}"#;
        let request: CreateStreamRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.path, "/test/video.mp4");
        assert!(request.stream_id.is_none());
    }

    #[test]
    fn test_create_stream_response() {
        let response = CreateStreamResponse {
            stream_id: "test-123".to_string(),
            master_playlist: "/streams/test-123/master.m3u8".to_string(),
            video_playlist: "/streams/test-123/video.m3u8".to_string(),
            duration_secs: 120.5,
            video_count: 1,
            audio_count: 2,
            subtitle_count: 1,
            segment_count: 30,
        };
        assert_eq!(response.stream_id, "test-123");
        assert_eq!(response.video_count, 1);
    }
}
