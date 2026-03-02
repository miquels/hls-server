use axum::{routing::any, Router};
use clap::Parser;
use reqwest::Client;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tracing::Level;

pub mod config;
pub mod hls;
pub mod playbackinfo;
pub mod proxy;
pub mod types;

use config::Config;
use hls::proxymedia_handler;
use playbackinfo::playback_info_handler;
use proxy::{proxy_handler, websocket_handler};

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Jellyfin server URL to proxy to
    #[arg(short, long)]
    jellyfin_url: Option<String>,

    /// Listen address for HTTP (CLI override)
    #[arg(short, long)]
    listen: Option<String>,

    /// Media root directory to prepend to filesystem paths
    #[arg(short, long)]
    mediaroot: Option<String>,

    /// Path to config file
    #[arg(short, long, default_value = "jellyfix-transmux-proxy.toml")]
    config: String,
}

pub struct AppState {
    pub jellyfin_url: String,
    pub media_root: String,
    pub http_client: Client,
    pub safari_force_transcoding: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "jellyfin_transmux_proxy=info,tower_http=info".into()),
        )
        .init();

    let args = Args::parse();

    // Load config if it exists
    let config = if std::path::Path::new(&args.config).exists() {
        tracing::info!("Loading config from {}", args.config);
        Some(Config::load(&args.config)?)
    } else {
        tracing::info!("Config file {} not found, using CLI arguments", args.config);
        None
    };

    // Merge config and args
    let jellyfin_url = args
        .jellyfin_url
        .or_else(|| config.as_ref().map(|c| c.jellyfin.jellyfin.clone()))
        .unwrap_or_else(|| "http://jf.high5.nl:8096".to_string())
        .trim_end_matches('/')
        .to_string();

    let mediaroot = args
        .mediaroot
        .or_else(|| config.as_ref().and_then(|c| c.jellyfin.mediaroot.clone()))
        .unwrap_or_default();

    let safari_force_transcoding = config
        .as_ref()
        .map(|c| c.safari.force_transcoding)
        .unwrap_or(false);

    let http_client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;

    let state = Arc::new(AppState {
        jellyfin_url: jellyfin_url.clone(),
        media_root: mediaroot,
        http_client,
        safari_force_transcoding,
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

    let mut listeners = Vec::new();

    // HTTP listener
    let http_port = config.as_ref().and_then(|c| c.server.http_listen_port);
    let http_addr_str = args
        .listen
        .clone()
        .or_else(|| http_port.map(|p| format!("0.0.0.0:{}", p)));

    if let Some(addr_str) = http_addr_str {
        let addr: std::net::SocketAddr = addr_str.parse()?;
        let app_clone = app.clone();
        tracing::info!("Starting HTTP listener on {}", addr);
        listeners.push(tokio::spawn(async move {
            axum_server::bind(addr)
                .serve(app_clone.into_make_service())
                .await
        }));
    } else if config.is_none() && args.listen.is_none() {
        // Default fallback ONLY if no config file was found and no --listen arg provided.
        let addr: std::net::SocketAddr = "127.0.0.1:8097".parse()?;
        let app_clone = app.clone();
        tracing::info!("Starting default HTTP listener on {}", addr);
        listeners.push(tokio::spawn(async move {
            axum_server::bind(addr)
                .serve(app_clone.into_make_service())
                .await
        }));
    } else {
        tracing::info!("HTTP listener disabled (none configured)");
    }

    // HTTPS listener
    if let Some(c) = config.as_ref() {
        if let Some(port) = c.server.https_listen_port {
            let addr: std::net::SocketAddr = format!("0.0.0.0:{}", port).parse()?;

            match (&c.server.tls_cert, &c.server.tls_key) {
                (Some(cert), Some(key)) => {
                    tracing::info!("Starting HTTPS listener on {}", addr);
                    let rustls_config =
                        axum_server::tls_rustls::RustlsConfig::from_pem_file(cert, key).await?;
                    let app_clone = app.clone();

                    listeners.push(tokio::spawn(async move {
                        axum_server::bind_rustls(addr, rustls_config)
                            .serve(app_clone.into_make_service())
                            .await
                    }));
                }
                _ => {
                    tracing::error!(
                        "HTTPS listen port {} is configured, but tls_cert or tls_key is missing!",
                        port
                    );
                    return Err("HTTPS configuration incomplete: missng cert or key".into());
                }
            }
        } else {
            tracing::info!("HTTPS listener disabled (no port configured)");
        }
    } else {
        tracing::info!("HTTPS listener disabled (no config)");
    }

    tracing::info!("Proxying to {}", jellyfin_url);

    if listeners.is_empty() {
        tracing::error!("No listeners configured!");
        return Err("No listeners configured".into());
    }

    // Wait for all listeners
    for handle in listeners {
        handle.await??;
    }

    Ok(())
}
