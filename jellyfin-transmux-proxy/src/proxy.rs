use axum::{
    body::Body,
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Request, State,
    },
    http::{uri::Uri, StatusCode},
    response::{IntoResponse, Response},
};
use std::sync::Arc;

use crate::AppState;

pub async fn proxy_handler(
    State(state): State<Arc<AppState>>,
    mut req: Request,
) -> Result<Response, StatusCode> {
    let path = req.uri().path();

    if req.method() == axum::http::Method::DELETE && path == "/Videos/ActiveEncodings" {
        if let Some(query) = req.uri().query() {
            if let Ok(params) =
                serde_urlencoded::from_str::<std::collections::HashMap<String, String>>(query)
            {
                if let Some(session_id) = params.get("playSessionId") {
                    if hls_vod_lib::cache::remove_stream_by_id(session_id) {
                        tracing::info!(
                            "Removed active encoding stream cache for session: {}",
                            session_id
                        );
                        return Ok(Response::builder()
                            .status(StatusCode::OK)
                            .body(Body::empty())
                            .unwrap());
                    }
                }
            }
        }
    }

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

    // Stream the body
    let body_stream = req.into_body().into_data_stream();
    proxy_req = proxy_req.body(reqwest::Body::wrap_stream(body_stream));

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

pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    req: Request,
) -> Response {
    let path_query = req
        .uri()
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or(req.uri().path());

    let ws_url = format!(
        "{}{}",
        state
            .jellyfin_url
            .replace("http://", "ws://")
            .replace("https://", "wss://"),
        path_query
    );

    tracing::info!("Proxying WebSocket to {}", ws_url);

    let mut ws_req =
        match tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(
            ws_url.clone(),
        ) {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("Failed to create WS request for {}: {}", ws_url, e);
                return (StatusCode::BAD_REQUEST, "Invalid WS URL").into_response();
            }
        };

    for (name, value) in req.headers() {
        if name != reqwest::header::HOST
            && name != reqwest::header::SEC_WEBSOCKET_KEY
            && name != reqwest::header::SEC_WEBSOCKET_ACCEPT
            && name != reqwest::header::SEC_WEBSOCKET_VERSION
            && name != reqwest::header::CONNECTION
            && name != reqwest::header::UPGRADE
        {
            ws_req.headers_mut().insert(name.clone(), value.clone());
        }
    }

    ws.on_upgrade(move |socket| handle_socket(socket, ws_req))
}

async fn handle_socket(
    client_socket: WebSocket,
    upstream_req: tokio_tungstenite::tungstenite::handshake::client::Request,
) {
    use futures_util::{SinkExt, StreamExt};

    let (upstream_socket, response) = match tokio_tungstenite::connect_async(upstream_req).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("Failed to connect to upstream WebSocket: {}", e);
            return;
        }
    };

    tracing::info!(
        "Upstream WebSocket connected with status: {}",
        response.status()
    );

    let (mut upstream_tx, mut upstream_rx) = upstream_socket.split();
    let (mut client_tx, mut client_rx) = client_socket.split();

    let client_to_upstream = tokio::spawn(async move {
        while let Some(msg) = client_rx.next().await {
            match msg {
                Ok(Message::Text(t)) => {
                    if upstream_tx
                        .send(tokio_tungstenite::tungstenite::Message::Text(
                            t.to_string().into(),
                        ))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(Message::Binary(b)) => {
                    if upstream_tx
                        .send(tokio_tungstenite::tungstenite::Message::Binary(
                            b.to_vec().into(),
                        ))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(Message::Ping(p)) => {
                    if upstream_tx
                        .send(tokio_tungstenite::tungstenite::Message::Ping(
                            p.to_vec().into(),
                        ))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(Message::Pong(p)) => {
                    if upstream_tx
                        .send(tokio_tungstenite::tungstenite::Message::Pong(
                            p.to_vec().into(),
                        ))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(Message::Close(c)) => {
                    let close_frame = c.map(|c| tokio_tungstenite::tungstenite::protocol::CloseFrame {
                        code: tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode::from(c.code),
                        reason: c.reason.to_string().into(),
                    });
                    if upstream_tx
                        .send(tokio_tungstenite::tungstenite::Message::Close(close_frame))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let upstream_to_client = tokio::spawn(async move {
        while let Some(msg) = upstream_rx.next().await {
            match msg {
                Ok(tokio_tungstenite::tungstenite::Message::Text(t)) => {
                    if client_tx
                        .send(Message::Text(t.to_string().into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(tokio_tungstenite::tungstenite::Message::Binary(b)) => {
                    if client_tx
                        .send(Message::Binary(b.to_vec().into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(tokio_tungstenite::tungstenite::Message::Ping(p)) => {
                    if client_tx
                        .send(Message::Ping(p.to_vec().into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(tokio_tungstenite::tungstenite::Message::Pong(p)) => {
                    if client_tx
                        .send(Message::Pong(p.to_vec().into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(tokio_tungstenite::tungstenite::Message::Close(c)) => {
                    let close_msg = c.map(|c| axum::extract::ws::CloseFrame {
                        code: c.code.into(),
                        reason: c.reason.to_string().into(),
                    });
                    if client_tx.send(Message::Close(close_msg)).await.is_err() {
                        break;
                    }
                }
                Ok(tokio_tungstenite::tungstenite::Message::Frame(_)) => {}
                Err(_) => break,
            }
        }
    });

    tokio::select! {
        _ = client_to_upstream => {}
        _ = upstream_to_client => {}
    }
}
