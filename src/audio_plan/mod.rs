//! Audio track planning module
//!
//! This module handles intelligent audio track handling:
//! - Audio codec capability detection (AAC, AC-3, E-AC-3, Opus, etc.)
//! - Transcode requirement logic
//! - Audio variant planning for HLS manifests
//! - Language track grouping

pub mod planner;

pub use planner::plan_audio_tracks;
