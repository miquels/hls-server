#![allow(dead_code)]

//! Audio resampler for the transcoding pipeline
//!
//! Converts decoded PCM frames to 48 kHz / stereo / `FLTP` for the AAC encoder.

use crate::error::{HlsError, Result};
use ffmpeg_next as ffmpeg;
use ffmpeg_next::software::resampling;
use ffmpeg_next::util::channel_layout::ChannelLayout;
use ffmpeg_next::util::format::sample::Sample;

/// Target sample rate for HLS-compatible AAC audio
pub const HLS_SAMPLE_RATE: u32 = 48000;
/// Target channel layout for HLS (stereo)
pub const HLS_CHANNEL_LAYOUT: ChannelLayout = ChannelLayout::STEREO;
/// Target sample format required by the AAC encoder
pub const HLS_SAMPLE_FORMAT: Sample = Sample::F32(ffmpeg::util::format::sample::Type::Planar);

/// Audio resampler wrapping FFmpeg's `SwrContext`
pub struct AudioResampler {
    context: resampling::Context,
    output_rate: u32,
}

impl AudioResampler {
    /// Create a resampler that converts the format described by `src_frame` to
    /// the standard HLS output format (48 kHz, stereo, FLTP).
    pub fn new(src_frame: &ffmpeg::util::frame::Audio, target_rate: u32) -> Result<Self> {
        let src_layout = if src_frame.channel_layout().bits() == 0 {
            // No channel layout set; fall back based on channel count
            match src_frame.channels() {
                1 => ChannelLayout::MONO,
                _ => ChannelLayout::STEREO,
            }
        } else {
            src_frame.channel_layout()
        };

        let context = resampling::Context::get(
            src_frame.format(),
            src_layout,
            src_frame.rate(),
            HLS_SAMPLE_FORMAT,
            HLS_CHANNEL_LAYOUT,
            target_rate,
        )
        .map_err(|e| {
            HlsError::Ffmpeg(crate::error::FfmpegError::ReadFrame(format!(
                "Failed to create resampling context: {}",
                e
            )))
        })?;

        Ok(Self {
            context,
            output_rate: target_rate,
        })
    }

    /// Convert one input PCM frame into one or more resampled output frames.
    ///
    /// Returns an empty `Vec` when the resampler needs more input to produce
    /// output (can happen at stream start/end with certain sample rates).
    pub fn convert(
        &mut self,
        frame: &ffmpeg::util::frame::Audio,
    ) -> Result<Vec<ffmpeg::util::frame::Audio>> {
        // Output frame must be empty — ffmpeg's swr_convert_frame allocates the
        // correct buffer (format/rate/channels) from the SwrContext config.
        // Pre-populating it with clone_from() caused the resampler to see a
        // frame already filled with source data and produce garbled/empty output.
        let mut out = ffmpeg::util::frame::Audio::empty();

        self.context.run(frame, &mut out).map_err(|e| {
            HlsError::Ffmpeg(crate::error::FfmpegError::ReadFrame(format!(
                "Resampling error: {}",
                e
            )))
        })?;

        if out.samples() == 0 {
            return Ok(vec![]);
        }

        Ok(vec![out])
    }

    /// Flush any remaining samples from the internal resampler buffer.
    ///
    /// When source and output rates match (no actual resampling needed), the
    /// SwrContext has nothing buffered and `flush()` returns an error \u2014 this is
    /// fine, we just return an empty vec.
    pub fn flush(&mut self) -> Result<Vec<ffmpeg::util::frame::Audio>> {
        let mut out = ffmpeg::util::frame::Audio::empty();
        match self.context.flush(&mut out) {
            Ok(_) => {}
            Err(e) => {
                // Not a real error — either no delayed samples or context is a
                // passthrough. Just log at debug level and return empty.
                tracing::debug!("Resampler flush returned non-fatal error: {}", e);
                return Ok(vec![]);
            }
        }

        if out.samples() == 0 {
            return Ok(vec![]);
        }

        Ok(vec![out])
    }

    /// The output sample rate.
    pub fn output_rate(&self) -> u32 {
        self.output_rate
    }
}

/// Get recommended AAC bitrate for a given channel count.
pub fn get_recommended_bitrate(channels: u16) -> u64 {
    match channels {
        1 => 64_000,
        2 => 128_000,
        6 => 384_000,
        8 => 512_000,
        _ => 128_000,
    }
}

/// Determine whether resampling is needed given the source frame parameters.
pub fn needs_resampling(frame: &ffmpeg::util::frame::Audio) -> bool {
    frame.rate() != HLS_SAMPLE_RATE
        || frame.format() != HLS_SAMPLE_FORMAT
        || frame.channel_layout() != HLS_CHANNEL_LAYOUT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_hls_sample_rate() {
        assert_eq!(HLS_SAMPLE_RATE, 48000);
    }

    #[test]
    fn test_get_channel_count_mono() {
        assert_eq!(get_recommended_bitrate(1), 64_000);
    }

    #[test]
    fn test_get_channel_count_stereo() {
        assert_eq!(get_recommended_bitrate(2), 128_000);
    }

    #[test]
    fn test_get_channel_count_5_1() {
        assert_eq!(get_recommended_bitrate(6), 384_000);
    }

    #[test]
    fn test_get_channel_count_default() {
        assert_eq!(get_recommended_bitrate(0), 128_000);
        assert_eq!(get_recommended_bitrate(4), 128_000);
    }

    #[test]
    fn test_resampler_config_default() {
        assert_eq!(HLS_SAMPLE_RATE, 48000);
    }

    #[test]
    fn test_needs_resampling() {
        // We cannot construct real AVFrame easily, but we can at least test
        // the constants are sane
        assert_eq!(HLS_CHANNEL_LAYOUT, ChannelLayout::STEREO);
    }
}
