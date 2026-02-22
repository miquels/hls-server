//! Axum router configuration

use axum::{
    http::{header, Method},
    routing::{any, get},
    Router,
};
use std::sync::Arc;
use std::time::Duration;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::state::AppState;

use super::dynamic::handle_dynamic_request;
use super::handlers::{active_streams, cache_stats, health_check, version_check};

/// Create the Axum router with all routes
pub fn create_router(state: Arc<AppState>) -> Router {
    // Build CORS layer
    // Safari and other modern browsers often require explicit headers
    // and private network access for local development.
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS, Method::HEAD])
        .allow_headers([
            header::ACCEPT,
            header::RANGE,
            header::CONTENT_TYPE,
            header::ORIGIN,
        ])
        .allow_private_network(true)
        .max_age(Duration::from_secs(3600));

    // Build router
    Router::new()
        // Health and version endpoints
        .route("/health", get(health_check))
        .route("/version", get(version_check))
        // Debug endpoints
        .route("/debug/cache", get(cache_stats))
        .route("/debug/streams", get(active_streams))
        // Media wildcard
        // Using `any` ensures that `OPTIONS` requests to media paths
        // are handled correctly by the handler or CORS layer.
        .route("/{*path}", any(handle_dynamic_request))
        // Middleware
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        // State
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ServerConfig;

    #[test]
    fn test_create_router() {
        let state = Arc::new(AppState::new(ServerConfig::default()));
        let _router = create_router(state);
        // Router creation successful
    }

    #[tokio::test]
    async fn test_cors_options() {
        use axum::body::Body;
        use axum::http::{Request, StatusCode};
        use tower::util::ServiceExt; // Use tower::util::ServiceExt for oneshot

        let state = Arc::new(AppState::new(ServerConfig::default()));
        let app = create_router(state);

        // Pre-flight OPTIONS request
        let request = Request::builder()
            .method(Method::OPTIONS)
            .uri("/test.mp4/master.m3u8")
            .header(header::ORIGIN, "http://localhost:8080")
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
            .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "range")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .unwrap(),
            "*"
        );
        assert!(response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_METHODS)
            .unwrap()
            .to_str()
            .unwrap()
            .contains("GET"));
    }
}
