use axum::{
    body::Body,
    extract::{Request, State},
    http::{uri::Uri, StatusCode},
    response::Response,
    routing::any,
    Router,
};
use clap::Parser;
use reqwest::Client;
use std::sync::Arc;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Jellyfin server URL to proxy to
    #[arg(short, long, default_value = "http://127.0.0.1:8096")]
    jellyfin_url: String,

    /// Listen address
    #[arg(short, long, default_value = "127.0.0.1:8097")]
    listen: String,
}

struct AppState {
    jellyfin_url: String,
    http_client: Client,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    // Normalize jellyfin URL (remove trailing slash)
    let jellyfin_url = args.jellyfin_url.trim_end_matches('/').to_string();

    let state = Arc::new(AppState {
        jellyfin_url,
        http_client: Client::new(),
    });

    let app = Router::new()
        .route(
            "/Items/:item_id/PlaybackInfo",
            axum::routing::post(playback_info_handler),
        )
        .route("/proxymedia/*path", axum::routing::get(proxymedia_handler))
        .fallback(any(proxy_handler))
        .with_state(state);

    let addr: std::net::SocketAddr = args.listen.parse()?;
    tracing::info!("Listening on {}", addr);
    tracing::info!("Proxying to {}", args.jellyfin_url);

    axum_server::bind(addr)
        .serve(app.into_make_service())
        .await?;

    Ok(())
}

async fn proxy_handler(
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

    *req.uri_mut() = Uri::try_from(&uri).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut proxy_req = state.http_client.request(req.method().clone(), uri.clone());

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
        Ok(res) => res,
        Err(e) => {
            tracing::error!("Proxy error: {}", e);
            return Err(StatusCode::BAD_GATEWAY);
        }
    };

    let mut response_builder = Response::builder().status(res.status());

    // Copy response headers
    if let Some(headers) = response_builder.headers_mut() {
        for (name, value) in res.headers() {
            headers.insert(name.clone(), value.clone());
        }
    }

    // Stream the body
    let stream = res.bytes_stream();
    let body = Body::from_stream(stream);

    response_builder
        .body(body)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn playback_info_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(_item_id): axum::extract::Path<String>,
    req: Request,
) -> Result<Response, StatusCode> {
    let parts = req.into_parts();

    // Read the entire body
    let body_bytes = axum::body::to_bytes(parts.1, usize::MAX)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    let mut json: serde_json::Value =
        serde_json::from_slice(&body_bytes).unwrap_or_else(|_| serde_json::json!({}));

    let dp_profile = serde_json::json!({
        "Container": "mp4,m4v,mkv,webm",
        "VideoCodec": "h264,h265,vp9",
        "AudioCodec": "aac,mp3,ac3,eac3,opus",
        "Type": "Video"
    });

    if let Some(device_profile) = json.get_mut("DeviceProfile") {
        device_profile["DirectPlayProfiles"] = serde_json::json!([dp_profile]);
        device_profile["TranscodingProfiles"] = serde_json::json!([]);
    } else {
        json["DeviceProfile"] = serde_json::json!({
            "DirectPlayProfiles": [dp_profile],
            "TranscodingProfiles": []
        });
    }

    let modified_body = serde_json::to_vec(&json).unwrap();

    let path_query = parts
        .0
        .uri
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or(parts.0.uri.path());
    let uri = format!("{}{}", state.jellyfin_url, path_query);

    let mut proxy_req = state.http_client.request(parts.0.method.clone(), uri);

    for (name, value) in parts.0.headers.iter() {
        if name != reqwest::header::HOST && name != reqwest::header::CONTENT_LENGTH {
            proxy_req = proxy_req.header(name, value);
        }
    }
    proxy_req = proxy_req.header(
        reqwest::header::CONTENT_LENGTH,
        modified_body.len().to_string(),
    );
    proxy_req = proxy_req.body(modified_body);

    let res = proxy_req.send().await.map_err(|e| {
        tracing::error!("Proxy error in PlaybackInfo: {}", e);
        StatusCode::BAD_GATEWAY
    })?;

    let mut response_builder = Response::builder().status(res.status());
    let is_json = res
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("application/json"))
        .unwrap_or(false);

    if let Some(headers) = response_builder.headers_mut() {
        for (name, value) in res.headers() {
            if name != reqwest::header::CONTENT_LENGTH {
                headers.insert(name.clone(), value.clone());
            }
        }
    }

    if is_json && res.status().is_success() {
        let body_bytes = res
            .bytes()
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let mut json: serde_json::Value =
            serde_json::from_slice(&body_bytes).unwrap_or_else(|_| serde_json::json!({}));

        if let Some(media_sources) = json.get_mut("MediaSources").and_then(|v| v.as_array_mut()) {
            for source in media_sources.iter_mut() {
                if let Some(path) = source.get("Path").and_then(|v| v.as_str()) {
                    let clean_path = path.trim_start_matches('/');
                    let transcode_url =
                        format!("/proxymedia/{}.as.m3u8", urlencoding::encode(clean_path));

                    source["TranscodingUrl"] = serde_json::json!(transcode_url);
                    source["TranscodingSubProtocol"] = serde_json::json!("hls");
                    source["TranscodingContainer"] = serde_json::json!("ts");

                    source["SupportsDirectPlay"] = serde_json::json!(false);
                    source["SupportsDirectStream"] = serde_json::json!(false);
                    source["SupportsTranscoding"] = serde_json::json!(true);
                }
            }
        }

        let modified_body = serde_json::to_vec(&json).unwrap();

        if let Some(headers) = response_builder.headers_mut() {
            headers.insert(
                reqwest::header::CONTENT_LENGTH,
                modified_body.len().to_string().parse().unwrap(),
            );
        }

        return response_builder
            .body(Body::from(modified_body))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR);
    }

    let content_len = res.headers().get(reqwest::header::CONTENT_LENGTH).cloned();
    if let Some(len) = content_len {
        if let Some(headers) = response_builder.headers_mut() {
            headers.insert(reqwest::header::CONTENT_LENGTH, len);
        }
    }

    let stream = res.bytes_stream();
    let body = Body::from_stream(stream);

    response_builder
        .body(body)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn proxymedia_handler(
    axum::extract::Path(path): axum::extract::Path<String>,
    axum::extract::Query(query_params): axum::extract::Query<
        std::collections::HashMap<String, String>,
    >,
) -> Result<Response, StatusCode> {
    tracing::info!("Proxymedia request for path: {}", path);
    // Path comes in like Users/mikevs/Devel/...
    let mut clean_path = path.clone();
    if !clean_path.starts_with('/') {
        clean_path = format!("/{}", clean_path);
    }

    // Fallback to removing the leading slash if parsing fails (for the relative paths)
    let hls_url = match hls_vod_lib::HlsParams::parse(&clean_path) {
        Some(params) => params,
        None => hls_vod_lib::HlsParams::parse(&path).ok_or_else(|| {
            tracing::error!("Invalid HLS request: {}", path);
            StatusCode::BAD_REQUEST
        })?,
    };

    tracing::info!("Parsed HLS URL: {:?}", hls_url);

    let media_path = std::path::PathBuf::from(&hls_url.video_url);
    if !media_path.exists() {
        tracing::error!("Media file not found: {:?}", media_path);
        return Err(StatusCode::NOT_FOUND);
    }

    tokio::task::spawn_blocking(move || {
        let mut hls_video = hls_vod_lib::HlsVideo::open(&media_path, hls_url).map_err(|e| {
            tracing::error!("Failed to open media: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        if let hls_vod_lib::HlsVideo::MainPlaylist(p) = &mut hls_video {
            let codecs: Vec<String> = query_params
                .get("codecs")
                .map(|s| s.split(',').map(|c| c.trim().to_string()).collect())
                .unwrap_or_default();
            p.filter_codecs(&codecs);

            let tracks: Vec<usize> = query_params
                .get("tracks")
                .map(|s| {
                    s.split(',')
                        .filter_map(|s| s.parse::<usize>().ok())
                        .collect::<Vec<usize>>()
                })
                .unwrap_or_default();
            if !tracks.is_empty() {
                p.enable_tracks(&tracks);
            }

            if query_params
                .get("interleave")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false)
            {
                p.interleave();
            }
        }

        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            axum::http::header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static(hls_video.mime_type()),
        );
        headers.insert(
            axum::http::header::CACHE_CONTROL,
            axum::http::HeaderValue::from_static(hls_video.cache_control()),
        );

        let bytes = hls_video.generate().map_err(|e| {
            tracing::error!("Failed to generate HLS data: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        let mut response = Response::new(Body::from(bytes));
        *response.headers_mut() = headers;
        Ok(response)
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
}
