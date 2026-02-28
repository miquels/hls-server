//! HTTP proxy utilities for forwarding requests to Jellyfin.

use crate::config::Config;
use crate::error::{ProxyError, Result};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use http::StatusCode;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

/// Proxy service for forwarding requests to Jellyfin.
#[derive(Clone)]
pub struct ProxyService {
    inner: Arc<ProxyServiceInner>,
}

struct ProxyServiceInner {
    base_url: String,
    http_client: Client<hyper_util::client::legacy::connect::HttpConnector, Full<Bytes>>,
    timeout: Duration,
}

impl Clone for ProxyServiceInner {
    fn clone(&self) -> Self {
        Self {
            base_url: self.base_url.clone(),
            http_client: self.http_client.clone(),
            timeout: self.timeout,
        }
    }
}

impl ProxyService {
    /// Create a new proxy service.
    pub fn new(config: &Config) -> Result<Self> {
        let mut connector = hyper_util::client::legacy::connect::HttpConnector::new();
        
        // Configure connection settings
        connector.set_keepalive(Some(Duration::from_secs(60)));
        connector.enforce_http(!config.jellyfin_url.starts_with("https://"));
        
        let http_client = Client::builder(TokioExecutor::new())
            .pool_idle_timeout(Duration::from_secs(90))
            .pool_max_idle_per_host(30)
            .retry_canceled_requests(true)
            .build(connector);

        Ok(Self {
            inner: Arc::new(ProxyServiceInner {
                base_url: config.jellyfin_url.clone(),
                http_client,
                timeout: Duration::from_secs(300), // 5 minute timeout for media requests
            }),
        })
    }

    /// Forward a request to Jellyfin.
    pub async fn forward(
        &self,
        req: http::Request<Full<Bytes>>,
    ) -> Result<http::Response<Full<Bytes>>> {
        // Extract the path and query
        let path_and_query = req
            .uri()
            .path_and_query()
            .map(|pq| pq.as_str())
            .unwrap_or("/");

        // Build the new URI
        let new_uri = format!("{}{}", self.inner.base_url, path_and_query);

        // Rebuild the request with the new URI
        let (mut parts, body) = req.into_parts();
        parts.uri = new_uri.parse().map_err(|e| {
            ProxyError::Config(format!("Failed to parse URI: {}", e))
        })?;

        // Build the new request
        let new_request = http::Request::from_parts(parts, body);

        // Send the request with timeout
        let response = tokio::time::timeout(
            self.inner.timeout,
            self.inner.http_client.request(new_request),
        )
        .await
        .map_err(|_| ProxyError::Config("Request timed out".to_string()))?
        .map_err(|e| ProxyError::Config(format!("HTTP client error: {}", e)))?;

        // Convert Incoming body to Full<Bytes>
        let (parts, body) = response.into_parts();
        let body_bytes = body
            .collect()
            .await
            .map(|b| b.to_bytes())
            .map_err(|e| ProxyError::Config(format!("Body collection error: {}", e)))?;

        Ok(http::Response::from_parts(parts, Full::new(body_bytes)))
    }

    /// Get the base URL.
    pub fn base_url(&self) -> &str {
        &self.inner.base_url
    }

    /// Set a custom timeout for requests.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        Arc::make_mut(&mut self.inner).timeout = timeout;
        self
    }
}

/// Log a proxy request.
pub fn log_request(method: &http::Method, uri: &http::Uri) {
    info!(
        method = %method,
        uri = %uri,
        "proxy request"
    );
}

/// Log a proxy response.
pub fn log_response(status: StatusCode, latency: std::time::Duration) {
    info!(
        status = %status,
        latency_ms = latency.as_millis(),
        "proxy response"
    );
}
