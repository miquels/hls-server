use std::sync::Arc;

use super::handlers::HttpError;
use crate::state::AppState;
use axum::http::{header, HeaderMap, HeaderValue};
use axum::response::IntoResponse;
use hls_vod_lib::HlsVideo;

/// Dynamic request handler mapped to `/*path`
pub async fn handle_dynamic_request(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::extract::Path(path): axum::extract::Path<String>,
    axum::extract::Query(query_params): axum::extract::Query<
        std::collections::HashMap<String, String>,
    >,
) -> Result<axum::response::Response, HttpError> {
    // Decode the URL.
    let hls_url = hls_vod_lib::HlsParams::parse(&path).ok_or_else(|| {
        HttpError::SegmentNotFound(format!(
            "Invalid path format or unsupported HLS request: {}",
            path
        ))
    })?;

    // We simply take the url path as the path to the video.
    let media_path_str = format!("/{}", &hls_url.video_url);
    let media_path = std::path::PathBuf::from(&media_path_str);

    // All code is sync, so spawn it in a separate thread.
    tokio::task::spawn_blocking(move || {
        if !media_path.exists() {
            return Err(HttpError::StreamNotFound(format!(
                "Media file not found: {}",
                hls_url.video_url,
            )));
        }

        tracing::info!(
            "Opening media: {:?} (stream_id: {:?})",
            media_path,
            hls_url.session_id
        );
        let mut hls_video = HlsVideo::open(&media_path, hls_url)
            .map_err(|e| HttpError::InternalError(format!("Failed to open media: {}", e)))?;

        if let HlsVideo::MainPlaylist(p) = &mut hls_video {
            let codecs: Vec<String> = query_params
                .get("codecs")
                .map(|s| s.split(',').map(|c| c.trim().to_string()).collect())
                .unwrap_or_default();
            p.filter_codecs(&codecs);

            let tracks: Vec<usize> = query_params
                .get("tracks")
                .map(|s| {
                    s.split(',')
                        .filter_map(|s| s.parse::<usize>().ok())
                        .collect::<Vec<usize>>()
                })
                .unwrap_or_default();
            if !tracks.is_empty() {
                p.enable_tracks(&tracks);
            }

            if query_params
                .get("interleave")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false)
            {
                p.interleave();
            }
        }

        let mut headers = HeaderMap::new();

        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(hls_video.mime_type()),
        );
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static(hls_video.cache_control()),
        );

        let bytes = hls_video
            .generate()
            .map_err(|e| HttpError::InternalError(e.to_string()))?;

        Ok((headers, bytes).into_response())
    })
    .await
    .map_err(|e| HttpError::InternalError(e.to_string()))?
}
