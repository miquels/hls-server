//! Subtitle conversion module
//!
//! This module handles on-the-fly subtitle conversion to WebVTT:
//! - Subtitle packet decoding from source streams
//! - Text extraction from AVSubtitle structs
//! - HTML entity escaping for WebVTT
//! - WebVTT format generation with X-TIMESTAMP-MAP
//! - ASS/SSA style conversion (optional)

pub mod decoder;
pub mod extractor;
pub mod webvtt;
