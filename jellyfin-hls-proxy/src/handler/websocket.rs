//! WebSocket proxy handler for forwarding WebSocket connections to Jellyfin.

use axum::{
    body::Body,
    extract::{Path, State},
    http::Request,
    response::Response,
};
use axum::extract::ws::{WebSocket, WebSocketUpgrade, Message as WsMessage};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{info, warn};

use crate::handler::playback::AppState;

/// Handle WebSocket upgrade requests and proxy them to Jellyfin.
pub async fn websocket_proxy(
    State(state): State<Arc<AppState>>,
    ws: WebSocketUpgrade,
    Path(path): Path<String>,
) -> Response {
    let ws_url = build_ws_url(&state.proxy_service.base_url(), &format!("/{}", path));
    info!("WebSocket upgrade request for path: {}", path);

    ws.on_upgrade(move |socket| async move {
        info!("Client WebSocket connection established");
        
        // Connect to Jellyfin's WebSocket
        match connect_async(&ws_url).await {
            Ok((backend_ws, _)) => {
                info!("Backend WebSocket connection established to {}", ws_url);
                proxy_websocket(socket, backend_ws).await;
            }
            Err(e) => {
                warn!("Failed to connect to backend WebSocket: {}", e);
            }
        }
    })
}

/// Build a WebSocket URL from the base URL and request path.
fn build_ws_url(base_url: &str, path: &str) -> String {
    // Convert http(s) URL to ws(s) URL
    let ws_base = base_url
        .replace("http://", "ws://")
        .replace("https://", "wss://");
    
    format!("{}{}", ws_base, path)
}

/// Proxy messages between client and backend WebSockets.
async fn proxy_websocket(client_ws: WebSocket, backend_ws: tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>) {
    let (mut client_sink, mut client_stream) = client_ws.split();
    let (mut backend_sink, mut backend_stream) = backend_ws.split();

    // Forward client -> backend
    let client_to_backend = async {
        while let Some(msg) = client_stream.next().await {
            match msg {
                Ok(WsMessage::Text(text)) => {
                    // Convert axum's Utf8Bytes to String, then to tungstenite's Utf8Bytes
                    if let Err(e) = backend_sink.send(Message::Text(text.to_string().into())).await {
                        warn!("Error sending to backend: {}", e);
                        break;
                    }
                }
                Ok(WsMessage::Binary(data)) => {
                    if let Err(e) = backend_sink.send(Message::Binary(data)).await {
                        warn!("Error sending to backend: {}", e);
                        break;
                    }
                }
                Ok(WsMessage::Close(frame)) => {
                    let close_frame = frame.map(|f| {
                        tokio_tungstenite::tungstenite::protocol::CloseFrame {
                            code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::Normal,
                            reason: f.reason.to_string().into(),
                        }
                    });
                    let _ = backend_sink.send(Message::Close(close_frame)).await;
                    break;
                }
                Ok(WsMessage::Ping(data)) => {
                    let _ = backend_sink.send(Message::Ping(data)).await;
                }
                Ok(WsMessage::Pong(data)) => {
                    let _ = backend_sink.send(Message::Pong(data)).await;
                }
                Err(e) => {
                    warn!("Client WebSocket error: {}", e);
                    break;
                }
            }
        }
    };

    // Forward backend -> client
    let backend_to_client = async {
        while let Some(msg) = backend_stream.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    // Convert tungstenite's Utf8Bytes to String, then to axum's Utf8Bytes
                    if let Err(e) = client_sink.send(WsMessage::Text(text.to_string().into())).await {
                        warn!("Error sending to client: {}", e);
                        break;
                    }
                }
                Ok(Message::Binary(data)) => {
                    if let Err(e) = client_sink.send(WsMessage::Binary(data)).await {
                        warn!("Error sending to client: {}", e);
                        break;
                    }
                }
                Ok(Message::Close(frame)) => {
                    let close_frame = frame.map(|f| {
                        axum::extract::ws::CloseFrame {
                            code: f.code.into(),
                            reason: f.reason.to_string().into(),
                        }
                    });
                    let _ = client_sink.send(WsMessage::Close(close_frame)).await;
                    break;
                }
                Ok(Message::Ping(data)) => {
                    let _ = client_sink.send(WsMessage::Ping(data)).await;
                }
                Ok(Message::Pong(data)) => {
                    let _ = client_sink.send(WsMessage::Pong(data)).await;
                }
                Ok(Message::Frame(_)) => {
                    // Ignore raw frames
                }
                Err(e) => {
                    warn!("Backend WebSocket error: {}", e);
                    break;
                }
            }
        }
    };

    // Run both directions concurrently
    tokio::select! {
        _ = client_to_backend => {},
        _ = backend_to_client => {},
    }

    info!("WebSocket proxy closed");
}

/// Check if a request is a WebSocket upgrade request.
pub fn is_websocket_upgrade(req: &Request<Body>) -> bool {
    req.headers()
        .get("upgrade")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false)
}
