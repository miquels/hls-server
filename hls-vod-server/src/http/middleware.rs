//! HTTP middleware
//!
//! Additional middleware for the HTTP server.

use axum::{body::Body, http::Request, middleware::Next, response::Response};
use std::time::Instant;
use tracing::{info, warn};

/// Request logging middleware
pub async fn request_logger(request: Request<Body>, next: Next) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let start = Instant::now();

    let response = next.run(request).await;

    let duration = start.elapsed();
    let status = response.status();

    if status.is_success() {
        info!("{} {} {} in {:?}", method, uri, status, duration);
    } else if status.is_client_error() {
        warn!("{} {} {} in {:?}", method, uri, status, duration);
    } else {
        warn!("{} {} {} in {:?}", method, uri, status, duration);
    }

    response
}

/// Rate limiting middleware (placeholder)
///
/// TODO: Implement proper rate limiting with:
/// - Token bucket algorithm
/// - Per-IP limits
/// - Configurable limits
pub async fn rate_limiter(request: Request<Body>, next: Next) -> Response {
    // For now, pass through all requests
    next.run(request).await
}

/// Connection limit middleware (placeholder)
///
/// TODO: Implement connection limiting with:
/// - Max concurrent connections
/// - Per-IP connection limits
/// - Queue for excess connections
pub async fn connection_limiter(request: Request<Body>, next: Next) -> Response {
    // For now, pass through all requests
    next.run(request).await
}
