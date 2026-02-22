//! HLS Streaming Server
//!
//! A Rust-based HLS server that serves MP4/MKV files as fMP4/CMAF segments
//! without transcoding video, with intelligent audio track handling and
//! on-the-fly subtitle conversion to WebVTT.

#![allow(dead_code)]
#![allow(unused_variables)]

mod audio_plan;
mod config;
mod config_file;
mod error;
mod ffmpeg;
mod http;
mod index;
mod integration;
mod limits;
mod metrics;
mod playlist;
mod segment;
mod state;
mod subtitle;
mod transcode;

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
const APP_NAME: &str = "hls-server";

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    init_logging();

    tracing::info!("{} v{} starting", APP_NAME, VERSION);
    tracing::info!("FFmpeg version: {}", ffmpeg::version_info());

    // Initialize FFmpeg
    ffmpeg::init()?;
    // Set FFmpeg log level once at startup. AV_LOG_WARNING suppresses the
    // verbose DEBUG/INFO output from the demuxer/muxer on every segment.
    // Setting this in Fmp4Muxer::new() (per-segment) caused a global write
    // race under concurrent requests and flooded stderr.
    unsafe {
        ffmpeg_next::ffi::av_log_set_level(ffmpeg_next::ffi::AV_LOG_WARNING as i32);
    }
    ffmpeg::install_log_filter();
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
                .unwrap_or_else(|_| "hls_server=debug,tower_http=debug".into()),
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
