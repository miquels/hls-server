//! FFmpeg module - provides wrappers and utilities for FFmpeg library access
//!
//! This module handles:
//! - FFmpeg initialization
//! - Input/output context management
//! - Custom AVIOContext for in-memory writing
//! - Timebase conversion and other utilities

pub mod context;
pub mod io;
pub mod utils;

#[allow(unused_imports)]
pub use utils::*;
pub use ffmpeg_next as ffmpeg;

/// Initialize FFmpeg library
///
/// This should be called once at application startup.
/// Returns an error if FFmpeg fails to initialize.
pub fn init() -> Result<(), crate::error::FfmpegError> {
    ffmpeg::init().map_err(|e| {
        crate::error::FfmpegError::InitFailed(format!("ffmpeg::init() failed: {}", e))
    })?;
    
    tracing::info!("FFmpeg initialized");
    
    Ok(())
}

/// Get FFmpeg version information
pub fn version_info() -> String {
    // Return a simple version string since the API changed in FFmpeg 8.0
    "FFmpeg 8.0+".to_string()
}
