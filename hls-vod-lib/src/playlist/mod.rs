//! Playlist generation module
//!
//! This module handles HLS playlist generation:
//! - Master playlist (master.m3u8) with all variants
//! - Video variant playlist (video.m3u8)
//! - Audio variant playlists (audio_*.m3u8)
//! - Subtitle variant playlists (sub_*.m3u8)
//! - Proper HLS tags and codec strings

pub mod master;
pub mod variant;
pub mod codec;

pub use master::generate_master_playlist;
pub use variant::{generate_video_playlist, generate_audio_playlist, generate_subtitle_playlist};
