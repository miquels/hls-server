//! Stream indexing module
//!
//! This module handles extraction of metadata from media files:
//! - Video stream detection (codec, resolution, keyframes)
//! - Audio stream detection (codec, sample rate, channels, language)
//! - Subtitle stream detection (codec, language, format)
//! - Segment boundary calculation (keyframe-based)

pub mod audio;
pub mod scanner;
pub mod subtitle;
pub mod video;

pub use audio::analyze_audio_stream;
pub use subtitle::analyze_subtitle_stream;
pub use video::analyze_video_stream;
