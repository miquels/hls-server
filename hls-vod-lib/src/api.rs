use crate::error::Result;
use crate::index::scanner;
use crate::playlist::{generate_master_playlist, variant};
use crate::segment::generator;
use crate::types::StreamIndex;
use ffmpeg_next as ffmpeg;
use std::path::Path;

pub struct MediaInfo {
    pub file_size: u64,
    pub duration_secs: f64,
    pub video_timebase: ffmpeg::Rational,
    pub tracks: Vec<TrackInfo>,
    /// The underlying stream index
    pub index: StreamIndex,
}

pub struct TrackInfo {
    pub id: String,
    pub track_type: TrackType,
    pub codec_id: String,
    pub language: Option<String>,
    pub bitrate: Option<u64>,
    pub transcode_to: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TrackType {
    Video { width: u32, height: u32 },
    Audio { channels: u16, sample_rate: u32 },
    Subtitle { format: String },
}

/// Parse a media file and return its metadata
pub fn parse_file(path: &Path, _cache: bool) -> Result<MediaInfo> {
    let index = scanner::scan_file(path)?;

    let mut tracks = Vec::new();

    // Video tracks
    for v in &index.video_streams {
        tracks.push(TrackInfo {
            id: format!("v/{}", v.stream_index),
            track_type: TrackType::Video {
                width: v.width,
                height: v.height,
            },
            codec_id: format!("{:?}", v.codec_id),
            language: v.language.clone(),
            bitrate: Some(v.bitrate),
            transcode_to: None,
        });
    }

    // Audio tracks
    for a in &index.audio_streams {
        tracks.push(TrackInfo {
            id: format!("a/{}", a.stream_index),
            track_type: TrackType::Audio {
                channels: a.channels,
                sample_rate: a.sample_rate,
            },
            codec_id: format!("{:?}", a.codec_id),
            language: a.language.clone(),
            bitrate: Some(a.bitrate),
            transcode_to: None,
        });
    }

    // Subtitle tracks
    for s in &index.subtitle_streams {
        tracks.push(TrackInfo {
            id: format!("s/{}", s.stream_index),
            track_type: TrackType::Subtitle {
                format: format!("{:?}", s.format),
            },
            codec_id: format!("{:?}", s.codec_id),
            language: s.language.clone(),
            bitrate: None,
            transcode_to: None,
        });
    }

    let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

    Ok(MediaInfo {
        file_size,
        duration_secs: index.duration_secs,
        video_timebase: index.video_timebase,
        tracks,
        index,
    })
}

/// Generate the master playlist (m3u8)
pub fn generate_main_playlist(media: &MediaInfo, prefix: &str) -> Result<String> {
    Ok(generate_master_playlist(&media.index, prefix))
}

/// Generate a variant track playlist
pub fn generate_track_playlist(media: &MediaInfo, playlist_id: &str) -> Result<String> {
    if playlist_id == "v/media.m3u8" {
        return Ok(variant::generate_video_playlist(&media.index));
    }

    if let Some(caps) = regex::Regex::new(r"a/(\d+)(?:-aac)?\.m3u8")
        .unwrap()
        .captures(playlist_id)
    {
        let id_idx: usize = caps[1]
            .parse()
            .map_err(|_| crate::error::HlsError::Muxing("Invalid audio ID".to_string()))?;
        let force_aac = playlist_id.contains("-aac");
        return Ok(variant::generate_audio_playlist(
            &media.index,
            id_idx,
            force_aac,
        ));
    }

    if let Some(caps) = regex::Regex::new(r"s/(\d+)\.m3u8")
        .unwrap()
        .captures(playlist_id)
    {
        let id_idx: usize = caps[1]
            .parse()
            .map_err(|_| crate::error::HlsError::Muxing("Invalid subtitle ID".to_string()))?;
        return Ok(variant::generate_subtitle_playlist(&media.index, id_idx));
    }

    Err(crate::error::HlsError::Muxing(format!(
        "Invalid playlist ID: {}",
        playlist_id
    )))
}

/// Generate a media segment
pub fn generate_segment(media: &MediaInfo, segment_id: &str) -> Result<Vec<u8>> {
    // Handle init segments
    if segment_id == "v/init.mp4" {
        return generator::generate_video_init_segment(&media.index).map(|b| b.to_vec());
    }

    if let Some(caps) = regex::Regex::new(r"a/(\d+)(?:-aac)?/init\.mp4")
        .unwrap()
        .captures(segment_id)
    {
        let id_idx: usize = caps[1]
            .parse()
            .map_err(|_| crate::error::HlsError::Muxing("Invalid audio ID".to_string()))?;
        let force_aac = segment_id.contains("-aac");
        return generator::generate_audio_init_segment(&media.index, id_idx, force_aac)
            .map(|b| b.to_vec());
    }

    // Handle media segments (.m4s, .vtt)
    if let Some(caps) = regex::Regex::new(r"v/(\d+)\.m4s")
        .unwrap()
        .captures(segment_id)
    {
        let seq: usize = caps[1]
            .parse()
            .map_err(|_| crate::error::HlsError::Muxing("Invalid sequence".to_string()))?;

        let track_idx = media
            .index
            .video_streams
            .first()
            .map(|v| v.stream_index)
            .unwrap_or(0);
        return generator::generate_video_segment(
            &media.index,
            track_idx,
            seq,
            &media.index.source_path,
        )
        .map(|b| b.to_vec());
    }

    if let Some(caps) = regex::Regex::new(r"a/(\d+)(?:-aac)?/(\d+)\.m4s")
        .unwrap()
        .captures(segment_id)
    {
        let id_idx: usize = caps[1]
            .parse()
            .map_err(|_| crate::error::HlsError::Muxing("Invalid audio ID".to_string()))?;
        let seq: usize = caps[2]
            .parse()
            .map_err(|_| crate::error::HlsError::Muxing("Invalid sequence".to_string()))?;
        let force_aac = segment_id.contains("-aac");
        return generator::generate_audio_segment(
            &media.index,
            id_idx,
            seq,
            &media.index.source_path,
            force_aac,
        )
        .map(|b| b.to_vec());
    }

    if let Some(caps) = regex::Regex::new(r"s/(\d+)/(\d+)-(\d+)\.vtt")
        .unwrap()
        .captures(segment_id)
    {
        let id_idx: usize = caps[1]
            .parse()
            .map_err(|_| crate::error::HlsError::Muxing("Invalid subtitle ID".to_string()))?;
        let start_seq: usize = caps[2]
            .parse()
            .map_err(|_| crate::error::HlsError::Muxing("Invalid start seq".to_string()))?;
        let end_seq: usize = caps[3]
            .parse()
            .map_err(|_| crate::error::HlsError::Muxing("Invalid end seq".to_string()))?;
        return generator::generate_subtitle_segment(
            &media.index,
            id_idx,
            start_seq,
            end_seq,
            &media.index.source_path,
        )
        .map(|b| b.to_vec());
    }

    Err(crate::error::HlsError::Muxing(format!(
        "Invalid segment ID: {}",
        segment_id
    )))
}
