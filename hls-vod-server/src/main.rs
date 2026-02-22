//! HLS Streaming Server
//!
//! A Rust-based HLS server that serves MP4/MKV files as fMP4/CMAF segments
//! using the hls-vod-lib core.

#![allow(dead_code)]
#![allow(unused_variables)]

mod config;
mod config_file;
mod error;
mod http;
mod integration;
mod limits;
mod metrics;
mod state;

use std::net::SocketAddr;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::ServerConfig;
use crate::error::Result;
use crate::http::create_router;
use crate::state::AppState;

/// Application version
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Application name
const APP_NAME: &str = "hls-vod-server";

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    init_logging();

    tracing::info!("{} v{} starting", APP_NAME, VERSION);
    tracing::info!(
        "FFmpeg version: {}",
        hls_vod_lib::ffmpeg_utils::version_info()
    );

    // Initialize FFmpeg and install log filter
    hls_vod_lib::ffmpeg_utils::init()
        .map_err(|e| crate::error::ServerError::Internal(e.to_string()))?;
    hls_vod_lib::ffmpeg_utils::install_log_filter();
    tracing::info!("FFmpeg initialized successfully");

    // Load configuration
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.toml".to_string());
    let config = if std::path::Path::new(&config_path).exists() {
        match crate::config_file::ConfigFile::from_file(&config_path) {
            Ok(cf) => cf.into_server_config(),
            Err(e) => {
                tracing::warn!(
                    "Failed to load config file {}: {}. Using defaults.",
                    config_path,
                    e
                );
                ServerConfig::default()
            }
        }
    } else {
        ServerConfig::default()
    };
    tracing::info!("Configuration loaded: {:?}", config);

    // Create application state
    let state = Arc::new(AppState::new(config.clone()));

    // Background task: evict expired streams every 60 seconds.
    {
        let state_bg = Arc::clone(&state);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                let removed = state_bg.cleanup_expired_streams();
                if removed > 0 {
                    tracing::info!("Evicted {} expired stream(s)", removed);
                }
            }
        });
    }

    // Build router
    let app = create_router(state.clone());

    // Start server
    let addr: SocketAddr = config.socket_addr().parse().unwrap();
    tracing::info!("Starting HTTP server on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();

    Ok(())
}

/// Initialize logging with tracing
fn init_logging() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "hls_vod_server=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }
}
