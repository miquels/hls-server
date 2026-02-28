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
    let hls_url = hls_vod_lib::HlsUrl::parse(&path).ok_or_else(|| {
        HttpError::SegmentNotFound(format!(
            "Invalid path format or unsupported HLS request: {}",
            path
        ))
    })?;

    // Since `HlsUrl::parse` handles the decoding of media file and session
    let media_path_str = if hls_url.video_url.starts_with('/') {
        hls_url.video_url.clone()
    } else {
        format!("/{}", hls_url.video_url)
    };
    let media_path = std::path::PathBuf::from(&media_path_str);

    if !media_path.exists() {
        return Err(HttpError::StreamNotFound(format!(
            "Media file not found: {}",
            media_path_str
        )));
    }

    let mut hls_video = {
        let media_path2 = media_path.clone();
        let sid = hls_url.session_id.clone();

        async move {
            tracing::info!("Opening media: {:?} (stream_id: {:?})", media_path2, sid);
            tokio::task::spawn_blocking(move || {
                HlsVideo::new(&media_path2, hls_url)
            })
            .await
            .map_err(|e| HttpError::InternalError(e.to_string()))?
            .map_err(|e| HttpError::InternalError(format!("Failed to open media: {}", e)))
        }
        .await?
    };

    if let HlsVideo::MainPlaylist(p) = &mut hls_video {
        let codecs: Vec<String> = query_params
            .get("codecs")
            .map(|s| s.split(',').map(|c| c.trim().to_string()).collect())
            .unwrap_or_default();
        p.filter_codecs(&codecs);
        let interleave = query_params
            .get("interleave")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
        if interleave {
            p.interleave();
        }
    }

    let bytes = hls_video
        .generate()
        .map_err(|e| HttpError::InternalError(e.to_string()))?;

    let mut headers = HeaderMap::new();

    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(hls_video.mime_type()),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(hls_video.cache_control()));

    Ok((headers, bytes).into_response())
}
