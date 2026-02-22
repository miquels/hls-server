//! FFmpeg module - provides wrappers and utilities for FFmpeg library access
//!
//! This module handles:
//! - FFmpeg initialization
//! - Input/output context management
//! - Custom AVIOContext for in-memory writing
//! - Timebase conversion and other utilities

pub mod context;
pub mod index;
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

/// Install a custom FFmpeg log callback that suppresses known-noisy messages
/// which are expected side-effects of our deliberate muxer configuration
/// (no delay_moov, empty_moov, early packet-loop termination).
///
/// Must be called after `ffmpeg::init()` and after `av_log_set_level`.
pub fn install_log_filter() {
    unsafe {
        ffmpeg_next::ffi::av_log_set_callback(Some(ffmpeg_log_callback));
    }
}

/// Messages that are expected side-effects of our muxer design and should be suppressed.
const SUPPRESSED_MESSAGES: &[&str] = &[
    "No meaningful edit list will be written when using empty_moov without delay_moov",
    "starts with a nonzero dts",
    "Set the delay_moov flag to handle this case",
];

unsafe extern "C" fn ffmpeg_log_callback(
    avcl: *mut std::ffi::c_void,
    level: std::ffi::c_int,
    fmt: *const std::ffi::c_char,
    vl: *mut ffmpeg_next::ffi::__va_list_tag,
) {
    use std::ffi::CStr;

    // Respect the configured log level
    if level > unsafe { ffmpeg_next::ffi::av_log_get_level() } {
        return;
    }

    // Format the message using FFmpeg's own vsnprintf helper
    let mut buf = [0i8; 1024];
    let mut print_prefix: std::ffi::c_int = 1;
    ffmpeg_next::ffi::av_log_format_line(
        avcl,
        level,
        fmt,
        vl,
        buf.as_mut_ptr(),
        buf.len() as std::ffi::c_int,
        &mut print_prefix,
    );

    let msg = CStr::from_ptr(buf.as_ptr()).to_string_lossy();

    // Drop messages that are known, benign side-effects of our design
    for suppressed in SUPPRESSED_MESSAGES {
        if msg.contains(suppressed) {
            return;
        }
    }

    eprint!("{}", msg);
}

/// Get FFmpeg version information
pub fn version_info() -> String {
    // Return a simple version string since the API changed in FFmpeg 8.0
    "FFmpeg 8.0+".to_string()
}
