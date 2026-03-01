use serde::{Deserialize, Serialize};
use std::collections::HashMap;

//
// First, the PlaybackInfoRequest sent to the PlaybackInfo endpoint.
//
#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub struct PlaybackInfoRequest {
    /// The specific device ID making the request
    pub id: Option<String>,
    /// The ID of the user requesting playback
    pub user_id: Option<String>,
    /// The index of the audio stream to play
    pub audio_stream_index: Option<i32>,
    /// The index of the subtitle stream to play
    pub subtitle_stream_index: Option<i32>,
    /// The preferred media source ID
    pub media_source_id: Option<String>,
    /// Max bitrate the client can handle
    pub max_streaming_bitrate: Option<i64>,
    /// Starting position in Ticks (1 second = 10,000,000 ticks)
    pub start_time_ticks: Option<i64>,
    /// The hardware/software capabilities of the client
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_profile: Option<DeviceProfile>,
    /// Whether to enable direct play
    pub enable_direct_play: Option<bool>,
    /// Whether to enable transcoding
    pub enable_transcoding: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub struct DeviceProfile {
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub direct_play_profiles: Vec<DirectPlayProfile>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub transcoding_profiles: Vec<TranscodingProfile>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub subtitle_profiles: Vec<SubtitleProfile>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct DirectPlayProfile {
    pub container: Option<String>,
    pub audio_codec: Option<String>,
    pub video_codec: Option<String>,
    #[serde(rename = "Type")]
    pub profile_type: String, // e.g., "Video"
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TranscodingProfile {
    pub container: Option<String>,
    #[serde(rename = "Type")]
    pub profile_type: String,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub protocol: String, // e.g., "hls"
    pub context: String,  // e.g., "Streaming"
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SubtitleProfile {
    pub format: String, // e.g., "srt", "vtt"
    pub method: String, // e.g., "External", "Hls", "Embed"
}

//
// Then the response sent by the jellyfin server.
//
#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub struct PlaybackInfoResponse {
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub media_sources: Vec<MediaSource>,
    pub play_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub struct MediaSource {
    /// The transport protocol used to access the file (e.g., "File", "Http", "Rtmp").
    pub protocol: String,
    /// A unique identifier for this specific media source.
    pub id: String,
    /// The physical or virtual path to the file on the server.
    pub path: String,
    /// Path to the specific encoder binary if using external tools.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoder_path: Option<String>,
    /// Protocol used by the encoder (usually "Http" for streaming).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoder_protocol: Option<String>,
    /// The type of source (usually "Default" or "Grouping").
    #[serde(rename = "Type")]
    pub r#type: String,
    /// The file container format (e.g., "mkv", "mp4", "m4s").
    pub container: String,
    /// Total file size in bytes.
    pub size: i64,
    /// Human-readable name for the source.
    pub name: String,
    /// Whether the file is located on a remote network/cloud.
    pub is_remote: bool,
    /// Total duration of the media in Ticks (1 sec = 10,000,000 ticks).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_time_ticks: Option<i64>,
    /// Tells the client if the server is capable of transcoding this file.
    pub supports_transcoding: bool,
    /// Whether the server can "remux" (change container only) without re-encoding.
    pub supports_direct_stream: bool,
    /// Whether the client can play the raw file via HTTP without server help.
    pub supports_direct_play: bool,
    /// Used for live streams that do not have a defined end.
    pub is_infinite_stream: bool,
    /// Whether the media requires a "LiveStream" open request before playback.
    pub requires_opening: bool,
    /// Token used to maintain an open session for protected/live streams.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_token: Option<String>,
    /// Whether the server needs a "Close" signal when the user stops watching.
    pub requires_closing: bool,
    /// ID associated with a persistent live stream session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_stream_id: Option<String>,
    /// Suggested buffer size for the client in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buffer_ms: Option<i32>,
    /// Whether the player should loop back to the start automatically.
    pub requires_looping: bool,
    /// Whether the stream can be passed to an external player (like VLC/MPV).
    pub supports_external_stream: bool,
    /// The list of video, audio, and subtitle tracks found in the file.
    pub media_streams: Vec<MediaStream>,
    /// List of compatible containers for this source.
    pub formats: Vec<String>,
    /// Total combined bitrate (video + audio) in bits per second.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bitrate: Option<i32>,
    /// Last modified or creation timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    /// Custom headers required by the client to fetch segments (e.g., Cookies/Auth).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_http_headers: Option<HashMap<String, String>>,
    /// The critical HLS/DASH URL used for transcoding sessions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcoding_url: Option<String>,
    /// The sub-protocol for transcoding (usually "hls").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcoding_sub_protocol: Option<String>,
    /// The container used for transcode segments (e.g., "ts" or "m4s").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcoding_container: Option<String>,
    /// How many ms the client should analyze the stream before playing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analyze_duration_ms: Option<i32>,
    /// The index of the audio track the server recommends playing by default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_audio_stream_index: Option<i32>,
    /// The index of the subtitle track the server recommends playing by default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_subtitle_stream_index: Option<i32>,
    /// Categorization of video (e.g., "Video", "Map", "Thumbnail").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_type: Option<String>,
    /// Unique hash to help with client-side caching.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    /// URL for direct stream access (if different from Path).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direct_stream_url: Option<String>,
    /// List of extra files (like fonts or posters) associated with the source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_attachments: Option<Vec<String>>,
    /// Whether the server should force reading at the original FPS (common for Live).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_at_native_framerate: Option<bool>,
    /// Whether the media is pre-segmented (DASH/HLS) on the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_segments: Option<bool>,
    /// Tells the client to ignore DTS timestamps (fixes some sync issues).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignore_dts: Option<bool>,
    /// Tells the client to ignore the file index and scan sequentially.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ignore_index: Option<bool>,
    /// Tells FFmpeg to generate PTS timestamps on the fly.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gen_pts_input: Option<bool>,
    /// Whether the server has already "probed" the file for metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supports_probing: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct MediaStream {
    /// The codec name (e.g., "h264", "aac", "subrip").
    pub codec: String,
    /// 3-letter ISO language code (e.g., "eng", "fra").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// The internal time base for the stream (e.g., "1/90000").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_base: Option<String>,
    /// The title attribute from the stream metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The user-friendly title shown in the client UI (e.g., "English (AAC 5.1)").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_title: Option<String>,
    /// The localized name of the language (e.g., "Spanish").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_language: Option<String>,
    /// Whether the video is interlaced (as opposed to progressive).
    pub is_interlaced: bool,
    /// The audio channel layout (e.g., "5.1", "stereo", "7.1").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_layout: Option<String>,
    /// Bitrate of this specific track in bits per second.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bit_rate: Option<i32>,
    /// Color depth of the video (e.g., 8, 10 for HDR).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bit_depth: Option<i32>,
    /// Number of reference frames (relevant for H.264 profiles).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_frames: Option<i32>,
    /// Internal packet length (mostly for specialized transport).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub packet_length: Option<i32>,
    /// Number of audio channels (e.g., 2, 6).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channels: Option<i32>,
    /// Audio sample rate in Hz (e.g., 44100, 48000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<i32>,
    /// Whether this is the default track for its type.
    pub is_default: bool,
    /// Whether this is a "forced" subtitle track.
    pub is_forced: bool,
    /// Video height in pixels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<i32>,
    /// Video width in pixels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<i32>,
    /// The average FPS of the video.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub average_frame_rate: Option<f32>,
    /// The actual/variable frame rate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub real_frame_rate: Option<f32>,
    /// The codec profile (e.g., "High", "Main 10").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// Type of stream: "Video", "Audio", "Subtitle", or "Data".
    #[serde(rename = "Type")]
    pub stream_type: String,
    /// Display aspect ratio (e.g., "16:9").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aspect_ratio: Option<String>,
    /// The absolute global index of this stream in the file (FFmpeg index).
    pub index: i32,
    /// Internal priority score for track selection logic.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<i32>,
    /// Whether this track is an external file (e.g., .srt) or embedded.
    pub is_external: bool,
    /// How the track is delivered: "External", "Hls", "Embed".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_method: Option<String>,
    /// URL to fetch the subtitle or audio file if it is external.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery_url: Option<String>,
    /// Whether the delivery URL points to a different server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_external_url: Option<bool>,
    /// True if the subtitle is text-based (SRT/ASS) vs image-based (PGS).
    pub is_text_subtitle_stream: bool,
    /// Whether this stream supports being served via an external URL.
    pub supports_external_stream: bool,
    /// Path to the external track file on disk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Video pixel format (e.g., "yuv420p", "yuv420p10le").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pixel_format: Option<String>,
    /// Codec level (e.g., 4.1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<f32>,
    /// The internal codec tag (e.g., "avc1", "hvc1").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codec_tag: Option<String>,
    /// Whether the pixels are non-square (common in DVD rips).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_anamorphic: Option<bool>,
    /// The color range (e.g., "SDR", "HDR10", "HLG", "DOVI").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_range: Option<String>,
    /// Specific color space metadata (e.g., "BT709", "BT2020").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_range_type: Option<String>,
    /// Information for Dolby Atmos or DTS:X spatial audio.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_spatial_format: Option<String>,
    /// Localized string for the "Default" label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub localized_default: Option<String>,
    /// Localized string for the "External" label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub localized_external: Option<String>,
    /// True if the video codec is H.264/AVC.
    #[serde(rename = "IsAVC")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_avc: Option<bool>,
    /// Flag for SDH (Subtitles for the Deaf and Hard of Hearing).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_hearing_impaired: Option<bool>,
    /// If the HLS stream interleaves audio and video.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_interleaved: Option<bool>,
}
