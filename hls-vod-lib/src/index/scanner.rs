//! File scanner - extracts metadata and calculates segment boundaries
//!
//! Segment boundaries and subtitle sample references are derived entirely from
//! the demuxer's in-memory index tables (built from `moov` for MP4, `Cues` for
//! MKV, etc.) without reading any media data.  Files that do not have a
//! complete index are rejected with `HlsError::NoIndex`.

use std::path::Path;
use std::time::SystemTime;

use ffmpeg_next as ffmpeg;

use crate::error::{FfmpegError, HlsError, Result};
use crate::ffmpeg_utils::index::read_index_entries;
use crate::types::{SegmentInfo, StreamIndex, SubtitleSampleRef};

use super::{analyze_audio_stream, analyze_subtitle_stream, analyze_video_stream};

/// Indexing options
#[derive(Debug, Clone)]
pub struct IndexOptions {
    /// Target segment duration in seconds
    pub segment_duration_secs: f64,
}

impl Default for IndexOptions {
    fn default() -> Self {
        Self {
            segment_duration_secs: 4.0,
        }
    }
}

/// Scan a media file and extract all metadata
pub fn scan_file<P: AsRef<Path>>(path: P) -> Result<StreamIndex> {
    scan_file_with_options(path, &IndexOptions::default())
}

/// Scan a media file with custom options.
///
/// Opens the file (which causes the demuxer to parse the container header and
/// populate its internal index tables), then builds all metadata purely from
/// those in-memory tables — no media data is read.
pub fn scan_file_with_options<P: AsRef<Path>>(
    path: P,
    options: &IndexOptions,
) -> Result<StreamIndex> {
    let path = path.as_ref().to_path_buf();

    // Opening the file parses moov/cues and populates the demuxer index.
    // No media data is read at this point.
    let mut context = ffmpeg::format::input(&path)
        .map_err(|e| FfmpegError::OpenInput(format!("Failed to open {:?}: {}", path, e)))?;

    let mut index = StreamIndex::new(path.clone());
    index.duration_secs = context.duration() as f64 / ffmpeg::ffi::AV_TIME_BASE as f64;

    // Analyze each stream (reads only codec parameters from the container header)
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
                Err(e) => tracing::warn!("Failed to analyze video stream {}: {}", i, e),
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
                Err(e) => tracing::warn!("Failed to analyze audio stream {}: {}", i, e),
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
            _ => tracing::debug!("Skipping stream {} (type={:?})", i, medium),
        }
    }

    if index.video_streams.is_empty() {
        return Err(HlsError::NoVideoStream);
    }

    // --- Build everything from the demuxer index tables ---

    let video_stream_idx = index.video_streams.first().unwrap().stream_index;
    let video_stream = context
        .streams()
        .into_iter()
        .nth(video_stream_idx)
        .ok_or_else(|| FfmpegError::ReadFrame("Video stream not found".to_string()))?;

    let video_tb = video_stream.time_base();
    let mut video_start_time = video_stream.start_time();
    if video_start_time == std::i64::MIN {
        video_start_time = 0;
    }
    index.video_timebase = video_tb;

    tracing::debug!(
        "Video stream {}: timebase={}/{}, start_time={}, start_time_sec={:.6}",
        video_stream_idx,
        video_tb.numerator(),
        video_tb.denominator(),
        video_start_time,
        video_start_time as f64 * video_tb.numerator() as f64 / video_tb.denominator() as f64
    );

    // Read the video stream's index entries (keyframe positions from moov/cues)
    let video_entries = read_index_entries(&video_stream);
    // Drop video_stream borrow so we can call context.packets() mutably below
    drop(video_stream);
    if video_entries.is_empty() {
        return Err(HlsError::NoIndex(format!(
            "File {:?} has no demuxer index for the video stream. \
             Only files with a complete container index (MP4 moov, MKV Cues) are supported.",
            path
        )));
    }

    if let Some(first) = video_entries.first() {
        tracing::debug!(
            "First video index entry: pts={}, pos={}, is_keyframe={}",
            first.timestamp,
            first.pos,
            first.is_keyframe()
        );
    }

    // Determine encoder_delay for each audio stream by reading its first packet.
    // FFmpeg signals encoder delay as a negative first-packet DTS — universal
    // across all containers (MP4, MKV, …) and codecs (AAC, Opus, Vorbis, …).
    // The init segment's edit list tells the player:
    //   presentation = (tfdt - encoder_delay) / timescale
    // so we must set: tfdt = video_presentation * timescale + encoder_delay
    {
        use std::collections::HashMap;
        let audio_indices: std::collections::HashSet<usize> =
            index.audio_streams.iter().map(|a| a.stream_index).collect();
        let mut delays: HashMap<usize, i64> = HashMap::new();

        for (stream, packet) in context.packets() {
            let idx = stream.index();
            if !audio_indices.contains(&idx) || delays.contains_key(&idx) {
                continue;
            }
            let dts = packet.dts().unwrap_or(0);
            let delay = if dts < 0 { -dts } else { 0 };
            delays.insert(idx, delay);
            tracing::debug!(
                "Audio stream {}: first_pkt_dts={}, encoder_delay={}",
                idx,
                dts,
                delay
            );
            if delays.len() == audio_indices.len() {
                break;
            }
        }

        for audio in &mut index.audio_streams {
            audio.encoder_delay = *delays.get(&audio.stream_index).unwrap_or(&0);
        }
    }

    // Build segment boundaries from keyframe entries
    let segments = build_segments_from_entries(
        &video_entries,
        video_tb,
        video_start_time,
        index.duration_secs,
        options.segment_duration_secs,
    );

    if let Some(seg0) = segments.first() {
        tracing::debug!(
            "Segment 0: start_pts={}, end_pts={}, start_sec={:.6}",
            seg0.start_pts,
            seg0.end_pts,
            seg0.start_pts as f64 * video_tb.numerator() as f64 / video_tb.denominator() as f64
        );
    }

    // Build subtitle sample_index and non_empty_sequences from subtitle index entries
    for sub in &mut index.subtitle_streams {
        let sub_stream = match context.streams().into_iter().nth(sub.stream_index) {
            Some(s) => s,
            None => continue,
        };
        let sub_entries = read_index_entries(&sub_stream);

        // Store per-sample byte offsets for direct seeking at request time
        sub.sample_index = sub_entries
            .iter()
            .map(|e| SubtitleSampleRef {
                byte_offset: e.pos,
                pts: e.timestamp,
                duration: 0, // duration is not in the index; read from packet at request time
                size: e.size,
            })
            .collect();

        // Derive non_empty_sequences by mapping each subtitle PTS to a segment
        let non_empty = map_pts_to_segments(
            sub_entries.iter().map(|e| e.timestamp),
            sub.timebase,
            sub.start_time,
            video_tb,
            video_start_time,
            &segments,
        );
        sub.non_empty_sequences = non_empty;

        tracing::debug!(
            "Subtitle stream {}: {} index entries, {} non-empty segments",
            sub.stream_index,
            sub.sample_index.len(),
            sub.non_empty_sequences.len()
        );
    }

    index.segments = segments;
    index.init_segment_first_pts();
    index.indexed_at = SystemTime::now();

    tracing::info!(
        "Indexed {:?}: duration={:.2}s, video={}, audio={}, subtitles={}, segments={}",
        path,
        index.duration_secs,
        index.video_streams.len(),
        index.audio_streams.len(),
        index.subtitle_streams.len(),
        index.segments.len()
    );

    Ok(index)
}

/// Build `SegmentInfo` list from video keyframe index entries.
///
/// Walks the keyframe entries and closes a segment whenever the accumulated
/// duration reaches `target_duration_secs * 0.8` (same threshold as before).
/// Each `SegmentInfo` now carries the correct `video_byte_offset`.
fn build_segments_from_entries(
    entries: &[crate::ffmpeg_utils::index::IndexEntry],
    timebase: ffmpeg::Rational,
    _video_start_time: i64,
    total_duration_secs: f64,
    target_duration_secs: f64,
) -> Vec<SegmentInfo> {
    let mut segments = Vec::new();
    let mut segment_sequence: usize = 0;
    let mut seg_start_pts: Option<i64> = None;
    let mut seg_start_byte: u64 = 0;

    for entry in entries {
        if !entry.is_keyframe() {
            continue;
        }
        let pts = entry.timestamp;

        if let Some(start_pts) = seg_start_pts {
            let duration = pts_to_seconds(pts - start_pts, timebase);
            if duration >= target_duration_secs * 0.8 {
                segments.push(SegmentInfo {
                    sequence: segment_sequence,
                    start_pts,
                    end_pts: pts,
                    duration_secs: duration,
                    is_keyframe: true,
                    video_byte_offset: seg_start_byte,
                });
                segment_sequence += 1;
                seg_start_pts = Some(pts);
                seg_start_byte = entry.pos;
            }
        } else {
            // First keyframe — start of first segment.
            // Clamp to 0: some files have a negative first keyframe PTS due to
            // B-frame pre-roll (e.g. pts=-1335 @ 1/16000). If we keep it negative,
            // EXTINF(seg=0) = (start_pts(seg=1) - neg) / tb is inflated by |neg|,
            // making the playlist timeline ahead of the segment tfdt values by that
            // same amount — causing a seek double-jump.
            seg_start_pts = Some(pts.max(0));
            seg_start_byte = entry.pos;
        }
    }

    // Close the final segment
    if let Some(start_pts) = seg_start_pts {
        let total_pts = seconds_to_pts(total_duration_secs, timebase);
        let end_pts = total_pts.max(start_pts);
        let duration = pts_to_seconds(end_pts - start_pts, timebase).max(0.1);
        segments.push(SegmentInfo {
            sequence: segment_sequence,
            start_pts,
            end_pts,
            duration_secs: duration,
            is_keyframe: true,
            video_byte_offset: seg_start_byte,
        });
    }

    segments
}

/// Map an iterator of subtitle PTS values to the video segment sequences that
/// contain them.  Returns a sorted, deduplicated `Vec<usize>`.
fn map_pts_to_segments(
    pts_iter: impl Iterator<Item = i64>,
    sub_tb: ffmpeg::Rational,
    sub_start_time: i64,
    video_tb: ffmpeg::Rational,
    video_start_time: i64,
    segments: &[SegmentInfo],
) -> Vec<usize> {
    let mut non_empty = std::collections::HashSet::new();

    for pts in pts_iter {
        let sub_playtime = pts.saturating_sub(sub_start_time);
        let rescaled = crate::ffmpeg_utils::utils::rescale_ts(sub_playtime, sub_tb, video_tb);

        if let Some(seg) = segments.iter().find(|s| {
            let seg_start = s.start_pts.saturating_sub(video_start_time);
            let seg_end = s.end_pts.saturating_sub(video_start_time);
            rescaled >= seg_start && rescaled < seg_end
        }) {
            non_empty.insert(seg.sequence);
        } else if let Some(last) = segments.last() {
            let last_end = last.end_pts.saturating_sub(video_start_time);
            if rescaled >= last_end {
                non_empty.insert(last.sequence);
            } else if let Some(first) = segments.first() {
                if rescaled < first.start_pts.saturating_sub(video_start_time) {
                    non_empty.insert(first.sequence);
                }
            }
        }
    }

    let mut sorted: Vec<usize> = non_empty.into_iter().collect();
    sorted.sort_unstable();
    sorted
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
    ((secs * den) / num) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_index_options_default() {
        let options = IndexOptions::default();
        assert_eq!(options.segment_duration_secs, 4.0);
    }

    #[test]
    fn test_pts_conversion() {
        let timebase = ffmpeg::Rational::new(1, 90000);

        assert!((pts_to_seconds(90000, timebase) - 1.0).abs() < 0.0001);
        assert!((pts_to_seconds(45000, timebase) - 0.5).abs() < 0.0001);

        let pts = seconds_to_pts(2.5, timebase);
        assert!((pts_to_seconds(pts, timebase) - 2.5).abs() < 0.0001);
    }
}
