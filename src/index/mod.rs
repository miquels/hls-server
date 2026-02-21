//! Stream indexing module
//!
//! This module handles extraction of metadata from media files:
//! - Video stream detection (codec, resolution, keyframes)
//! - Audio stream detection (codec, sample rate, channels, language)
//! - Subtitle stream detection (codec, language, format)
//! - Segment boundary calculation (keyframe-based)

pub mod scanner;
pub mod video;
pub mod audio;
pub mod subtitle;

pub use video::analyze_video_stream;
pub use audio::analyze_audio_stream;
pub use subtitle::analyze_subtitle_stream;
