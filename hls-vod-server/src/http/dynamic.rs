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
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::extract::Path(path): axum::extract::Path<String>,
    axum::extract::Query(query_params): axum::extract::Query<
        std::collections::HashMap<String, String>,
    >,
) -> Result<axum::response::Response, HttpError> {
    let (media_path, suffix) = parse_path(&path).ok_or_else(|| {
        HttpError::SegmentNotFound(format!(
            "Invalid path format or missing media file: {}",
            path
        ))
    })?;

    let codecs: Vec<String> = query_params
        .get("codecs")
        .map(|s| s.split(',').map(|c| c.trim().to_string()).collect())
        .unwrap_or_default();

    let path_str = media_path.to_string_lossy().to_string();

    if !media_path.exists() {
        return Err(HttpError::StreamNotFound(format!(
            "Media file not found: {}",
            path_str
        )));
    }

    // If suffix is NOT master.m3u8, it MUST start with a stream_id.
    // E.g. "STREAMID/v/media.m3u8"
    let (stream_id, sub_suffix) = if suffix == "master.m3u8" {
        (None, suffix.clone())
    } else {
        let parts: Vec<&str> = suffix.splitn(2, '/').collect();
        if parts.len() != 2 {
            return Err(HttpError::SegmentNotFound(format!(
                "Invalid suffix format (missing stream_id): {}",
                suffix
            )));
        }
        (Some(parts[0].to_string()), parts[1].to_string())
    };

    // Deduplicate concurrent indexing of the same file + codec profile
    let dedup_key = if codecs.is_empty() {
        path_str.clone()
    } else {
        format!("{}|{}", path_str, codecs.join(","))
    };

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
                info!("Opening media: {:?} (stream_id: {:?})", media_path2, sid);
                let result = tokio::task::spawn_blocking(move || {
                    let codecs_refs: Vec<&str> = codecs_clone.iter().map(|s| s.as_str()).collect();
                    hls_vod_lib::MediaInfo::open(&media_path2, &codecs_refs, sid)
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

    // The entry is removed from `indexing_in_flight` either by the `OnceCell` init block on error,
    // or implicitly when the stream is moved to the main `streams` map.
    // No explicit `remove` here is needed as the `OnceCell` handles the "once" aspect,
    // and the `streams` map is the final destination.

    if sub_suffix == "master.m3u8" {
        // Build the correct relative prefix
        let filename = media_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let prefix = format!("{}/{}", filename, media.index.stream_id);
        return super::handlers::master_playlist(&state, &media, &prefix).await;
    } else if sub_suffix == "v/media.m3u8" {
        return super::handlers::video_playlist(&state, &media).await;
    } else if let Some(sub) = sub_suffix.strip_prefix("v/") {
        if let Some(_track_str) = sub.strip_suffix(".init.mp4") {
            return super::handlers::video_init_segment(&state, &media).await;
        } else if let Some(rest) = sub.strip_suffix(".m4s") {
            let parts: Vec<&str> = rest.split('.').collect();
            if parts.len() == 2 {
                let _track = parts[0] // Track is typically 0 for primary video
                    .parse::<usize>()
                    .map_err(|_| HttpError::SegmentNotFound("Invalid video track".into()))?;
                let seq = parts[1]
                    .parse::<usize>()
                    .map_err(|_| HttpError::SegmentNotFound("Invalid video seq".into()))?;
                return super::handlers::video_segment(&state, &media, seq).await;
            }
        }
    } else if let Some(sub) = sub_suffix.strip_prefix("a/") {
        if let Some(mut track_str) = sub.strip_suffix(".m3u8") {
            let mut force_aac = false;
            if let Some(base) = track_str.strip_suffix("-aac") {
                track_str = base;
                force_aac = true;
            }
            let track = track_str
                .parse::<usize>()
                .map_err(|_| HttpError::SegmentNotFound("Invalid audio track".into()))?;
            return super::handlers::audio_playlist(&state, &media, track, force_aac).await;
        } else if let Some(mut track_str) = sub.strip_suffix(".init.mp4") {
            let mut force_aac = false;
            if let Some(base) = track_str.strip_suffix("-aac") {
                track_str = base;
                force_aac = true;
            }
            let track = track_str
                .parse::<usize>()
                .map_err(|_| HttpError::SegmentNotFound("Invalid audio track".into()))?;
            return super::handlers::audio_init_segment(&state, &media, track, force_aac).await;
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
                return super::handlers::audio_segment(&state, &media, track, seq, force_aac).await;
            }
        }
    } else if let Some(sub) = sub_suffix.strip_prefix("s/") {
        if let Some(track_str) = sub.strip_suffix(".m3u8") {
            let track = track_str
                .parse::<usize>()
                .map_err(|_| HttpError::SegmentNotFound("Invalid subtitle track".into()))?;
            return super::handlers::subtitle_playlist(&state, &media, track).await;
        } else if let Some(rest) = sub.strip_suffix(".vtt") {
            let parts: Vec<&str> = rest.split('.').collect();
            if parts.len() == 2 {
                let track = parts[0]
                    .parse::<usize>()
                    .map_err(|_| HttpError::SegmentNotFound("Invalid subtitle track".into()))?;

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

                return super::handlers::subtitle_segment(
                    &state, &media, track, start_seq, end_seq,
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
