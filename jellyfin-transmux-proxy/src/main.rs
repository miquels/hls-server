use axum::{routing::any, Router};
use clap::Parser;
use reqwest::Client;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tracing::Level;

pub mod hls;
pub mod playbackinfo;
pub mod proxy;
pub mod types;

use hls::proxymedia_handler;
use playbackinfo::playback_info_handler;
use proxy::{proxy_handler, websocket_handler};

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Jellyfin server URL to proxy to
    #[arg(short, long, default_value = "http://jf.high5.nl:8096")]
    jellyfin_url: String,

    /// Listen address
    #[arg(short, long, default_value = "127.0.0.1:8097")]
    listen: String,

    /// Media root directory to prepend to filesystem paths
    #[arg(short, long, default_value = "")]
    mediaroot: String,
}

pub struct AppState {
    jellyfin_url: String,
    media_root: String,
    http_client: Client,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "jellyfin_hls_proxy=info,tower_http=info".into()),
        )
        .init();

    let args = Args::parse();

    // Normalize jellyfin URL (remove trailing slash)
    let jellyfin_url = args.jellyfin_url.trim_end_matches('/').to_string();

    let http_client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;

    let state = Arc::new(AppState {
        jellyfin_url,
        media_root: args.mediaroot,
        http_client,
    });

    let app = Router::new()
        .route(
            "/Items/{item_id}/PlaybackInfo",
            axum::routing::post(playback_info_handler),
        )
        .route(
            "/proxymedia/{*path}",
            axum::routing::get(proxymedia_handler),
        )
        .route("/socket", axum::routing::get(websocket_handler))
        .fallback(any(proxy_handler))
        .layer(CorsLayer::permissive())
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_request(DefaultOnRequest::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(state);

    let addr: std::net::SocketAddr = args.listen.parse()?;
    tracing::info!("Listening on {}", addr);
    tracing::info!("Proxying to {}", args.jellyfin_url);

    axum_server::bind(addr)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}
