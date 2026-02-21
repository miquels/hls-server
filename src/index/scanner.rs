//! File scanner - extracts metadata and calculates segment boundaries

use std::path::Path;
use std::time::{Duration, SystemTime};

use ffmpeg_next as ffmpeg;

use crate::error::{FfmpegError, HlsError, Result};
use crate::state::{SegmentInfo, StreamIndex};

use super::{analyze_audio_stream, analyze_subtitle_stream, analyze_video_stream};

/// Indexing options
#[derive(Debug, Clone)]
pub struct IndexOptions {
    /// Target segment duration in seconds
    pub segment_duration_secs: f64,
    /// Timeout for indexing (for unindexed MKV files)
    pub timeout: Option<Duration>,
}

impl Default for IndexOptions {
    fn default() -> Self {
        Self {
            segment_duration_secs: 4.0,
            timeout: Some(Duration::from_secs(30)),
        }
    }
}

/// Scan a media file and extract all metadata
pub fn scan_file<P: AsRef<Path>>(path: P) -> Result<StreamIndex> {
    scan_file_with_options(path, &IndexOptions::default())
}

/// Scan a media file with timeout handling
pub fn scan_file_with_timeout<P: AsRef<Path>>(path: P, timeout: Duration) -> Result<StreamIndex> {
    let options = IndexOptions {
        timeout: Some(timeout),
        ..Default::default()
    };
    scan_file_with_options(path, &options)
}

/// Scan a media file with custom options
pub fn scan_file_with_options<P: AsRef<Path>>(
    path: P,
    options: &IndexOptions,
) -> Result<StreamIndex> {
    let path = path.as_ref().to_path_buf();

    // Initialize FFmpeg if not already done
    ffmpeg::init().map_err(|e| FfmpegError::InitFailed(format!("ffmpeg::init() failed: {}", e)))?;

    // Open the input file
    let mut context = ffmpeg::format::input(&path)
        .map_err(|e| FfmpegError::OpenInput(format!("Failed to open {:?}: {}", path, e)))?;

    let mut index = StreamIndex::new(path.clone());
    index.duration_secs = context.duration() as f64 / ffmpeg::ffi::AV_TIME_BASE as f64;

    // Analyze each stream
    for (i, stream) in context.streams().into_iter().enumerate() {
        let medium = stream.parameters().medium();

        match medium {
            ffmpeg::media::Type::Video => match analyze_video_stream(&stream, i) {
                Ok(info) => {
                    tracing::debug!(
                        "Found video stream: {}x{}, codec={:?}",
                        info.width,
                        info.height,
                        info.codec_id
                    );
                    index.video_streams.push(info);
                }
                Err(e) => {
                    tracing::warn!("Failed to analyze video stream {}: {}", i, e);
                }
            },
            ffmpeg::media::Type::Audio => match analyze_audio_stream(&stream, i) {
                Ok(info) => {
                    tracing::debug!(
                        "Found audio stream: {}Hz, {} channels, codec={:?}",
                        info.sample_rate,
                        info.channels,
                        info.codec_id
                    );
                    index.audio_streams.push(info);
                }
                Err(e) => {
                    tracing::warn!("Failed to analyze audio stream {}: {}", i, e);
                }
            },
            ffmpeg::media::Type::Subtitle => {
                if let Some(info) = analyze_subtitle_stream(&stream, i) {
                    tracing::debug!(
                        "Found subtitle stream: language={:?}, format={:?}",
                        info.language,
                        info.format
                    );
                    index.subtitle_streams.push(info);
                }
            }
            _ => {
                tracing::debug!("Skipping stream {} (type={:?})", i, medium);
            }
        }
    }

    // Validate we have at least a video stream
    if index.video_streams.is_empty() {
        return Err(HlsError::NoVideoStream);
    }

    // Calculate segment boundaries from keyframes
    let (segments, non_empty_subtitle_sequences) =
        calculate_segments(&mut context, &index, options.segment_duration_secs)?;
    index.segments = segments;

    for sub in &mut index.subtitle_streams {
        if let Some(sequences) = non_empty_subtitle_sequences.get(&sub.stream_index) {
            sub.non_empty_sequences = sequences.clone();
        }
    }

    // Record the video stream's timebase so downstream consumers (e.g. subtitle
    // generator) can correctly rescale segment.start_pts / end_pts.
    let video_stream_idx = index.video_streams.first().map(|v| v.stream_index);
    if let Some(idx) = video_stream_idx {
        if let Some(stream) = context.streams().into_iter().nth(idx) {
            index.video_timebase = stream.time_base();
        }
    }

    index.indexed_at = SystemTime::now();

    tracing::info!(
        "Indexed file: {:?}, duration={:.2}s, video={}, audio={}, subtitles={}, segments={}",
        path,
        index.duration_secs,
        index.video_streams.len(),
        index.audio_streams.len(),
        index.subtitle_streams.len(),
        index.segments.len()
    );

    Ok(index)
}

/// Calculate segment boundaries based on keyframe positions
fn calculate_segments(
    context: &mut ffmpeg::format::context::Input,
    index: &StreamIndex,
    target_duration_secs: f64,
) -> Result<(
    Vec<SegmentInfo>,
    std::collections::HashMap<usize, Vec<usize>>,
)> {
    let mut segments = Vec::new();

    // Get the video stream index
    let video_stream_idx = index
        .video_streams
        .first()
        .ok_or(HlsError::NoVideoStream)?
        .stream_index;

    // Get timebase and start_time from the video stream
    let video_stream = context
        .streams()
        .into_iter()
        .nth(video_stream_idx)
        .ok_or(FfmpegError::ReadFrame("Video stream not found".to_string()))?;
    let timebase = video_stream.time_base();
    let mut video_start_time = video_stream.start_time();
    if video_start_time == std::i64::MIN {
        // AV_NOPTS_VALUE in rust is i64::MIN
        video_start_time = 0;
    }

    // Prepare to collect subtitle PTS values
    // We store: (timebase, start_time, Vec<pts>)
    let mut subtitle_streams = std::collections::HashMap::new();
    for sub in &index.subtitle_streams {
        let stream = context.streams().into_iter().nth(sub.stream_index);
        let tb = stream
            .as_ref()
            .map(|s| s.time_base())
            .unwrap_or(ffmpeg::Rational::new(1, 1000));
        let mut start_time = stream
            .as_ref()
            .map(|s| s.start_time())
            .unwrap_or(std::i64::MIN);
        if start_time == std::i64::MIN {
            start_time = 0;
        }
        subtitle_streams.insert(sub.stream_index, (tb, start_time, Vec::new()));
    }

    let mut current_segment_start_pts: Option<i64> = None;
    let mut segment_sequence = 0;

    // Read packets to find keyframes
    for (_stream, packet) in context.packets() {
        let packet_stream = packet.stream() as usize;

        if packet_stream == video_stream_idx {
            let pts = packet.pts().unwrap_or(0);

            if packet.is_key() {
                // Calculate duration since last keyframe
                if let Some(start_pts) = current_segment_start_pts {
                    let duration = pts_to_seconds(pts - start_pts, timebase);

                    // Check if we've reached target segment duration
                    if duration >= target_duration_secs * 0.8 {
                        // Close current segment
                        segments.push(SegmentInfo {
                            sequence: segment_sequence,
                            start_pts: start_pts,
                            end_pts: pts,
                            duration_secs: duration,
                            is_keyframe: true,
                            video_byte_offset: 0,
                        });
                        segment_sequence += 1;

                        // Start new segment
                        current_segment_start_pts = Some(pts);
                    }
                } else {
                    // First keyframe
                    current_segment_start_pts = Some(pts);
                }
            }
        } else if let Some((_tb, _sub_start, pts_list)) = subtitle_streams.get_mut(&packet_stream) {
            let pts = packet.pts().unwrap_or(0);
            pts_list.push(pts);
        }
    }

    // Close the final segment
    if let Some(start_pts) = current_segment_start_pts {
        // Use total duration to calculate end_pts for the last segment
        let total_pts = seconds_to_pts(index.duration_secs, timebase);
        let end_pts = total_pts.max(start_pts);
        let duration = pts_to_seconds(end_pts - start_pts, timebase);

        segments.push(SegmentInfo {
            sequence: segment_sequence,
            start_pts,
            end_pts,
            duration_secs: duration.max(0.1),
            is_keyframe: true,
            video_byte_offset: 0,
        });
    }

    // If no segments were found, create a single segment for the entire file
    if segments.is_empty() && index.duration_secs > 0.0 {
        segments.push(SegmentInfo {
            sequence: 0,
            start_pts: 0,
            end_pts: seconds_to_pts(index.duration_secs, timebase),
            duration_secs: index.duration_secs,
            is_keyframe: true,
            video_byte_offset: 0,
        });
    }

    // Map subtitle PTS values to video segment sequences
    let mut non_empty_segments_by_stream = std::collections::HashMap::new();
    for (stream_idx, (sub_tb, sub_start, pts_list)) in subtitle_streams {
        let mut non_empty = std::collections::HashSet::new();
        for pts in pts_list {
            // Calculate absolute playtime in subtitle timebase
            let sub_playtime = pts.saturating_sub(sub_start);
            // Convert to video timebase
            let rescaled_playtime =
                crate::ffmpeg::utils::rescale_ts(sub_playtime, sub_tb, timebase);

            // Find which segment contains this PLAYTIME relative to video start
            if let Some(seg) = segments.iter().find(|s| {
                let seg_start_playtime = s.start_pts.saturating_sub(video_start_time);
                let seg_end_playtime = s.end_pts.saturating_sub(video_start_time);
                rescaled_playtime >= seg_start_playtime && rescaled_playtime < seg_end_playtime
            }) {
                non_empty.insert(seg.sequence);
            } else if let Some(last) = segments.last() {
                // If it's past the last segment, assign it to the last segment
                let last_end_playtime = last.end_pts.saturating_sub(video_start_time);
                if rescaled_playtime >= last_end_playtime {
                    non_empty.insert(last.sequence);
                } else if let Some(first) = segments.first() {
                    // If it's before the first segment, assign it to the first segment
                    let first_start_playtime = first.start_pts.saturating_sub(video_start_time);
                    if rescaled_playtime < first_start_playtime {
                        non_empty.insert(first.sequence);
                    }
                }
            }
        }
        let mut sorted: Vec<usize> = non_empty.into_iter().collect();
        sorted.sort_unstable();
        non_empty_segments_by_stream.insert(stream_idx, sorted);
    }

    tracing::debug!("Calculated {} segments", segments.len());
    Ok((segments, non_empty_segments_by_stream))
}

/// Convert PTS to seconds using timebase
fn pts_to_seconds(pts: i64, timebase: ffmpeg::Rational) -> f64 {
    let num = timebase.numerator() as f64;
    let den = timebase.denominator() as f64;
    (pts as f64 * num) / den
}

/// Convert seconds to PTS using timebase
fn seconds_to_pts(secs: f64, timebase: ffmpeg::Rational) -> i64 {
    let num = timebase.numerator() as f64;
    let den = timebase.denominator() as f64;
    ((secs * den as f64) / num as f64) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_index_options_default() {
        let options = IndexOptions::default();
        assert_eq!(options.segment_duration_secs, 4.0);
        assert!(options.timeout.is_some());
    }

    #[test]
    fn test_pts_conversion() {
        let timebase = ffmpeg::Rational::new(1, 90000);

        // 1 second = 90000 ticks
        assert!((pts_to_seconds(90000, timebase) - 1.0).abs() < 0.0001);
        assert!((pts_to_seconds(45000, timebase) - 0.5).abs() < 0.0001);

        // Round trip
        let pts = seconds_to_pts(2.5, timebase);
        assert!((pts_to_seconds(pts, timebase) - 2.5).abs() < 0.0001);
    }
}
