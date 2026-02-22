#![allow(dead_code)]

//! Application state management
//!
//! This module defines the AppState structure that holds:
//! - Active stream metadata (via hls-vod-lib::MediaInfo)
//! - Segment cache (LRU)
//! - Server configuration

use crate::config::ServerConfig;
use bytes::Bytes;
use dashmap::DashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use hls_vod_lib::MediaInfo;

/// Application state shared across all handlers
pub struct AppState {
    /// Active streams (stream_id -> MediaInfo)
    pub streams: DashMap<String, Arc<MediaInfo>>,

    /// Path-to-stream lookup for deduplication
    pub path_to_stream: DashMap<String, String>,

    /// In-flight indexing: path -> shared cell that resolves to the indexed media info.
    pub indexing_in_flight: DashMap<String, Arc<tokio::sync::OnceCell<Arc<MediaInfo>>>>,

    /// In-flight segment generation: cache_key -> shared cell that resolves to the segment bytes.
    pub segments_in_flight: DashMap<String, Arc<tokio::sync::OnceCell<bytes::Bytes>>>,

    /// Segment cache (stream_id:segment_type:sequence -> CacheEntry)
    pub segment_cache: crate::http::cache::SegmentCache,

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
            indexing_in_flight: DashMap::new(),
            segments_in_flight: DashMap::new(),
            segment_cache: crate::http::cache::SegmentCache::new(config.cache.clone()),
            shutdown: AtomicBool::new(false),
            config,
        }
    }

    /// Create AppState with default configuration
    pub fn with_defaults() -> Self {
        Self::new(ServerConfig::default())
    }

    /// Register a new stream
    pub fn register_stream(&self, media: MediaInfo) -> Arc<MediaInfo> {
        let stream_id = media.index.stream_id.clone();
        let source_path = media.index.source_path.to_string_lossy().to_string();
        let arc = Arc::new(media);

        self.streams.insert(stream_id.clone(), arc.clone());
        self.path_to_stream.insert(source_path, stream_id);

        arc
    }

    /// Get a stream by ID
    pub fn get_stream(&self, stream_id: &str) -> Option<Arc<MediaInfo>> {
        self.streams.get(stream_id).map(|r| r.clone())
    }

    /// Get a stream by source path
    pub fn get_stream_by_path(&self, path: &str) -> Option<Arc<MediaInfo>> {
        self.path_to_stream
            .get(path)
            .map(|r| r.clone())
            .and_then(|id| self.streams.get(&id).map(|r| r.clone()))
    }

    /// Remove a stream
    pub fn remove_stream(&self, stream_id: &str) -> Option<Arc<MediaInfo>> {
        if let Some((_, arc)) = self.streams.remove(stream_id) {
            let path = arc.index.source_path.to_string_lossy();
            self.path_to_stream.remove(path.as_ref());
            Some(arc)
        } else {
            None
        }
    }

    /// Get a cached segment or generate it exactly once even under concurrent requests.
    pub async fn get_or_generate_segment<F, Fut>(
        &self,
        stream_id: &str,
        segment_type: &str,
        sequence: usize,
        generate: F,
    ) -> hls_vod_lib::Result<Bytes>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = hls_vod_lib::Result<Bytes>> + Send + 'static,
    {
        let cache_key =
            crate::http::cache::SegmentCache::make_key(stream_id, segment_type, sequence);

        // Fast path: already cached
        if let Some(data) = self.segment_cache.get(stream_id, segment_type, sequence) {
            return Ok(data);
        }

        // Slow path: get-or-create an in-flight cell for this key
        let cell = self
            .segments_in_flight
            .entry(cache_key.clone())
            .or_insert_with(|| std::sync::Arc::new(tokio::sync::OnceCell::new()))
            .clone();

        let data = cell
            .get_or_try_init(|| async move { generate().await })
            .await?
            .clone();

        // Populate the persistent cache and drop the in-flight entry
        self.segment_cache
            .insert(stream_id, segment_type, sequence, data.clone());
        self.segments_in_flight.remove(&cache_key);

        Ok(data)
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
    pub fn cleanup_expired_streams(&self) -> usize {
        const STREAM_TIMEOUT_SECS: u64 = 600; // 10 minutes

        let mut streams_to_remove = Vec::new();

        for entry in self.streams.iter() {
            let media = entry.value();
            if media.index.time_since_last_access() > STREAM_TIMEOUT_SECS {
                streams_to_remove.push(entry.key().clone());
            }
        }

        let mut count = 0;
        for stream_id in streams_to_remove {
            if self.remove_stream(&stream_id).is_some() {
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
