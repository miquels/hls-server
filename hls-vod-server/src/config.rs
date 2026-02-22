//! Server configuration

use serde::{Deserialize, Serialize};

/// Cache configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Maximum memory usage for segment cache in megabytes
    pub max_memory_mb: usize,

    /// Maximum number of segments to cache
    pub max_segments: usize,

    /// Time-to-live for cached segments in seconds
    pub ttl_secs: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_memory_mb: 512,
            max_segments: 100, // ~400 seconds of content at 4s/segment
            ttl_secs: 300,     // 5 minutes
        }
    }
}

impl CacheConfig {
    /// Get maximum memory in bytes
    pub fn max_memory_bytes(&self) -> usize {
        self.max_memory_mb * 1024 * 1024
    }
}

/// Segment configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentConfig {
    /// Target segment duration in seconds
    pub target_duration_secs: f64,

    /// Minimum segment duration (tolerance)
    pub min_duration_secs: f64,

    /// Maximum segment duration (tolerance)
    pub max_duration_secs: f64,
}

impl Default for SegmentConfig {
    fn default() -> Self {
        Self {
            target_duration_secs: 4.0,
            min_duration_secs: 3.0,
            max_duration_secs: 6.0,
        }
    }
}

/// Audio transcoding configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    /// Target sample rate for AAC output
    pub target_sample_rate: u32,

    /// AAC bitrate in bps
    pub aac_bitrate: u64,

    /// Enable audio transcoding
    pub enable_transcoding: bool,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            target_sample_rate: 48000,
            aac_bitrate: 128000,
            enable_transcoding: true,
        }
    }
}

/// Server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Host address to bind to
    pub host: String,

    /// Port to listen on
    pub port: u16,

    /// Cache configuration
    pub cache: CacheConfig,

    /// Segment configuration
    pub segment: SegmentConfig,

    /// Audio configuration
    pub audio: AudioConfig,

    /// Enable CORS
    pub cors_enabled: bool,

    /// Log level (trace, debug, info, warn, error)
    pub log_level: String,

    /// Maximum concurrent streams
    pub max_concurrent_streams: Option<usize>,

    /// Rate limit requests per second
    pub rate_limit_rps: Option<u32>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 3000,
            cache: CacheConfig::default(),
            segment: SegmentConfig::default(),
            audio: AudioConfig::default(),
            cors_enabled: true,
            log_level: "info".to_string(),
            max_concurrent_streams: Some(100),
            rate_limit_rps: Some(100),
        }
    }
}

impl ServerConfig {
    /// Get the socket address string
    pub fn socket_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    /// Load configuration from a TOML file
    pub fn from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let content = std::fs::read_to_string(path)?;
        let config: ServerConfig = toml::from_str(&content)?;
        Ok(config)
    }

    /// Save configuration to a TOML file
    pub fn to_file(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ServerConfig::default();
        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.port, 3000);
        assert_eq!(config.cache.max_memory_mb, 512);
        assert_eq!(config.segment.target_duration_secs, 4.0);
    }

    #[test]
    fn test_cache_config_max_bytes() {
        let cache = CacheConfig {
            max_memory_mb: 256,
            ..Default::default()
        };
        assert_eq!(cache.max_memory_bytes(), 256 * 1024 * 1024);
    }

    #[test]
    fn test_socket_addr() {
        let config = ServerConfig {
            host: "127.0.0.1".to_string(),
            port: 8080,
            ..Default::default()
        };
        assert_eq!(config.socket_addr(), "127.0.0.1:8080");
    }
}
