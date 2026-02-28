//! Axum server setup and configuration.

use crate::config::Config;
use crate::error::Result;
use crate::handler::fallback::fallback_proxy;
use crate::handler::playback::{handle_playback_info, AppState};
use crate::handler::proxymedia::handle_proxymedia;
use crate::handler::websocket::websocket_proxy;
use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use tracing::info;

/// Create the main router for the application.
pub fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        // Health check endpoint
        .route("/health", get(|| async { "OK" }))
        // Playback info interception
        .route("/Items/:item_id/PlaybackInfo", post(handle_playback_info))
        // Proxy media endpoint for HLS
        .route("/proxymedia/*path", get(handle_proxymedia))
        // WebSocket proxy (catch-all for WebSocket upgrades)
        .route("/socket", get(websocket_proxy))
        .route("/socket/*path", get(websocket_proxy))
        // Fallback: proxy all other requests to Jellyfin
        .fallback(fallback_proxy)
        // Add tracing layer
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Run the HTTP server.
pub async fn run_http_server(config: &Config, router: Router) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(config.bind).await?;

    info!("Starting HTTP server on {}", config.bind);

    axum::serve(listener, router).await?;

    Ok(())
}
