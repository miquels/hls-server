//! Handler for PlaybackInfo interception.

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use http_body_util::BodyExt;
use serde_json::Value;
use std::sync::Arc;

use crate::jellyfin::DeviceProfile;
use crate::proxy::ProxyService;

/// Application state.
#[derive(Clone)]
pub struct AppState {
    pub proxy_service: ProxyService,
    pub proxy_base: String,
}

/// Extract item ID from path.
#[derive(serde::Deserialize)]
pub struct ItemIdParams {
    pub item_id: String,
}

/// Intercept and modify PlaybackInfo request.
pub async fn handle_playback_info(
    State(state): State<Arc<AppState>>,
    Path(params): Path<ItemIdParams>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    // Parse the body as JSON
    let mut request_body: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => Value::Object(serde_json::Map::new()),
    };

    // Inject our device profile with DirectPlayProfiles
    let device_profile = DeviceProfile::default();
    if let Some(profile_obj) = request_body.get_mut("DeviceProfile") {
        // Merge with existing profile or replace
        *profile_obj = serde_json::to_value(&device_profile).unwrap_or_default();
    } else {
        request_body["DeviceProfile"] = serde_json::to_value(&device_profile).unwrap_or_default();
    }

    // Forward the modified request to Jellyfin
    let modified_body = serde_json::to_vec(&request_body).unwrap_or_default();

    let path = format!("/Items/{}/PlaybackInfo", params.item_id);

    // Build the request
    let url = format!("{}{}", state.proxy_service.base_url(), path);
    
    let mut req_builder = http::Request::builder()
        .method(http::Method::POST)
        .uri(&url)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json");

    // Copy some headers from the original request
    for (key, value) in &headers {
        if key == "authorization" || key == "x-emby-authorization" {
            req_builder = req_builder.header(key, value);
        }
    }

    let request = match req_builder
        .body(http_body_util::Full::new(Bytes::from(modified_body)))
    {
        Ok(req) => req,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to build request: {}", e),
            )
                .into_response();
        }
    };

    match state.proxy_service.forward(request).await {
        Ok(response) => {
            let status = StatusCode::from_u16(response.status().into())
                .unwrap_or(StatusCode::OK);
            
            let mut out_headers = HeaderMap::new();
            for (key, value) in response.headers() {
                out_headers.insert(key.clone(), value.clone());
            }

            let body_bytes = response.into_body().collect().await
                .map(|b| b.to_bytes())
                .unwrap_or_default();

            (status, out_headers, body_bytes).into_response()
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            format!("Failed to communicate with Jellyfin: {}", e),
        )
            .into_response(),
    }
}
