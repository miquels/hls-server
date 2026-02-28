//! Jellyfin API types for playback information.

use serde::{Deserialize, Serialize};

/// Media type for direct play profiles.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum MediaType {
    Video,
    Audio,
    Photo,
}

/// Direct play profile describing what the client can play.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DirectPlayProfile {
    /// Container formats (comma-separated string or array).
    #[serde(default)]
    pub container: Option<String>,
    /// Media type.
    #[serde(default)]
    pub r#type: Option<MediaType>,
    /// Video codecs (comma-separated).
    #[serde(default)]
    pub video_codec: Option<String>,
    /// Audio codecs (comma-separated).
    #[serde(default)]
    pub audio_codec: Option<String>,
}

/// Transcoding profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TranscodingProfile {
    /// Container for transcoded output.
    #[serde(default)]
    pub container: Option<String>,
    /// Media type.
    #[serde(default)]
    pub r#type: Option<MediaType>,
    /// Video codec for transcoding.
    #[serde(default)]
    pub video_codec: Option<String>,
    /// Audio codec for transcoding.
    #[serde(default)]
    pub audio_codec: Option<String>,
    /// Transcoding protocol.
    #[serde(default)]
    pub protocol: Option<String>,
}

/// Subtitle profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SubtitleProfile {
    /// Subtitle format.
    #[serde(default)]
    pub format: Option<String>,
    /// Subtitle method (External, Embedded, Hls, etc.).
    #[serde(default)]
    pub method: Option<String>,
}

/// Device information sent to Jellyfin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DeviceProfile {
    /// Direct play profiles.
    #[serde(default)]
    pub direct_play_profiles: Vec<DirectPlayProfile>,
    /// Transcoding profiles.
    #[serde(default)]
    pub transcoding_profiles: Vec<TranscodingProfile>,
    /// Subtitle profiles.
    #[serde(default)]
    pub subtitle_profiles: Vec<SubtitleProfile>,
}

impl Default for DeviceProfile {
    fn default() -> Self {
        Self {
            direct_play_profiles: vec![DirectPlayProfile {
                container: Some("mp4,m4v,mkv,webm".to_string()),
                r#type: Some(MediaType::Video),
                video_codec: Some("h264,h265,hevc,vp9".to_string()),
                audio_codec: Some("aac,mp3,ac3,eac3,opus".to_string()),
            }],
            transcoding_profiles: vec![],
            subtitle_profiles: vec![],
        }
    }
}

/// Playback info request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PlaybackInfoRequest {
    /// User ID.
    #[serde(default)]
    pub user_id: Option<String>,
    /// Device profile describing client capabilities.
    #[serde(default)]
    pub device_profile: Option<DeviceProfile>,
    // Additional fields may be present but we don't need to specify them.
}

/// Media stream type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum MediaStreamType {
    Audio,
    Video,
    Subtitle,
    EmbeddedImage,
}

/// Media stream information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MediaStream {
    /// Stream index.
    #[serde(default)]
    pub index: Option<i32>,
    /// Stream type.
    #[serde(default)]
    pub r#type: Option<MediaStreamType>,
    /// Codec name.
    #[serde(default)]
    pub codec: Option<String>,
    /// Language.
    #[serde(default)]
    pub language: Option<String>,
    /// Display title.
    #[serde(default)]
    pub display_title: Option<String>,
    /// Whether this is the default stream.
    #[serde(default)]
    pub is_default: Option<bool>,
    /// Whether this is an external stream.
    #[serde(default)]
    pub is_external: Option<bool>,
    /// Bit rate.
    #[serde(default)]
    pub bit_rate: Option<i64>,
    /// Width (for video).
    #[serde(default)]
    pub width: Option<i32>,
    /// Height (for video).
    #[serde(default)]
    pub height: Option<i32>,
}

/// Media source information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MediaSourceInfo {
    /// Path to the media file on the server.
    #[serde(default)]
    pub path: Option<String>,
    /// Container format.
    #[serde(default)]
    pub container: Option<String>,
    /// Media streams.
    #[serde(default)]
    pub media_streams: Vec<MediaStream>,
    /// Direct URL to the media.
    #[serde(default)]
    pub direct_stream_url: Option<String>,
    /// Direct play URL.
    #[serde(default)]
    pub direct_play_url: Option<String>,
    /// Transcoding URL.
    #[serde(default)]
    pub transcoding_url: Option<String>,
    /// Whether direct play is supported.
    #[serde(default)]
    pub supports_direct_play: Option<bool>,
    /// Whether direct stream is supported.
    #[serde(default)]
    pub supports_direct_stream: Option<bool>,
}

/// Transcoding information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TranscodingInfo {
    /// Whether audio is being transcoded.
    #[serde(default)]
    pub is_audio_direct: Option<bool>,
    /// Whether video is being transcoded.
    #[serde(default)]
    pub is_video_direct: Option<bool>,
    /// Container being transcoded to.
    #[serde(default)]
    pub container: Option<String>,
}

/// Playback info response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PlaybackInfoResponse {
    /// Media sources.
    #[serde(default)]
    pub media_sources: Vec<MediaSourceInfo>,
    /// Whether direct play is supported.
    #[serde(default)]
    pub play_session_id: Option<String>,
    /// Transcoding info (if transcoding).
    #[serde(default)]
    pub transcoding_info: Option<TranscodingInfo>,
}
