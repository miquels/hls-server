use ffmpeg_next as ffmpeg;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// Video stream information
#[derive(Debug, Clone)]
pub struct VideoStreamInfo {
    pub stream_index: usize,
    pub codec_id: ffmpeg::codec::Id,
    pub width: u32,
    pub height: u32,
    pub bitrate: u64,
    pub framerate: ffmpeg::Rational,
    pub language: Option<String>,
    pub profile: Option<i32>,
    pub level: Option<i32>,
}

/// Audio stream information
#[derive(Debug, Clone)]
pub struct AudioStreamInfo {
    pub stream_index: usize,
    pub codec_id: ffmpeg::codec::Id,
    pub sample_rate: u32,
    pub channels: u16,
    pub bitrate: u64,
    pub language: Option<String>,
    pub is_transcoded: bool,
    pub source_stream_index: Option<usize>,
    /// Encoder delay in stream-native timebase samples (e.g. 1024 @ 48kHz for AAC).
    pub encoder_delay: i64,
}

/// A reference to a single subtitle sample in the source file.
#[derive(Debug, Clone)]
pub struct SubtitleSampleRef {
    pub byte_offset: u64,
    pub pts: i64,
    pub duration: i64,
    pub size: i32,
}

/// Subtitle stream information
#[derive(Debug, Clone)]
pub struct SubtitleStreamInfo {
    pub stream_index: usize,
    pub codec_id: ffmpeg::codec::Id,
    pub language: Option<String>,
    pub format: SubtitleFormat,
    pub non_empty_sequences: Vec<usize>,
    pub sample_index: Vec<SubtitleSampleRef>,
    pub timebase: ffmpeg::Rational,
    pub start_time: i64,
}

/// Subtitle format enumeration
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubtitleFormat {
    SubRip,
    Ass,
    MovText,
    WebVtt,
    Text,
    Unknown,
}

/// Segment information
#[derive(Debug, Clone)]
pub struct SegmentInfo {
    pub sequence: usize,
    pub start_pts: i64,
    pub end_pts: i64,
    pub duration_secs: f64,
    pub is_keyframe: bool,
    pub video_byte_offset: u64,
}

/// Stream index - metadata about a media file
#[derive(Debug)]
pub struct StreamIndex {
    pub stream_id: String,
    pub source_path: PathBuf,
    pub duration_secs: f64,
    pub video_timebase: ffmpeg::Rational,
    pub video_streams: Vec<VideoStreamInfo>,
    pub audio_streams: Vec<AudioStreamInfo>,
    pub subtitle_streams: Vec<SubtitleStreamInfo>,
    pub segments: Vec<SegmentInfo>,
    pub indexed_at: SystemTime,
    pub last_accessed: AtomicU64,
    pub segment_first_pts: Arc<Vec<AtomicI64>>,
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
            last_accessed: AtomicU64::new(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            ),
            segment_first_pts: Arc::new(Vec::new()),
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
