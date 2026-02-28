//! Configuration structures for the proxy server.

use clap::Parser;
use std::net::SocketAddr;
use std::path::PathBuf;

/// Jellyfin HLS transmuxing proxy configuration.
#[derive(Parser, Debug, Clone)]
#[command(name = "jellyfin-hls-proxy")]
#[command(author, version, about, long_about = None)]
pub struct Config {
    /// Address to bind the proxy server to.
    #[arg(short = 'b', long, default_value = "127.0.0.1:8096")]
    pub bind: SocketAddr,

    /// Jellyfin backend URL.
    #[arg(short = 'j', long, default_value = "http://127.0.0.1:8096")]
    pub jellyfin_url: String,

    /// TLS certificate path (PEM format).
    #[arg(long)]
    pub tls_cert: Option<PathBuf>,

    /// TLS private key path (PEM format).
    #[arg(long)]
    pub tls_key: Option<PathBuf>,

    /// Logging level (trace, debug, info, warn, error).
    #[arg(long, default_value = "info")]
    pub log_level: String,
}

impl Config {
    /// Check if TLS is enabled.
    pub fn tls_enabled(&self) -> bool {
        self.tls_cert.is_some() && self.tls_key.is_some()
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), String> {
        // Validate Jellyfin URL
        if !self.jellyfin_url.starts_with("http://") && !self.jellyfin_url.starts_with("https://") {
            return Err("Jellyfin URL must start with http:// or https://".to_string());
        }

        // Validate TLS configuration
        if self.tls_cert.is_some() != self.tls_key.is_some() {
            return Err("Both --tls-cert and --tls-key must be provided together".to_string());
        }

        // Validate TLS file paths if provided
        if let Some(ref cert) = self.tls_cert {
            if !cert.exists() {
                return Err(format!("TLS certificate file not found: {}", cert.display()));
            }
        }

        if let Some(ref key) = self.tls_key {
            if !key.exists() {
                return Err(format!("TLS key file not found: {}", key.display()));
            }
        }

        Ok(())
    }
}
