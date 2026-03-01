use axum::{
    body::{Body, Bytes},
    extract::{Request, State},
    http::{header::HeaderMap, method::Method, uri::Uri, StatusCode},
    response::Response,
    routing::any,
    Router,
};
use clap::Parser;
use reqwest::Client;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tracing::Level;

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

struct AppState {
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

async fn playback_info_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(_item_id): axum::extract::Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, StatusCode> {
    tracing::info!("PlaybackInfo request received: {} {}", method, uri.path());
    tracing::info!("Read PlaybackInfo body: {} bytes", body.len());

    let mut json: serde_json::Value =
        serde_json::from_slice(&body).unwrap_or_else(|_| serde_json::json!({}));

    mutate_playback_info_request(&mut json);

    let modified_body = serde_json::to_vec(&json).unwrap();

    let path_query = uri
        .path_and_query()
        .map(|v| v.as_str())
        .unwrap_or(uri.path());
    let upstream_uri = format!("{}{}", state.jellyfin_url, path_query);
    tracing::info!("Proxying PlaybackInfo to {}", upstream_uri);

    let mut proxy_req = state.http_client.request(method, upstream_uri.clone());

    for (name, value) in headers.iter() {
        if name != reqwest::header::HOST
            && name != reqwest::header::CONTENT_LENGTH
            && name != reqwest::header::ACCEPT_ENCODING
        {
            proxy_req = proxy_req.header(name, value);
        }
    }
    proxy_req = proxy_req.header(
        reqwest::header::CONTENT_LENGTH,
        modified_body.len().to_string(),
    );
    proxy_req = proxy_req.body(modified_body);

    let res = proxy_req.send().await.map_err(|e| {
        tracing::error!("Proxy error in PlaybackInfo for {}: {}", upstream_uri, e);
        StatusCode::BAD_GATEWAY
    })?;
    tracing::info!("PlaybackInfo upstream response: {}", res.status());

    let mut response_builder = Response::builder().status(res.status());
    let is_json = res
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("application/json"))
        .unwrap_or(false);

    if let Some(resp_headers) = response_builder.headers_mut() {
        for (name, value) in res.headers() {
            if name != reqwest::header::CONTENT_LENGTH
                && name != reqwest::header::CONTENT_ENCODING
                && name != reqwest::header::TRANSFER_ENCODING
                && name != reqwest::header::CONNECTION
            {
                resp_headers.insert(name.clone(), value.clone());
            }
        }
    }

    if is_json && res.status().is_success() {
        let resp_body_bytes = res.bytes().await.map_err(|e| {
            tracing::error!("Failed to read PlaybackInfo upstream body: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
        tracing::info!(
            "Read PlaybackInfo upstream body: {} bytes",
            resp_body_bytes.len()
        );

        let mut resp_json: serde_json::Value =
            serde_json::from_slice(&resp_body_bytes).unwrap_or_else(|_| serde_json::json!({}));

        mutate_playback_info_response(&mut resp_json);

        let modified_resp_body = serde_json::to_vec(&resp_json).unwrap();

        if let Some(resp_headers) = response_builder.headers_mut() {
            resp_headers.insert(
                axum::http::header::CONTENT_LENGTH,
                axum::http::HeaderValue::from(modified_resp_body.len()),
            );
        }

        tracing::info!(
            "Returning mutated PlaybackInfo response, size: {}",
            modified_resp_body.len()
        );

        return response_builder
            .body(Body::from(modified_resp_body))
            .map_err(|e| {
                tracing::error!("Response building error in PlaybackInfo branch: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            });
    }

    let content_len = res.headers().get(reqwest::header::CONTENT_LENGTH).cloned();
    if let Some(len) = content_len {
        if let Some(resp_headers) = response_builder.headers_mut() {
            resp_headers.insert(reqwest::header::CONTENT_LENGTH, len);
        }
    }

    let stream = res.bytes_stream();
    let body = Body::from_stream(stream);

    response_builder.body(body).map_err(|e| {
        tracing::error!("Response building error in PlaybackInfo fallback: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })
}

async fn proxymedia_handler(
    State(state): State<Arc<AppState>>,
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

    let mut media_path = std::path::PathBuf::from(&hls_url.video_url);

    // If media_root is set, prepend it to the path
    if !state.media_root.is_empty() {
        let root = std::path::Path::new(&state.media_root);
        // We want to join them. If hls_url.video_url starts with /, we might need to be careful
        // depending on if we want it to be relative to root.
        // Usually joining an absolute path with another path makes it absolute.
        // Let's trim leading slash if we have a root.
        let video_url = hls_url.video_url.trim_start_matches('/');
        media_path = root.join(video_url);
        tracing::info!("Prepended media_root: {:?}", media_path);
    }

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

fn mutate_playback_info_request(json: &mut serde_json::Value) {
    if !json.is_object() {
        return;
    }
    let dp_profile = serde_json::json!({
        "Container": "mp4,m4v,mkv,webm",
        "VideoCodec": "h264,h265,vp9",
        "AudioCodec": "aac,mp3,ac3,eac3,opus",
        "Type": "Video"
    });

    if let Some(device_profile) = json
        .get_mut("DeviceProfile")
        .and_then(|v| v.as_object_mut())
    {
        device_profile.insert(
            "DirectPlayProfiles".to_string(),
            serde_json::json!([dp_profile]),
        );
        device_profile.insert("TranscodingProfiles".to_string(), serde_json::json!([]));
    } else {
        json["DeviceProfile"] = serde_json::json!({
            "DirectPlayProfiles": [dp_profile],
            "TranscodingProfiles": []
        });
    }
}

fn mutate_playback_info_response(json: &mut serde_json::Value) {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mutate_playback_info_request() {
        let mut json = serde_json::json!({
            "DeviceProfile": {
                "TranscodingProfiles": [{"Container": "mp3"}],
                "DirectPlayProfiles": []
            }
        });
        mutate_playback_info_request(&mut json);
        let device_profile = &json["DeviceProfile"];
        assert_eq!(
            device_profile["TranscodingProfiles"]
                .as_array()
                .unwrap()
                .len(),
            0
        );
        assert_eq!(
            device_profile["DirectPlayProfiles"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let dp = &device_profile["DirectPlayProfiles"][0];
        assert_eq!(dp["VideoCodec"], "h264,h265,vp9");
    }

    #[test]
    fn test_mutate_playback_info_response() {
        let mut json = serde_json::json!({
            "MediaSources": [
                {
                    "Path": "/some/media/file.mp4"
                }
            ]
        });
        mutate_playback_info_response(&mut json);
        let media_sources = &json["MediaSources"][0];
        assert_eq!(media_sources["SupportsDirectPlay"].as_bool(), Some(false));
        assert_eq!(media_sources["SupportsTranscoding"].as_bool(), Some(true));
        assert_eq!(
            media_sources["TranscodingUrl"].as_str().unwrap(),
            "/proxymedia/some%2Fmedia%2Ffile.mp4.as.m3u8"
        );
    }
}
