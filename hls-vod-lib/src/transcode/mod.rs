//! Audio transcoding module
//!
//! This module handles audio transcoding for HLS compatibility:
//! - Audio decoder initialization from source streams
//! - Audio resampling to 48kHz (HLS standard)
//! - AAC encoder initialization
//! - Standalone audio transcoding pipeline (independent tracks)
//! - In-memory encoded packet buffering

pub mod decoder;
pub mod encoder;
pub mod resampler;
pub mod pipeline;
