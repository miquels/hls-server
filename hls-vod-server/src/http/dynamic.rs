use axum::{
    extract::{Path, State},
    response::Response,
};
use std::path::PathBuf;
use std::sync::Arc;

use super::handlers::HttpError;
use crate::state::AppState;
use tracing::info;

/// Parse a dynamic path into a media file path and a suffix
pub fn parse_path(full_path: &str) -> Option<(PathBuf, String)> {
    let full_path = if full_path.starts_with('/') {
        full_path.to_string()
    } else {
        format!("/{}", full_path)
    };
    let parts: Vec<&str> = full_path.split('/').filter(|s| !s.is_empty()).collect();

    // Check for explicit .as.m3u8 request on a media file
    if full_path.ends_with(".as.m3u8") {
        let media_path_str = full_path.strip_suffix(".as.m3u8").unwrap();
        let media_path = PathBuf::from(media_path_str);

        // Ensure the media path has a recognized extension
        if media_path_str.ends_with(".mp4")
            || media_path_str.ends_with(".mkv")
            || media_path_str.ends_with(".webm")
        {
            return Some((media_path, "master.m3u8".to_string()));
        }
    }

    for (i, part) in parts.iter().enumerate().rev() {
        let part_lower = part.to_lowercase();

        // Ignore init segments, they share the .mp4 extension but are part of the HLS suffix
        if part_lower.ends_with(".init.mp4") {
            continue;
        }

        if part_lower.ends_with(".mp4")
            || part_lower.ends_with(".mkv")
            || part_lower.ends_with(".webm")
        {
            let media_path_str = "/".to_owned() + &parts[..=i].join("/");
            let suffix = parts[i + 1..].join("/");
            return Some((PathBuf::from(media_path_str), suffix));
        }
    }

    None
}

/// Dynamic request handler mapped to `/*path`
pub async fn handle_dynamic_request(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
) -> Result<Response, HttpError> {
    let (media_path, suffix) = parse_path(&path).ok_or_else(|| {
        HttpError::SegmentNotFound(format!(
            "Invalid path format or missing media file: {}",
            path
        ))
    })?;

    let path_str = media_path.to_string_lossy().to_string();

    let media = if let Some(media) = state.get_stream_by_path(&path_str) {
        media.index.touch();
        media
    } else {
        if !media_path.exists() {
            return Err(HttpError::StreamNotFound(format!(
                "Media file not found: {}",
                path_str
            )));
        }

        // Deduplicate concurrent indexing of the same file
        let cell = state
            .indexing_in_flight
            .entry(path_str.clone())
            .or_insert_with(|| std::sync::Arc::new(tokio::sync::OnceCell::new()))
            .clone();

        let arc = cell
            .get_or_try_init(|| {
                let state2 = state.clone();
                let path_str2 = path_str.clone();
                let media_path2 = media_path.clone();
                async move {
                    info!("Indexing new file: {:?}", media_path2);
                    let new_media = tokio::task::spawn_blocking(move || {
                        hls_vod_lib::api::parse_file(&media_path2, true)
                    })
                    .await
                    .map_err(|e| HttpError::InternalError(e.to_string()))?
                    .map_err(|e| {
                        HttpError::InternalError(format!("Failed to index file: {}", e))
                    })?;
                    // Re-check in case another request registered it while we were scanning
                    if let Some(existing) = state2.get_stream_by_path(&path_str2) {
                        return Ok::<Arc<hls_vod_lib::MediaInfo>, HttpError>(existing);
                    }
                    Ok(state2.register_stream(new_media))
                }
            })
            .await?
            .clone();

        // Remove the in-flight entry now that the result is in the main streams map
        state.indexing_in_flight.remove(&path_str);

        arc
    };

    let stream_id = media.index.stream_id.clone();

    if suffix == "master.m3u8" {
        return super::handlers::master_playlist(State(state), Path(stream_id)).await;
    } else if suffix == "v/media.m3u8" {
        return super::handlers::video_playlist(State(state), Path(stream_id)).await;
    } else if let Some(sub) = suffix.strip_prefix("v/") {
        if let Some(_track_str) = sub.strip_suffix(".init.mp4") {
            // we could parse track_index but video_init_segment generates for primary video
            return super::handlers::video_init_segment(State(state), Path(stream_id)).await;
        } else if let Some(rest) = sub.strip_suffix(".m4s") {
            let parts: Vec<&str> = rest.split('.').collect();
            if parts.len() == 2 {
                let track = parts[0]
                    .parse::<usize>()
                    .map_err(|_| HttpError::SegmentNotFound("Invalid video track".into()))?;
                let seq = parts[1]
                    .parse::<usize>()
                    .map_err(|_| HttpError::SegmentNotFound("Invalid video seq".into()))?;
                return super::handlers::video_segment(State(state), Path((stream_id, track, seq)))
                    .await;
            }
        }
    } else if let Some(sub) = suffix.strip_prefix("a/") {
        if let Some(mut track_str) = sub.strip_suffix(".m3u8") {
            let mut force_aac = false;
            if let Some(base) = track_str.strip_suffix("-aac") {
                track_str = base;
                force_aac = true;
            }
            let track = track_str
                .parse::<usize>()
                .map_err(|_| HttpError::SegmentNotFound("Invalid audio track".into()))?;
            return super::handlers::audio_playlist(
                State(state),
                Path((stream_id, track, force_aac)),
            )
            .await;
        } else if let Some(mut track_str) = sub.strip_suffix(".init.mp4") {
            let mut force_aac = false;
            if let Some(base) = track_str.strip_suffix("-aac") {
                track_str = base;
                force_aac = true;
            }
            let track = track_str
                .parse::<usize>()
                .map_err(|_| HttpError::SegmentNotFound("Invalid audio track".into()))?;
            return super::handlers::audio_init_segment(
                State(state),
                Path((stream_id, track, force_aac)),
            )
            .await;
        } else if let Some(rest) = sub.strip_suffix(".m4s") {
            let parts: Vec<&str> = rest.split('.').collect();
            if parts.len() == 2 {
                let mut track_str = parts[0];
                let mut force_aac = false;
                if let Some(base) = track_str.strip_suffix("-aac") {
                    track_str = base;
                    force_aac = true;
                }
                let track = track_str
                    .parse::<usize>()
                    .map_err(|_| HttpError::SegmentNotFound("Invalid audio track".into()))?;
                let seq = parts[1]
                    .parse::<usize>()
                    .map_err(|_| HttpError::SegmentNotFound("Invalid audio seq".into()))?;
                return super::handlers::audio_segment(
                    State(state),
                    Path((stream_id, track, seq, force_aac)),
                )
                .await;
            }
        }
    } else if let Some(sub) = suffix.strip_prefix("s/") {
        if let Some(track_str) = sub.strip_suffix(".m3u8") {
            let track = track_str
                .parse::<usize>()
                .map_err(|_| HttpError::SegmentNotFound("Invalid subtitle track".into()))?;
            return super::handlers::subtitle_playlist(State(state), Path((stream_id, track)))
                .await;
        } else if let Some(rest) = sub.strip_suffix(".vtt") {
            let parts: Vec<&str> = rest.split('.').collect();
            if parts.len() == 2 {
                let track = parts[0]
                    .parse::<usize>()
                    .map_err(|_| HttpError::SegmentNotFound("Invalid subtitle track".into()))?;

                // Sequence can be "5" or "5-10"
                let seq_part = parts[1];
                let (start_seq, end_seq) = if seq_part.contains('-') {
                    let mut seq_parts = seq_part.split('-');
                    let start = seq_parts.next().unwrap().parse::<usize>().unwrap_or(0);
                    let end = seq_parts.next().unwrap().parse::<usize>().unwrap_or(start);
                    (start, end)
                } else {
                    let seq = seq_part.parse::<usize>().unwrap_or(0);
                    (seq, seq)
                };

                // The handler logic only needs the precise range of segments to generate.
                // We'll update handlers::subtitle_segment to take (start_seq, end_seq).
                return super::handlers::subtitle_segment(
                    State(state),
                    Path((stream_id, track, start_seq, end_seq)),
                )
                .await;
            }
        }
    }

    Err(HttpError::SegmentNotFound(format!(
        "Unknown suffix: {}",
        suffix
    )))
}
