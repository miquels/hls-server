use axum::{
    body::Body,
    extract::{Request, State},
    http::{uri::Uri, StatusCode},
    response::Response,
};
use std::sync::Arc;

use crate::AppState;

pub async fn proxy_handler(
    State(state): State<Arc<AppState>>,
    mut req: Request,
) -> Result<Response, StatusCode> {
    let path = req.uri().path();
    let path_query = req
        .uri()
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or(path);

    let uri = format!("{}{}", state.jellyfin_url, path_query);
    let uri_str = uri.clone();
    tracing::info!(
        "Proxying {} {} to {}",
        req.method(),
        req.uri().path(),
        uri_str
    );

    *req.uri_mut() = Uri::try_from(&uri).map_err(|_| {
        tracing::error!("Invalid URI for proxy: {}", uri_str);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let mut proxy_req = state
        .http_client
        .request(req.method().clone(), uri_str.clone());

    // Copy headers
    for (name, value) in req.headers() {
        if name != reqwest::header::HOST {
            proxy_req = proxy_req.header(name, value);
        }
    }

    // Copy body
    let body_bytes = axum::body::to_bytes(req.into_body(), usize::MAX)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    proxy_req = proxy_req.body(body_bytes);

    let res = match proxy_req.send().await {
        Ok(res) => {
            tracing::info!("Upstream response for {}: {}", uri_str, res.status());
            res
        }
        Err(e) => {
            tracing::error!("Proxy error for {}: {}", uri_str, e);
            return Err(StatusCode::BAD_GATEWAY);
        }
    };

    let mut response_builder = Response::builder().status(res.status());

    // Copy response headers
    if let Some(headers) = response_builder.headers_mut() {
        for (name, value) in res.headers() {
            if name == reqwest::header::LOCATION {
                if let Ok(loc_str) = value.to_str() {
                    // Rewrite absolute upstream URLs to relative root URLs
                    if loc_str.starts_with(&state.jellyfin_url) {
                        let new_loc = loc_str.replace(&state.jellyfin_url, "");
                        // Ensure it's not totally empty if it was exactly the root
                        let new_loc = if new_loc.is_empty() {
                            "/".to_string()
                        } else {
                            new_loc
                        };
                        if let Ok(new_val) = axum::http::HeaderValue::from_str(&new_loc) {
                            headers.insert(name.clone(), new_val);
                            continue;
                        }
                    }
                }
            }
            // Strip hop-by-hop headers
            if name != reqwest::header::TRANSFER_ENCODING && name != reqwest::header::CONNECTION {
                headers.insert(name.clone(), value.clone());
            }
        }
    }

    // Stream the body
    let stream = res.bytes_stream();
    let body = Body::from_stream(stream);

    response_builder.body(body).map_err(|e| {
        tracing::error!("Response building error in proxy: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })
}
