use std::sync::Arc;

use super::handlers::HttpError;
use crate::state::AppState;
use axum::http::{header, HeaderMap, HeaderValue};
use axum::response::IntoResponse;
use hls_vod_lib::url::UrlType;

/// Dynamic request handler mapped to `/*path`
pub async fn handle_dynamic_request(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::extract::Path(path): axum::extract::Path<String>,
    axum::extract::Query(query_params): axum::extract::Query<
        std::collections::HashMap<String, String>,
    >,
) -> Result<axum::response::Response, HttpError> {
    let hls_url = hls_vod_lib::url::HlsUrl::parse(&path).ok_or_else(|| {
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

    let codecs: Vec<String> = query_params
        .get("codecs")
        .map(|s| s.split(',').map(|c| c.trim().to_string()).collect())
        .unwrap_or_default();

    if !media_path.exists() {
        return Err(HttpError::StreamNotFound(format!(
            "Media file not found: {}",
            media_path_str
        )));
    }

    let dedup_key = if codecs.is_empty() {
        media_path_str.clone()
    } else {
        format!("{}|{}", media_path_str, codecs.join(","))
    };

    let stream_id = hls_url.session_id.clone();

    let media = {
        let cell = state
            .indexing_in_flight
            .entry(dedup_key.clone())
            .or_insert_with(|| Arc::new(tokio::sync::OnceCell::new()))
            .clone();

        cell.get_or_try_init(|| {
            let media_path2 = media_path.clone();
            let dedup_key2 = dedup_key.clone();
            let state2 = state.clone();
            let sid = stream_id.clone();
            let codecs_clone = codecs.clone();
            async move {
                tracing::info!("Opening media: {:?} (stream_id: {:?})", media_path2, sid);
                let result = tokio::task::spawn_blocking(move || {
                    let codecs_refs: Vec<&str> = codecs_clone.iter().map(|s| s.as_str()).collect();
                    hls_vod_lib::StreamIndex::open(&media_path2, &codecs_refs, sid)
                })
                .await
                .map_err(|e| HttpError::InternalError(e.to_string()))?
                .map_err(|e| HttpError::InternalError(format!("Failed to open media: {}", e)));

                if result.is_err() {
                    state2.indexing_in_flight.remove(&dedup_key2);
                }
                result
            }
        })
        .await?
        .clone()
    };

    // Output generation
    // Parse 'interleave' query parameter (default: false)
    let interleave = query_params
        .get("interleave")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);

    // Parse 'codecs' query parameter to check if AAC is explicitly requested
    let codecs_param = query_params.get("codecs").map(|s| s.as_str()).unwrap_or("");
    let force_aac_in_interleave = interleave && codecs_param.to_lowercase().contains("aac");

    let bytes = hls_url
        .generate(&media, interleave, force_aac_in_interleave)
        .map_err(|e| HttpError::InternalError(e.to_string()))?;

    let mut headers = HeaderMap::new();

    match hls_url.url_type {
        UrlType::MainPlaylist | UrlType::Playlist(_) => {
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/vnd.apple.mpegurl"),
            );
            headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        }
        UrlType::VideoSegment(v) => {
            if v.segment_id.is_none() {
                headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("video/mp4"));
            } else {
                headers.insert(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static("video/iso.segment"),
                );
            }
            headers.insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("max-age=3600"),
            );
        }
        UrlType::AudioSegment(a) => {
            if a.segment_id.is_none() {
                headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("video/mp4"));
            } else {
                headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("audio/mp4"));
            }
            headers.insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("max-age=3600"),
            );
        }
        UrlType::VttSegment(_) => {
            headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/vtt"));
            headers.insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("max-age=3600"),
            );
        }
    }

    Ok((headers, bytes).into_response())
}
