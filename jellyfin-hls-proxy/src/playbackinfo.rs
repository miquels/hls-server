use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{header::HeaderMap, method::Method, uri::Uri, StatusCode},
    response::Response,
};
use std::sync::Arc;

use crate::AppState;

use crate::types::{PlaybackInfoRequest, PlaybackInfoResponse};

// TODO:
// - playback_info_handler should decode the POST request into PlaybackInfoRequest.
// - it should then POST this to the remote jellyfin server.
//
// - The response it gets should be decoded into PlaybackInfoResponse
// - This PlaybackInfoResponse should then be sent back to the client.
//
// This might need splitting up playback_info_handler in two, one that handles
// POST and one that handles GET requests.
//
// Then, mutate_playback_info_request and mutate_playback_info_response should
// mutate the PlaybackInfoRequest data and the PlaybackInfoResponse data
// instead of mangling JSON as they do now.
//
pub async fn playback_info_handler(
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
