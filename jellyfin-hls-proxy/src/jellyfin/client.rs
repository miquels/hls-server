//! Jellyfin HTTP client for making API requests.

use crate::config::Config;
use crate::error::{ProxyError, Result};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use std::sync::Arc;

/// Jellyfin HTTP client.
#[derive(Clone)]
pub struct JellyfinClient {
    inner: Arc<JellyfinClientInner>,
}

struct JellyfinClientInner {
    base_url: String,
    http_client: Client<hyper_util::client::legacy::connect::HttpConnector, Full<Bytes>>,
}

impl JellyfinClient {
    /// Create a new Jellyfin client from configuration.
    pub fn new(config: &Config) -> Result<Self> {
        let http_client = Client::builder(TokioExecutor::new())
            .build(hyper_util::client::legacy::connect::HttpConnector::new());

        Ok(Self {
            inner: Arc::new(JellyfinClientInner {
                base_url: config.jellyfin_url.clone(),
                http_client,
            }),
        })
    }

    /// Build a URL for a Jellyfin API endpoint.
    fn build_url(&self, path: &str) -> String {
        format!("{}{}", self.inner.base_url, path)
    }

    /// Make an HTTP request to Jellyfin.
    pub async fn request(
        &self,
        method: hyper::Method,
        path: &str,
        body: Option<Bytes>,
    ) -> Result<http::Response<Full<Bytes>>> {
        let url = self.build_url(path);

        let mut req_builder = http::Request::builder()
            .method(&method)
            .uri(&url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json");

        let req_body = if let Some(body_bytes) = body {
            req_builder = req_builder.header("Content-Length", body_bytes.len());
            Full::new(body_bytes)
        } else {
            Full::new(Bytes::new())
        };

        let request = req_builder
            .body(req_body)
            .map_err(|e| ProxyError::Http(e))?;

        let response = self
            .inner
            .http_client
            .request(request)
            .await
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

    /// Get the base URL for constructing media URLs.
    pub fn base_url(&self) -> &str {
        &self.inner.base_url
    }
}
