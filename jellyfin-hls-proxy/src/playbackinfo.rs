use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{header::HeaderMap, method::Method, uri::Uri, StatusCode},
    response::Response,
};
use std::sync::Arc;

use crate::AppState;

use crate::types::{PlaybackInfoRequest, PlaybackInfoResponse};

pub async fn playback_info_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(_item_id): axum::extract::Path<String>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, StatusCode> {
    tracing::info!("PlaybackInfo request received: {} {}", method, uri.path());
    // 1. Decode request
    let mut req_data: PlaybackInfoRequest = if body.is_empty() {
        PlaybackInfoRequest::default()
    } else {
        serde_json::from_slice(&body).unwrap_or_else(|e| {
            tracing::warn!(
                "Failed to decode PlaybackInfo request: {}, using default",
                e
            );
            PlaybackInfoRequest::default()
        })
    };

    let user_agent = headers
        .get(reqwest::header::USER_AGENT)
        .and_then(|h| h.to_str().ok());

    // 2. Mutate request
    mutate_playback_info_request(&mut req_data, user_agent);

    let modified_body = serde_json::to_vec(&req_data).unwrap();

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

        // 3. Decode response
        let mut resp_data: PlaybackInfoResponse = serde_json::from_slice(&resp_body_bytes)
            .unwrap_or_else(|e| {
                tracing::warn!(
                    "Failed to decode PlaybackInfo response: {}, returning default",
                    e
                );
                PlaybackInfoResponse::default()
            });

        // 4. Mutate response
        mutate_playback_info_response(&mut resp_data);

        let modified_resp_body = serde_json::to_vec(&resp_data).unwrap();

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

fn mutate_playback_info_request(req: &mut PlaybackInfoRequest, user_agent: Option<&str>) {
    let dp_profile = crate::types::DirectPlayProfile {
        container: Some("mp4,m4v,mkv,webm".to_string()),
        video_codec: Some("h264,h265,vp9".to_string()),
        audio_codec: Some("aac,mp3,ac3,eac3,opus".to_string()),
        profile_type: "Video".to_string(),
    };

    let is_safari = user_agent.map_or(false, |ua| {
        ua.contains("Safari") && !ua.contains("Chrome") && !ua.contains("Chromium")
    });

    if let Some(device_profile) = req.device_profile.as_mut() {
        // Remove 'ts' transcoding support from TranscodingProfiles
        device_profile
            .transcoding_profiles
            .retain(|p| p.container.as_deref() != Some("ts"));

        if is_safari {
            for dp in &mut device_profile.direct_play_profiles {
                if let Some(codecs) = dp.video_codec.as_mut() {
                    if codecs.contains("h264") && !codecs.contains("h265") {
                        codecs.push_str(",h265");
                    }
                }
            }
        } else {
            device_profile.direct_play_profiles = vec![dp_profile];
        }
    } else {
        req.device_profile = Some(crate::types::DeviceProfile {
            direct_play_profiles: vec![dp_profile],
            transcoding_profiles: vec![],
            ..Default::default()
        });
    }
}

fn mutate_playback_info_response(resp: &mut PlaybackInfoResponse) {
    for source in resp.media_sources.iter_mut() {
        let clean_path = source.path.trim_start_matches('/');
        let mut base_transcode_url =
            format!("/proxymedia/{}.as.m3u8", urlencoding::encode(clean_path));

        if let Some(orig_url_str) = &source.transcoding_url {
            // Some Jellyfin URLs might be relative. We'll prepend a dummy base so we can parse them.
            let full_url_str = if orig_url_str.starts_with('/') {
                format!("http://localhost{}", orig_url_str)
            } else {
                orig_url_str.clone()
            };

            if let Ok(parsed_url) = url::Url::parse(&full_url_str) {
                let query_str = parsed_url.query().unwrap_or("");
                if let Ok(params) =
                    serde_urlencoded::from_str::<crate::types::HlsTranscodingParameters>(query_str)
                {
                    let mut proxy_query = Vec::new();

                    let mut codecs = Vec::new();
                    if let Some(vc) = &params.video_codec {
                        codecs.push(vc.clone());
                    }
                    if let Some(ac) = &params.audio_codec {
                        codecs.push(ac.clone());
                    }
                    if !codecs.is_empty() {
                        proxy_query.push(format!("codecs={}", codecs.join(",")));
                    }

                    if let Some(audio_idx) = params.audio_stream_index {
                        proxy_query.push(format!("tracks={}", audio_idx));
                    }

                    if let Some(session_id) = &params.play_session_id {
                        proxy_query.push(format!("stream_id={}", urlencoding::encode(session_id)));
                    }

                    if !proxy_query.is_empty() {
                        base_transcode_url =
                            format!("{}?{}", base_transcode_url, proxy_query.join("&"));
                    }
                }
            }
        }

        source.transcoding_url = Some(base_transcode_url);
        source.transcoding_sub_protocol = Some("hls".to_string());
        source.transcoding_container = Some("m4s".to_string());

        source.supports_direct_play = false;
        source.supports_direct_stream = false;
        source.supports_transcoding = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mutate_playback_info_request_ts_filtered() {
        let mut req = PlaybackInfoRequest {
            device_profile: Some(crate::types::DeviceProfile {
                transcoding_profiles: vec![
                    crate::types::TranscodingProfile {
                        container: Some("mp3".to_string()),
                        profile_type: "Audio".to_string(),
                        video_codec: None,
                        audio_codec: Some("mp3".to_string()),
                        protocol: "http".to_string(),
                        context: "Streaming".to_string(),
                    },
                    crate::types::TranscodingProfile {
                        container: Some("ts".to_string()),
                        profile_type: "Video".to_string(),
                        video_codec: None,
                        audio_codec: None,
                        protocol: "hls".to_string(),
                        context: "Streaming".to_string(),
                    },
                ],
                direct_play_profiles: vec![],
                ..Default::default()
            }),
            ..Default::default()
        };
        mutate_playback_info_request(&mut req, None);
        let device_profile = req.device_profile.as_ref().unwrap();
        assert_eq!(device_profile.transcoding_profiles.len(), 1);
        assert_eq!(
            device_profile.transcoding_profiles[0].container.as_deref(),
            Some("mp3")
        );
        assert_eq!(device_profile.direct_play_profiles.len(), 1);
        let dp = &device_profile.direct_play_profiles[0];
        assert_eq!(dp.video_codec.as_deref(), Some("h264,h265,vp9"));
    }

    #[test]
    fn test_mutate_playback_info_request_safari() {
        let mut req = PlaybackInfoRequest {
            device_profile: Some(crate::types::DeviceProfile {
                transcoding_profiles: vec![],
                direct_play_profiles: vec![crate::types::DirectPlayProfile {
                    container: Some("mp4".to_string()),
                    video_codec: Some("h264".to_string()),
                    audio_codec: Some("aac".to_string()),
                    profile_type: "Video".to_string(),
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        mutate_playback_info_request(&mut req, Some("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Safari/605.1.15"));
        let device_profile = req.device_profile.as_ref().unwrap();
        assert_eq!(device_profile.direct_play_profiles.len(), 1);
        let dp = &device_profile.direct_play_profiles[0];
        // Ensure h265 was appended
        assert_eq!(dp.video_codec.as_deref(), Some("h264,h265"));
    }

    #[test]
    fn test_mutate_playback_info_response() {
        let mut resp = PlaybackInfoResponse {
            media_sources: vec![crate::types::MediaSource {
                path: "/some/media/file.mp4".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        };
        mutate_playback_info_response(&mut resp);
        let media_source = &resp.media_sources[0];
        assert_eq!(media_source.supports_direct_play, false);
        assert_eq!(media_source.supports_transcoding, true);
        assert_eq!(
            media_source.transcoding_url.as_deref(),
            Some("/proxymedia/some%2Fmedia%2Ffile.mp4.as.m3u8")
        );
    }

    #[test]
    fn test_mutate_playback_info_response_with_params() {
        let mut resp = PlaybackInfoResponse {
            media_sources: vec![crate::types::MediaSource {
                path: "/movie.mkv".to_string(),
                transcoding_url: Some("/videos/123/master.m3u8?Id=test&MediaSourceId=test&VideoCodec=h264&AudioCodec=aac&AudioStreamIndex=1&PlaySessionId=abcdef123".to_string()),
                ..Default::default()
            }],
            ..Default::default()
        };
        mutate_playback_info_response(&mut resp);
        let media_source = &resp.media_sources[0];
        assert_eq!(
            media_source.transcoding_url.as_deref(),
            Some("/proxymedia/movie.mkv.as.m3u8?codecs=h264,aac&tracks=1&stream_id=abcdef123")
        );
    }
}
