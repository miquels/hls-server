//! LRU Segment Cache
//!
//! Implements a least-recently-used cache for HLS segments
//! with memory limit enforcement.

use bytes::Bytes;
use dashmap::DashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

use crate::config::CacheConfig;

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

    pub fn is_expired(&self, ttl_secs: u64) -> bool {
        self.age_secs() > ttl_secs
    }
}

/// LRU cache for HLS segments
pub struct SegmentCache {
    /// Cache entries (key -> entry)
    entries: DashMap<String, CacheEntry>,
    /// Current memory usage in bytes
    memory_bytes: AtomicUsize,
    /// Cache configuration
    config: CacheConfig,
}

impl SegmentCache {
    /// Create a new segment cache
    pub fn new(config: CacheConfig) -> Self {
        Self {
            entries: DashMap::new(),
            memory_bytes: AtomicUsize::new(0),
            config,
        }
    }

    /// Generate cache key from components
    pub fn make_key(stream_id: &str, segment_type: &str, sequence: usize) -> String {
        format!("{}:{}:{}", stream_id, segment_type, sequence)
    }

    /// Get a cached segment
    pub fn get(&self, stream_id: &str, segment_type: &str, sequence: usize) -> Option<Bytes> {
        let key = Self::make_key(stream_id, segment_type, sequence);

        if let Some(mut entry) = self.entries.get_mut(&key) {
            entry.touch();
            Some(entry.data.clone())
        } else {
            None
        }
    }

    /// Check if a segment is cached
    pub fn contains(&self, stream_id: &str, segment_type: &str, sequence: usize) -> bool {
        let key = Self::make_key(stream_id, segment_type, sequence);
        self.entries.contains_key(&key)
    }

    /// Cache a segment
    pub fn insert(&self, stream_id: &str, segment_type: &str, sequence: usize, data: Bytes) {
        let key = Self::make_key(stream_id, segment_type, sequence);
        let size = data.len();

        // Check memory limit before inserting
        let current = self.memory_bytes.load(Ordering::Relaxed);
        if current + size > self.config.max_memory_bytes() {
            // Evict entries to make room
            self.evict_if_needed(size);
        }

        // Check segment count limit
        if self.entries.len() >= self.config.max_segments {
            self.evict_if_needed(size);
        }

        self.entries.insert(key, CacheEntry::new(data));
        self.memory_bytes.fetch_add(size, Ordering::Relaxed);
    }

    /// Evict entries if needed to make room for new data
    fn evict_if_needed(&self, needed_size: usize) {
        let mut freed = 0;
        let target = self.config.max_memory_bytes() / 2;

        // First, remove expired entries
        self.entries.retain(|_, entry| {
            if entry.is_expired(self.config.ttl_secs) {
                freed += entry.data.len();
                false
            } else {
                true
            }
        });
        self.memory_bytes.fetch_sub(freed, Ordering::Relaxed);

        // If still need space, remove by LRU
        if self.memory_bytes.load(Ordering::Relaxed) + needed_size > self.config.max_memory_bytes()
        {
            // Collect entries sorted by last_accessed
            let mut entries: Vec<_> = self.entries.iter().collect();
            entries.sort_by_key(|e| e.value().last_accessed);

            let mut to_remove = Vec::new();
            freed = 0;

            for entry in entries {
                if freed >= target {
                    break;
                }
                to_remove.push(entry.key().clone());
                freed += entry.value().data.len();
            }

            for key in to_remove {
                if let Some((_, entry)) = self.entries.remove(&key) {
                    self.memory_bytes
                        .fetch_sub(entry.data.len(), Ordering::Relaxed);
                }
            }
        }
    }

    /// Remove all cache entries for a stream
    pub fn remove_stream(&self, stream_id: &str) {
        let mut freed = 0;
        self.entries.retain(|key, entry| {
            if key.starts_with(stream_id) {
                freed += entry.data.len();
                false
            } else {
                true
            }
        });
        self.memory_bytes.fetch_sub(freed, Ordering::Relaxed);
    }

    /// Clear all expired entries
    pub fn clear_expired(&self) {
        let mut freed = 0;
        self.entries.retain(|_, entry| {
            if entry.is_expired(self.config.ttl_secs) {
                freed += entry.data.len();
                false
            } else {
                true
            }
        });
        self.memory_bytes.fetch_sub(freed, Ordering::Relaxed);
    }

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        let mut count = 0;
        let mut total_size = 0;
        let mut oldest_age = 0;

        for entry in self.entries.iter() {
            count += 1;
            total_size += entry.value().data.len();
            let age = entry.value().age_secs();
            if age > oldest_age {
                oldest_age = age;
            }
        }

        CacheStats {
            entry_count: count,
            total_size_bytes: total_size,
            memory_limit_bytes: self.config.max_memory_bytes(),
            oldest_entry_age_secs: oldest_age,
        }
    }

    /// Get the number of cached entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if cache is empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get current memory usage in bytes
    pub fn memory_usage(&self) -> usize {
        self.memory_bytes.load(Ordering::Relaxed)
    }
}

/// Cache statistics
#[derive(Debug)]
pub struct CacheStats {
    pub entry_count: usize,
    pub total_size_bytes: usize,
    pub memory_limit_bytes: usize,
    pub oldest_entry_age_secs: u64,
}

impl Default for SegmentCache {
    fn default() -> Self {
        Self::new(CacheConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_cache_entry_creation() {
        let data = Bytes::from("test data");
        let entry = CacheEntry::new(data.clone());

        assert_eq!(entry.data, data);
        assert_eq!(entry.access_count, 1);
        assert!(entry.age_secs() < 2);
    }

    #[test]
    fn test_cache_entry_touch() {
        let data = Bytes::from("test");
        let mut entry = CacheEntry::new(data);

        std::thread::sleep(Duration::from_millis(10));
        entry.touch();

        assert_eq!(entry.access_count, 2);
    }

    #[test]
    fn test_cache_insert_get() {
        let cache = SegmentCache::new(CacheConfig::default());
        let data = Bytes::from("segment data");

        cache.insert("stream1", "video", 0, data.clone());

        assert!(cache.contains("stream1", "video", 0));
        assert_eq!(cache.get("stream1", "video", 0), Some(data));
    }

    #[test]
    fn test_cache_miss() {
        let cache = SegmentCache::new(CacheConfig::default());

        assert!(!cache.contains("stream1", "video", 0));
        assert_eq!(cache.get("stream1", "video", 0), None);
    }

    #[test]
    fn test_cache_remove_stream() {
        let cache = SegmentCache::new(CacheConfig::default());

        cache.insert("stream1", "video", 0, Bytes::from("v0"));
        cache.insert("stream1", "video", 1, Bytes::from("v1"));
        cache.insert("stream1", "audio", 0, Bytes::from("a0"));
        cache.insert("stream2", "video", 0, Bytes::from("v0"));

        cache.remove_stream("stream1");

        assert!(!cache.contains("stream1", "video", 0));
        assert!(!cache.contains("stream1", "video", 1));
        assert!(!cache.contains("stream1", "audio", 0));
        assert!(cache.contains("stream2", "video", 0));
    }

    #[test]
    fn test_cache_stats() {
        let cache = SegmentCache::new(CacheConfig::default());

        cache.insert("stream1", "video", 0, Bytes::from("data"));

        let stats = cache.stats();
        assert_eq!(stats.entry_count, 1);
        assert!(stats.total_size_bytes > 0);
    }

    #[test]
    fn test_cache_make_key() {
        let key = SegmentCache::make_key("abc123", "video", 5);
        assert_eq!(key, "abc123:video:5");
    }

    #[test]
    fn test_cache_len_and_empty() {
        let cache = SegmentCache::new(CacheConfig::default());
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);

        cache.insert("s1", "v", 0, Bytes::from("x"));
        assert!(!cache.is_empty());
        assert_eq!(cache.len(), 1);
    }
}
