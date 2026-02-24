//! Prometheus-compatible metrics endpoint

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Metrics collector
#[derive(Debug)]
pub struct Metrics {
    /// Server start time
    start_time: Instant,
    /// Total requests processed
    request_count: RwLock<u64>,
    /// Requests by endpoint
    requests_by_endpoint: RwLock<std::collections::HashMap<String, u64>>,
    /// Total bytes served
    bytes_served: RwLock<u64>,
    /// Cache hits
    cache_hits: RwLock<u64>,
    /// Cache misses
    cache_misses: RwLock<u64>,
    /// Active streams
    active_streams: RwLock<u64>,
    /// Transcoding operations
    transcode_operations: RwLock<u64>,
    /// Errors by type
    errors_by_type: RwLock<std::collections::HashMap<String, u64>>,
}

impl Metrics {
    /// Create new metrics collector
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            request_count: RwLock::new(0),
            requests_by_endpoint: RwLock::new(std::collections::HashMap::new()),
            bytes_served: RwLock::new(0),
            cache_hits: RwLock::new(0),
            cache_misses: RwLock::new(0),
            active_streams: RwLock::new(0),
            transcode_operations: RwLock::new(0),
            errors_by_type: RwLock::new(std::collections::HashMap::new()),
        }
    }

    /// Record a request
    pub fn record_request(&self, endpoint: &str) {
        *self.request_count.write() += 1;
        *self
            .requests_by_endpoint
            .write()
            .entry(endpoint.to_string())
            .or_insert(0) += 1;
    }

    /// Record bytes served
    pub fn record_bytes(&self, bytes: u64) {
        *self.bytes_served.write() += bytes;
    }

    /// Record cache hit
    pub fn record_cache_hit(&self) {
        *self.cache_hits.write() += 1;
    }

    /// Record cache miss
    pub fn record_cache_miss(&self) {
        *self.cache_misses.write() += 1;
    }

    /// Update active stream count
    pub fn set_active_streams(&self, count: u64) {
        *self.active_streams.write() = count;
    }

    /// Record transcoding operation
    pub fn record_transcode(&self) {
        *self.transcode_operations.write() += 1;
    }

    /// Record error
    pub fn record_error(&self, error_type: &str) {
        *self
            .errors_by_type
            .write()
            .entry(error_type.to_string())
            .or_insert(0) += 1;
    }

    /// Get uptime in seconds
    pub fn uptime_secs(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }

    /// Export metrics in Prometheus format
    pub fn export_prometheus(&self) -> String {
        let mut output = String::new();

        // Server info
        output.push_str("# HELP hls_server_uptime_seconds Server uptime in seconds\n");
        output.push_str("# TYPE hls_server_uptime_seconds counter\n");
        output.push_str(&format!(
            "hls_server_uptime_seconds {}\n",
            self.uptime_secs()
        ));

        output.push_str(
            "\n# HELP hls_server_start_time_seconds Server start time as Unix timestamp\n",
        );
        output.push_str("# TYPE hls_server_start_time_seconds gauge\n");
        output.push_str(&format!(
            "hls_server_start_time_seconds {}\n",
            std::time::SystemTime::UNIX_EPOCH
                .elapsed()
                .unwrap_or(Duration::ZERO)
                .as_secs()
                - self.uptime_secs()
        ));

        // Request metrics
        output.push_str("\n# HELP hls_requests_total Total number of HTTP requests\n");
        output.push_str("# TYPE hls_requests_total counter\n");
        output.push_str(&format!(
            "hls_requests_total {}\n",
            *self.request_count.read()
        ));

        output.push_str("\n# HELP hls_requests_by_endpoint Requests by endpoint\n");
        output.push_str("# TYPE hls_requests_by_endpoint counter\n");
        for (endpoint, count) in self.requests_by_endpoint.read().iter() {
            output.push_str(&format!(
                "hls_requests_by_endpoint{{endpoint=\"{}\"}} {}\n",
                endpoint, count
            ));
        }

        // Bytes served
        output.push_str("\n# HELP hls_bytes_served_total Total bytes served\n");
        output.push_str("# TYPE hls_bytes_served_total counter\n");
        output.push_str(&format!(
            "hls_bytes_served_total {}\n",
            *self.bytes_served.read()
        ));

        // Cache metrics
        output.push_str("\n# HELP hls_cache_hits_total Total cache hits\n");
        output.push_str("# TYPE hls_cache_hits_total counter\n");
        output.push_str(&format!(
            "hls_cache_hits_total {}\n",
            *self.cache_hits.read()
        ));

        output.push_str("\n# HELP hls_cache_misses_total Total cache misses\n");
        output.push_str("# TYPE hls_cache_misses_total counter\n");
        output.push_str(&format!(
            "hls_cache_misses_total {}\n",
            *self.cache_misses.read()
        ));

        let hits = *self.cache_hits.read();
        let misses = *self.cache_misses.read();
        let hit_ratio = if hits + misses > 0 {
            hits as f64 / (hits + misses) as f64
        } else {
            0.0
        };
        output.push_str("\n# HELP hls_cache_hit_ratio Cache hit ratio\n");
        output.push_str("# TYPE hls_cache_hit_ratio gauge\n");
        output.push_str(&format!("hls_cache_hit_ratio {:.4}\n", hit_ratio));

        // Stream metrics
        output.push_str("\n# HELP hls_active_streams Number of active streams\n");
        output.push_str("# TYPE hls_active_streams gauge\n");
        output.push_str(&format!(
            "hls_active_streams {}\n",
            *self.active_streams.read()
        ));

        // Transcoding metrics
        output.push_str("\n# HELP hls_transcode_operations_total Total transcoding operations\n");
        output.push_str("# TYPE hls_transcode_operations_total counter\n");
        output.push_str(&format!(
            "hls_transcode_operations_total {}\n",
            *self.transcode_operations.read()
        ));

        // Error metrics
        output.push_str("\n# HELP hls_errors_total Total errors by type\n");
        output.push_str("# TYPE hls_errors_total counter\n");
        for (error_type, count) in self.errors_by_type.read().iter() {
            output.push_str(&format!(
                "hls_errors_total{{type=\"{}\"}} {}\n",
                error_type, count
            ));
        }

        output
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Metrics endpoint handler
pub async fn metrics_handler(State(metrics): State<Arc<Metrics>>) -> Response {
    let prometheus_output = metrics.export_prometheus();

    (
        StatusCode::OK,
        [("Content-Type", "text/plain; version=0.0.4")],
        prometheus_output,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_creation() {
        let metrics = Metrics::new();
        assert!(metrics.uptime_secs() < 2);
    }

    #[test]
    fn test_record_request() {
        let metrics = Metrics::new();
        metrics.record_request("/test");
        metrics.record_request("/test");

        assert_eq!(*metrics.request_count.read(), 2);
        assert_eq!(metrics.requests_by_endpoint.read().get("/test"), Some(&2));
    }

    #[test]
    fn test_cache_metrics() {
        let metrics = Metrics::new();
        metrics.record_cache_hit();
        metrics.record_cache_hit();
        metrics.record_cache_miss();

        assert_eq!(*metrics.cache_hits.read(), 2);
        assert_eq!(*metrics.cache_misses.read(), 1);
    }

    #[test]
    fn test_export_prometheus() {
        let metrics = Metrics::new();
        metrics.record_request("/test");
        metrics.record_cache_hit();

        let output = metrics.export_prometheus();

        assert!(output.contains("hls_requests_total"));
        assert!(output.contains("hls_cache_hits_total"));
        assert!(output.contains("hls_server_uptime_seconds"));
    }

    #[test]
    fn test_error_recording() {
        let metrics = Metrics::new();
        metrics.record_error("stream_not_found");
        metrics.record_error("stream_not_found");
        metrics.record_error("segment_not_found");

        let errors = metrics.errors_by_type.read();
        assert_eq!(errors.get("stream_not_found"), Some(&2));
        assert_eq!(errors.get("segment_not_found"), Some(&1));
    }
}
