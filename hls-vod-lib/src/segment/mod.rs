//! Segment generation module
//!
//! This module handles fMP4/CMAF segment generation using FFmpeg CLI.

pub mod muxer;
pub mod generator;

pub use generator::{
    generate_init_segment,
    generate_video_segment,
    generate_audio_segment,
    generate_subtitle_segment,
    generate_video_init_segment,
    generate_audio_init_segment,
};
