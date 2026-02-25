use crate::error::{HlsError, Result};
use ffmpeg_next as ffmpeg;
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::MutexGuard;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// A transparent wrapper to access an FFmpeg Input context.
/// It can either hold a freshly opened context (Owned) or a locked reference to a cached one (Shared).
pub enum ContextGuard<'a> {
    Owned(ffmpeg::format::context::Input),
    Shared(MutexGuard<'a, ffmpeg::format::context::Input>),
}

impl<'a> Deref for ContextGuard<'a> {
    type Target = ffmpeg::format::context::Input;

    fn deref(&self) -> &Self::Target {
        match self {
            ContextGuard::Owned(input) => input,
            ContextGuard::Shared(guard) => &**guard,
        }
    }
}

impl<'a> DerefMut for ContextGuard<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            ContextGuard::Owned(input) => input,
            ContextGuard::Shared(guard) => &mut **guard,
        }
    }
}

/// Video stream information
#[derive(Debug, Clone)]
pub struct VideoStreamInfo {
    /// Zero-based index of this stream in the source file
    pub stream_index: usize,
    /// FFmpeg codec identifier (e.g. `Id::H264`)
    pub codec_id: ffmpeg::codec::Id,
    /// Width of the video in pixels
    pub width: u32,
    /// Height of the video in pixels
    pub height: u32,
    /// Video bitrate in bits per second
    pub bitrate: u64,
    /// Video framerate as a rational number (e.g. 24000/1001)
    pub framerate: ffmpeg::Rational,
    /// Language code if specified
    pub language: Option<String>,
    /// Video encoder profile if detected
    pub profile: Option<i32>,
    /// Video encoder level if detected
    pub level: Option<i32>,
}

/// Audio stream information
#[derive(Debug, Clone)]
pub struct AudioStreamInfo {
    /// Zero-based index of this stream in the source file
    pub stream_index: usize,
    /// FFmpeg codec identifier for the audio stream
    pub codec_id: ffmpeg::codec::Id,
    /// Sampling rate in Hz (e.g., 48000)
    pub sample_rate: u32,
    /// Number of audio channels (e.g., 2 for stereo, 6 for 5.1 surround)
    pub channels: u16,
    /// Estimated or exact audio bitrate in bits per second
    pub bitrate: u64,
    /// Language code as specified in the source file metadata
    pub language: Option<String>,
    /// Boolean flag indicating if this stream needs to be transcoded to AAC
    pub is_transcoded: bool,
    /// If transcoded, the index of the original source stream
    pub source_stream_index: Option<usize>,
    /// Encoder delay in stream-native timebase samples (e.g. 1024 @ 48kHz for AAC).
    pub encoder_delay: i64,
}

/// A reference to a single subtitle sample in the source file.
/// Used to precisely extract subtitles without scanning from the beginning.
#[derive(Debug, Clone)]
pub struct SubtitleSampleRef {
    /// Byte offset within the source file where this subtitle sample begins
    pub byte_offset: u64,
    /// Presentation timestamp of the subtitle, in stream timebase units
    pub pts: i64,
    /// Duration of the subtitle display, in stream timebase units
    pub duration: i64,
    /// Raw byte size of the subtitle sample payload
    pub size: i32,
}

/// Subtitle stream information
#[derive(Debug, Clone)]
pub struct SubtitleStreamInfo {
    /// Zero-based index of this stream in the source file
    pub stream_index: usize,
    /// FFmpeg codec identifier (e.g., `Id::SUBRIP`)
    pub codec_id: ffmpeg::codec::Id,
    /// Subtitle language code if specified
    pub language: Option<String>,
    /// Normalized format categorization of the subtitle
    pub format: SubtitleFormat,
    /// A list of segment sequence numbers that contain at least one subtitle event (used to avoid serving empty segment files)
    pub non_empty_sequences: Vec<usize>,
    /// Pre-indexed index of every subtitle sample in the stream
    pub sample_index: Vec<SubtitleSampleRef>,
    /// Subtitle stream timebase
    pub timebase: ffmpeg::Rational,
    /// Start time offset measured in timebase units
    pub start_time: i64,
}

/// Subtitle format enumeration.
/// Represents the supported types of textual and bitmap subtitle streams.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubtitleFormat {
    /// SRT format texts
    SubRip,
    /// Advanced SubStation Alpha (SSA/ASS)
    Ass,
    /// QuickTime / MP4 generic text tracks
    MovText,
    /// WebVTT formatted subtitles
    WebVtt,
    /// Generic text subtitles
    Text,
    /// Unrecognized or unsupported subtitle format
    Unknown,
}

/// Segment information.
/// Represents a single time-bounded slice of the original file, used to generate an HLS segment.
#[derive(Debug, Clone)]
pub struct SegmentInfo {
    /// The consecutive segment sequence number starting from 0
    pub sequence: usize,
    /// Start presentation timestamp of the segment (in the video timeline's timebase)
    pub start_pts: i64,
    /// End presentation timestamp of the segment
    pub end_pts: i64,
    /// Length of the segment in seconds
    pub duration_secs: f64,
    /// Whether the segment begins with a keyframe
    pub is_keyframe: bool,
    /// Approximate byte offset in the file corresponding to the video start point
    pub video_byte_offset: u64,
}

/// Stream index - metadata about a media file.
/// This struct holds all the pre-calculated timings, tracks, and segment boundaries
/// necessary to reliably serve HLS playlists and fragments on demand.
pub struct StreamIndex {
    /// A unique identifier for the stream instance
    pub stream_id: String,
    /// Absolute path to the source media file
    pub source_path: PathBuf,
    /// Total duration of the media in seconds
    pub duration_secs: f64,
    /// The canonical video reference timebase used across all segments
    pub video_timebase: ffmpeg::Rational,
    /// List of video streams present in the media
    pub video_streams: Vec<VideoStreamInfo>,
    /// List of audio streams present in the media
    pub audio_streams: Vec<AudioStreamInfo>,
    /// List of subtitle streams present in the media
    pub subtitle_streams: Vec<SubtitleStreamInfo>,
    /// Pre-calculated timeline boundaries breaking the content into HLS segments
    pub segments: Vec<SegmentInfo>,
    /// Instant when the index was created
    pub indexed_at: SystemTime,
    /// Last access timestamp mapped to Unix EPOCH for cache eviction checking
    pub last_accessed: AtomicU64,
    /// Cache of the exact first PTS for each segment sequence, to perfectly align varying track timelines over time
    pub segment_first_pts: Arc<Vec<AtomicI64>>,
    /// Protected cache of the opened FFmpeg format context to avoid reopening the file repeatedly
    pub cached_context: Option<Arc<std::sync::Mutex<ffmpeg::format::context::Input>>>,
}

impl std::fmt::Debug for StreamIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamIndex")
            .field("stream_id", &self.stream_id)
            .field("source_path", &self.source_path)
            .field("duration_secs", &self.duration_secs)
            .field("video_timebase", &self.video_timebase)
            .field("video_streams", &self.video_streams)
            .field("audio_streams", &self.audio_streams)
            .field("subtitle_streams", &self.subtitle_streams)
            .field("segments", &self.segments)
            .field("indexed_at", &self.indexed_at)
            .field("last_accessed", &self.last_accessed)
            .field("segment_first_pts", &self.segment_first_pts)
            .field(
                "cached_context",
                &if self.cached_context.is_some() {
                    "Some(...)"
                } else {
                    "None"
                },
            )
            .finish()
    }
}

impl Clone for StreamIndex {
    fn clone(&self) -> Self {
        Self {
            stream_id: self.stream_id.clone(),
            source_path: self.source_path.clone(),
            duration_secs: self.duration_secs,
            video_timebase: self.video_timebase,
            video_streams: self.video_streams.clone(),
            audio_streams: self.audio_streams.clone(),
            subtitle_streams: self.subtitle_streams.clone(),
            segments: self.segments.clone(),
            indexed_at: self.indexed_at,
            last_accessed: AtomicU64::new(self.last_accessed.load(Ordering::Relaxed)),
            segment_first_pts: Arc::clone(&self.segment_first_pts),
            cached_context: self.cached_context.clone(),
        }
    }
}

impl StreamIndex {
    pub fn new(source_path: PathBuf) -> Self {
        Self {
            stream_id: Uuid::new_v4().to_string(),
            source_path,
            duration_secs: 0.0,
            video_timebase: ffmpeg::Rational::new(1, 1),
            video_streams: Vec::new(),
            audio_streams: Vec::new(),
            subtitle_streams: Vec::new(),
            segments: Vec::new(),
            indexed_at: SystemTime::now(),
            last_accessed: AtomicU64::new(0),
            segment_first_pts: Arc::new(Vec::new()),
            cached_context: None,
        }
    }

    pub fn init_segment_first_pts(&mut self) {
        let n = self.segments.len();
        let v: Vec<AtomicI64> = (0..n).map(|_| AtomicI64::new(i64::MIN)).collect();
        self.segment_first_pts = Arc::new(v);
    }

    pub fn set_segment_first_pts(&self, seq: usize, pts_90k: i64) {
        if let Some(slot) = self.segment_first_pts.get(seq) {
            slot.store(pts_90k, Ordering::Relaxed);
        }
    }

    pub fn get_segment_first_pts(&self, seq: usize) -> Option<i64> {
        self.segment_first_pts.get(seq).and_then(|slot| {
            let v = slot.load(Ordering::Relaxed);
            if v == i64::MIN {
                None
            } else {
                Some(v)
            }
        })
    }

    /// Retrieve a context to read the file.
    /// Returns either the locked cached context, or freshly opens the file if none is cached.
    pub fn get_context(&self) -> Result<ContextGuard<'_>> {
        if let Some(arc_mutex) = &self.cached_context {
            let guard = arc_mutex.lock().map_err(|_| {
                HlsError::Ffmpeg(crate::error::FfmpegError::OpenInput(
                    "Poisoned mutex lock on cached input context".to_string(),
                ))
            })?;
            Ok(ContextGuard::Shared(guard))
        } else {
            let input = ffmpeg::format::input(&self.source_path).map_err(|e| {
                HlsError::Ffmpeg(crate::error::FfmpegError::OpenInput(e.to_string()))
            })?;
            Ok(ContextGuard::Owned(input))
        }
    }

    pub fn primary_video(&self) -> Option<&VideoStreamInfo> {
        self.video_streams.first()
    }

    pub fn audio_by_language(&self, language: &str) -> Vec<&AudioStreamInfo> {
        self.audio_streams
            .iter()
            .filter(|a| {
                a.language
                    .as_ref()
                    .map(|l| l.to_lowercase() == language.to_lowercase())
                    .unwrap_or(false)
            })
            .collect()
    }

    pub fn subtitle_by_language(&self, language: &str) -> Vec<&SubtitleStreamInfo> {
        self.subtitle_streams
            .iter()
            .filter(|s| {
                s.language
                    .as_ref()
                    .map(|l| l.to_lowercase() == language.to_lowercase())
                    .unwrap_or(false)
            })
            .collect()
    }

    pub fn is_vod(&self) -> bool {
        true
    }

    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    pub fn touch(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.last_accessed.store(now, Ordering::Relaxed);
    }

    pub fn time_since_last_access(&self) -> u64 {
        let last = self.last_accessed.load(Ordering::Relaxed);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if now > last {
            now - last
        } else {
            0
        }
    }
}
