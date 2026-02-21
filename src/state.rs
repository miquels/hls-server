#![allow(dead_code)]

//! Application state management
//!
//! This module defines the AppState structure that holds:
//! - Active stream indices
//! - Segment cache (LRU)
//! - Audio encoder pool
//! - Server configuration

use crate::config::ServerConfig;
use bytes::Bytes;
use dashmap::DashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use ffmpeg_next as ffmpeg;

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
}

/// A reference to a single subtitle sample in the source file.
/// Populated at index time from the demuxer's in-memory index (moov/cues).
/// Allows seeking directly to the sample without scanning the file.
#[derive(Debug, Clone)]
pub struct SubtitleSampleRef {
    /// Byte offset of this sample in the file.
    pub byte_offset: u64,
    /// Presentation timestamp in the subtitle stream's native timebase.
    pub pts: i64,
    /// Duration in the subtitle stream's native timebase (0 if unknown).
    pub duration: i64,
    /// Size of the sample payload in bytes.
    pub size: i32,
}

/// Subtitle stream information
#[derive(Debug, Clone)]
pub struct SubtitleStreamInfo {
    pub stream_index: usize,
    pub codec_id: ffmpeg::codec::Id,
    pub language: Option<String>,
    pub format: SubtitleFormat,
    /// Video segment sequences that contain at least one subtitle packet.
    /// Empty means not yet determined (treat all as non-empty).
    pub non_empty_sequences: Vec<usize>,
    /// Per-sample byte offsets extracted from the demuxer index at scan time.
    /// Sorted by pts. Used to seek directly to each subtitle sample on request.
    pub sample_index: Vec<SubtitleSampleRef>,
    /// Timebase of this subtitle stream (stored at scan time to avoid re-opening).
    pub timebase: ffmpeg::Rational,
    /// start_time of this subtitle stream (stored at scan time).
    pub start_time: i64,
}

/// Subtitle format enumeration
#[derive(Debug, Clone, PartialEq)]
pub enum SubtitleFormat {
    SubRip,  // SRT
    Ass,     // ASS/SSA
    MovText, // TTXT (QuickTime)
    WebVtt,  // WebVTT
    Text,    // Plain text
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
    /// Timebase of the video stream — this is the timebase in which
    /// `SegmentInfo::start_pts` / `end_pts` are expressed.
    pub video_timebase: ffmpeg::Rational,
    pub video_streams: Vec<VideoStreamInfo>,
    pub audio_streams: Vec<AudioStreamInfo>,
    pub subtitle_streams: Vec<SubtitleStreamInfo>,
    pub segments: Vec<SegmentInfo>,
    pub indexed_at: SystemTime,
    pub last_accessed: AtomicU64,
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
        }
    }
}

impl StreamIndex {
    /// Create a new stream index
    pub fn new(source_path: PathBuf) -> Self {
        Self {
            stream_id: Uuid::new_v4().to_string(),
            source_path,
            duration_secs: 0.0,
            video_timebase: ffmpeg::Rational::new(1, 1), // overwritten by scanner
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
        }
    }

    /// Get the primary video stream (first one if multiple)
    pub fn primary_video(&self) -> Option<&VideoStreamInfo> {
        self.video_streams.first()
    }

    /// Get audio streams for a specific language
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

    /// Get subtitle streams for a specific language
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

    /// Check if this is a VOD (video on demand) stream
    pub fn is_vod(&self) -> bool {
        true // For now, all streams are VOD
    }

    /// Get the number of segments
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// Update last accessed time to now
    pub fn touch(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.last_accessed.store(now, Ordering::Relaxed);
    }

    /// Get seconds since last access
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

/// Cache entry with metadata
#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub data: Bytes,
    pub created_at: SystemTime,
    pub last_accessed: SystemTime,
    pub access_count: usize,
}

impl CacheEntry {
    pub fn new(data: Bytes) -> Self {
        let now = SystemTime::now();
        Self {
            data,
            created_at: now,
            last_accessed: now,
            access_count: 1,
        }
    }

    pub fn touch(&mut self) {
        self.last_accessed = SystemTime::now();
        self.access_count += 1;
    }

    pub fn age_secs(&self) -> u64 {
        self.created_at.elapsed().map(|d| d.as_secs()).unwrap_or(0)
    }
}

/// Key for audio encoder pool
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioEncoderKey {
    pub source_codec: String, // Use String instead of codec::Id for Hash
    pub target_sample_rate: u32,
    pub channels: u16,
}

impl std::hash::Hash for AudioEncoderKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.source_codec.hash(state);
        self.target_sample_rate.hash(state);
        self.channels.hash(state);
    }
}

/// Application state shared across all handlers
pub struct AppState {
    /// Active streams (stream_id -> StreamIndex)
    pub streams: DashMap<String, Arc<StreamIndex>>,

    /// Path-to-stream lookup for deduplication
    pub path_to_stream: DashMap<String, String>,

    /// Segment cache (stream_id:segment_type:sequence -> CacheEntry)
    pub segment_cache: crate::http::cache::SegmentCache,

    /// Audio encoder pool (shared across streams)
    pub audio_encoders: dashmap::DashMap<AudioEncoderKey, ()>, // Placeholder for encoder pool

    /// Server shutdown flag
    pub shutdown: AtomicBool,

    /// Server configuration
    pub config: ServerConfig,
}

impl AppState {
    /// Create a new AppState with the given configuration
    pub fn new(config: ServerConfig) -> Self {
        Self {
            streams: DashMap::new(),
            path_to_stream: DashMap::new(),
            segment_cache: crate::http::cache::SegmentCache::new(config.cache.clone()),
            audio_encoders: dashmap::DashMap::new(),
            shutdown: AtomicBool::new(false),
            config,
        }
    }

    /// Create AppState with default configuration
    pub fn with_defaults() -> Self {
        Self::new(ServerConfig::default())
    }

    /// Register a new stream
    pub fn register_stream(&self, index: StreamIndex) -> Arc<StreamIndex> {
        let stream_id = index.stream_id.clone();
        let source_path = index.source_path.to_string_lossy().to_string();
        let arc = Arc::new(index);

        self.streams.insert(stream_id.clone(), arc.clone());
        self.path_to_stream.insert(source_path, stream_id);

        arc
    }

    /// Get a stream by ID
    pub fn get_stream(&self, stream_id: &str) -> Option<Arc<StreamIndex>> {
        self.streams.get(stream_id).map(|r| r.clone())
    }

    /// Get a stream by source path
    pub fn get_stream_by_path(&self, path: &str) -> Option<Arc<StreamIndex>> {
        self.path_to_stream
            .get(path)
            .map(|r| r.clone())
            .and_then(|id| self.streams.get(&id).map(|r| r.clone()))
    }

    /// Remove a stream
    pub fn remove_stream(&self, stream_id: &str) -> Option<Arc<StreamIndex>> {
        // Remove from path lookup
        if let Some(index) = self.streams.remove(stream_id) {
            let (_, arc) = index;
            if let Some(path) = self
                .path_to_stream
                .iter()
                .find(|r| r.value() == stream_id)
                .map(|r| r.key().clone())
            {
                self.path_to_stream.remove(&path);
            }
            Some(arc)
        } else {
            None
        }
    }

    /// Cache a segment
    pub fn cache_segment(&self, stream_id: &str, segment_type: &str, sequence: usize, data: Bytes) {
        self.segment_cache
            .insert(stream_id, segment_type, sequence, data);
    }

    /// Get a cached segment
    pub fn get_cached_segment(
        &self,
        stream_id: &str,
        segment_type: &str,
        sequence: usize,
    ) -> Option<Bytes> {
        self.segment_cache.get(stream_id, segment_type, sequence)
    }

    /// Check if a segment is cached
    pub fn is_segment_cached(&self, stream_id: &str, segment_type: &str, sequence: usize) -> bool {
        self.segment_cache
            .contains(stream_id, segment_type, sequence)
    }

    /// Clear all cache entries for a stream
    pub fn clear_stream_cache(&self, stream_id: &str) {
        self.segment_cache.remove_stream(stream_id);
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> CacheStats {
        let stats = self.segment_cache.stats();
        CacheStats {
            entry_count: stats.entry_count,
            total_size_bytes: stats.total_size_bytes,
            memory_limit_bytes: stats.memory_limit_bytes,
        }
    }

    /// Signal shutdown
    pub fn shutdown(&self) {
        self.shutdown
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Check if shutdown is requested
    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Remove expired streams
    /// Returns number of removed streams
    pub fn cleanup_expired_streams(&self) -> usize {
        // Default timeout: 300 seconds (5 minutes)
        // TODO: Make this configurable
        const STREAM_TIMEOUT_SECS: u64 = 300;

        let mut streams_to_remove = Vec::new();

        for entry in self.streams.iter() {
            let stream = entry.value();
            if stream.time_since_last_access() > STREAM_TIMEOUT_SECS {
                streams_to_remove.push(entry.key().clone());
            }
        }

        let mut count = 0;
        for stream_id in streams_to_remove {
            if self.remove_stream(&stream_id).is_some() {
                // Also clean up segment cache for this stream
                self.segment_cache.remove_stream(&stream_id);
                count += 1;
            }
        }

        count
    }
}

/// Cache statistics
#[derive(Debug)]
pub struct CacheStats {
    pub entry_count: usize,
    pub total_size_bytes: usize,
    pub memory_limit_bytes: usize,
}

impl Default for AppState {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_index_creation() {
        let index = StreamIndex::new(PathBuf::from("/test/video.mp4"));
        assert!(!index.stream_id.is_empty());
        assert_eq!(index.source_path, PathBuf::from("/test/video.mp4"));
        assert!(index.video_streams.is_empty());
    }

    #[test]
    fn test_cache_entry() {
        let data = Bytes::from("test data");
        let entry = CacheEntry::new(data.clone());
        assert_eq!(entry.data, data);
        assert_eq!(entry.access_count, 1);
    }

    #[test]
    fn test_app_state_creation() {
        let state = AppState::with_defaults();
        assert_eq!(state.streams.len(), 0);
        assert_eq!(state.segment_cache.len(), 0);
        assert!(!state.is_shutdown());
    }

    #[test]
    fn test_app_state_register_stream() {
        let state = AppState::with_defaults();
        let index = StreamIndex::new(PathBuf::from("/test/video.mp4"));
        let stream_id = index.stream_id.clone();

        state.register_stream(index);

        assert!(state.get_stream(&stream_id).is_some());
        assert!(state.get_stream_by_path("/test/video.mp4").is_some());
    }

    #[test]
    fn test_app_state_cache_segment() {
        let state = AppState::with_defaults();
        let index = StreamIndex::new(PathBuf::from("/test/video.mp4"));
        let stream_id = index.stream_id.clone();
        state.register_stream(index);

        let data = Bytes::from("segment data");
        state.cache_segment(&stream_id, "video", 0, data.clone());

        assert!(state.is_segment_cached(&stream_id, "video", 0));
        assert_eq!(state.get_cached_segment(&stream_id, "video", 0), Some(data));
    }

    #[test]
    fn test_cache_stats() {
        let state = AppState::with_defaults();
        let stats = state.cache_stats();
        assert_eq!(stats.entry_count, 0);
        assert_eq!(stats.total_size_bytes, 0);
    }
}
