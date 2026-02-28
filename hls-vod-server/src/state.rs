#![allow(dead_code)]

//! Application state management
//!
//! This module defines the AppState structure that holds:
//! - Active stream metadata (via hls-vod-lib::MediaInfo)
//! - Segment cache (LRU)
//! - Server configuration

use std::sync::atomic::AtomicBool;

use crate::config::ServerConfig;

/// Application state shared across all handlers
pub struct AppState {
    /// Server shutdown flag
    pub shutdown: AtomicBool,

    /// Server configuration
    pub config: ServerConfig,
}

impl AppState {
    pub fn new(config: ServerConfig) -> Self {
        hls_vod_lib::init_segment_cache(config.cache.clone());

        Self {
            shutdown: AtomicBool::new(false),
            config,
        }
    }

    /// Create AppState with default configuration
    pub fn with_defaults() -> Self {
        Self::new(ServerConfig::default())
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> hls_vod_lib::SegmentCacheStats {
        hls_vod_lib::segment_cache_stats()
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
        hls_vod_lib::cleanup_expired_streams()
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::with_defaults()
    }
}
