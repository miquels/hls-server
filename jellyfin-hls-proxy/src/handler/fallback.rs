//! Fallback proxy handler for forwarding unmatched requests to Jellyfin.

use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    response::Response,
};
use http_body_util::BodyExt;
use std::sync::Arc;
use tracing::{info, warn};

use crate::handler::playback::AppState;
use crate::handler::websocket;

/// Fallback handler that proxies all unmatched requests to Jellyfin.
pub async fn fallback_proxy(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Result<Response, StatusCode> {
    // Check if this is a WebSocket upgrade request
    if websocket::is_websocket_upgrade(&req) {
        warn!("WebSocket upgrade detected in fallback - use dedicated route for WebSocket");
        // For now, return an error - WebSocket should be handled by a dedicated route
        return Err(StatusCode::BAD_REQUEST);
    }

    let method = req.method().clone();
    let uri = req.uri().clone();
    
    info!("Proxying request: {} {}", method, uri);

    // Convert axum Body to http_body_util::Full<Bytes>
    let (parts, body) = req.into_parts();
    
    let body_bytes = body
        .collect()
        .await
        .map(|b| b.to_bytes())
        .map_err(|e| {
            warn!("Failed to collect request body: {}", e);
            StatusCode::BAD_REQUEST
        })?;

    // Rebuild the request with Full body
    let req = http::Request::from_parts(parts, http_body_util::Full::new(body_bytes));

    // Forward to Jellyfin via proxy service
    match state.proxy_service.forward(req).await {
        Ok(response) => {
            // Convert back to axum Response
            // Full<Bytes> already contains the bytes, we can collect them
            let (parts, body) = response.into_parts();
            let collected = body
                .collect()
                .await
                .map_err(|_| StatusCode::BAD_GATEWAY)?;
            let body = Body::from(collected.to_bytes());
            
            let mut response = Response::new(body);
            *response.headers_mut() = parts.headers;
            *response.status_mut() = StatusCode::from_u16(parts.status.into())
                .unwrap_or(StatusCode::OK);
            
            info!("Proxy response: {}", response.status());
            Ok(response)
        }
        Err(e) => {
            warn!("Proxy error for {} {}: {}", method, uri, e);
            Err(StatusCode::BAD_GATEWAY)
        }
    }
}
