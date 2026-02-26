//! # HLS VOD Library
//!
//! `hls-vod-lib` is a library for generating HTTP Live Streaming (HLS) playlists and segments
//! on-the-fly from local video files. It leverages FFmpeg (via `ffmpeg-next`) to demux,
//! optionally transcode, and mux media content into fragmented MP4 (fMP4) or WebVTT segments
//! suitable for HLS playback.
//!
//! ## Core Features
//!
//! - **On-the-fly Packaging:** Muxes existing compatible video (e.g., H.264) directly into fMP4 without transcoding.
//! - **Audio Transcoding:** Transcodes unsupported audio formats (like AC-3 or Opus) to AAC on-the-fly.
//! - **Multiple Tracks:** Supports multiple audio and subtitle tracks, accurately multiplexing them into HLS variant playlists.
//! - **Subtitle Support:** Extracts and serves subtitles (e.g., SubRip) as WebVTT segments.
//!
//! ## Usage
//!
//! 1. **Initialization:** Call `init()` and `install_log_filter()` at startup. `init_segment_cache()` too
//!    if you want to enable the segment cache.
//! 2. **Parsing:** Use `MediaInfo::open` to scan a media file and return an `<Arc<MediaInfo>` struct.
//!    The information is cached, it's cheap to call after the first tume.
//! 3. **Playlists:**
//!    - Generate a master playlist with `MediaInfo::generate_main_playlist`.
//!    - Generate variant playlists (video/audio/subtitle) with `MediaInfo::generate_track_playlist`.
//! 4. **Segments:** Generate actual media segments (fMP4 or WebVTT) handling specific sequence requests with `MediaInfo::generate_segment`.

pub(crate) mod api;
pub(crate) mod error;
pub(crate) mod ffmpeg_utils;
pub(crate) mod index;
pub(crate) mod playlist;
pub(crate) mod segment;
pub(crate) mod subtitle;
pub(crate) mod transcode;
pub(crate) mod types;
pub mod url;

#[cfg(test)]
pub(crate) mod tests;

pub use api::*;
pub use error::{FfmpegError, HlsError, Result};
pub use ffmpeg_utils::version_info as ffmpeg_version_info;
pub use ffmpeg_utils::{init, install_log_filter};
pub use url::*;
