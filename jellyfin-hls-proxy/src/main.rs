//! Jellyfin HLS transmuxing proxy server.
//!
//! This proxy interceptts Jellyfin media playback requests and handles
//! transcoding/transmuxing using hls-vod-lib instead of relying on
//! Jellyfin's ffmpeg-based approach.

mod config;
mod error;
mod handler;
mod jellyfin;
mod proxy;
mod server;

use clap::Parser;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::Config;
use crate::error::Result;
use crate::handler::playback::AppState;
use crate::proxy::ProxyService;

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let config = Config::parse();

    // Validate configuration
    config.validate().map_err(|e| error::ProxyError::Config(e))?;

    // Initialize logging
    init_logging(&config);

    info!("Starting Jellyfin HLS Proxy");
    info!("Bind address: {}", config.bind);
    info!("Jellyfin URL: {}", config.jellyfin_url);

    if config.tls_enabled() {
        info!("TLS enabled (will be implemented in a later milestone)");
    }

    // Create proxy service
    let proxy_service = ProxyService::new(&config)?;

    // Create application state
    let state = Arc::new(AppState {
        proxy_service,
        proxy_base: "/proxymedia".to_string(),
    });

    // Create router
    let router = server::create_router(state);

    // Run server (TLS support to be added later)
    server::run_http_server(&config, router).await?;

    Ok(())
}

/// Initialize logging based on configuration.
fn init_logging(config: &Config) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| config.log_level.parse().unwrap_or_else(|_| "info".parse().unwrap()));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}
