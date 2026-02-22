//! FFmpeg context wrappers for input/output handling

use crate::error::{FfmpegError, Result};
use ffmpeg_next as ffmpeg;
use ffmpeg_next::format::input;
use std::path::Path;

/// Wrapper for FFmpeg input context
pub struct InputContext {
    inner: ffmpeg::format::context::Input,
    source_path: std::path::PathBuf,
}

impl InputContext {
    /// Open a media file for reading
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let inner = input(&path).map_err(|e| {
            FfmpegError::OpenInput(format!("Failed to open {:?}: {}", path, e))
        })?;

        tracing::debug!("Opened input file: {:?}", path);

        Ok(Self {
            inner,
            source_path: path.to_path_buf(),
        })
    }

    /// Get the source file path
    pub fn source_path(&self) -> &std::path::Path {
        &self.source_path
    }

    /// Get the duration of the media in seconds
    pub fn duration(&self) -> f64 {
        self.inner.duration() as f64 / ffmpeg::ffi::AV_TIME_BASE as f64
    }

    /// Get the bitrate of the media
    pub fn bitrate(&self) -> u64 {
        self.inner.bit_rate() as u64
    }

    /// Get the number of streams
    pub fn num_streams(&self) -> usize {
        self.inner.streams().len()
    }

    /// Get a stream by index
    pub fn stream(&self, index: usize) -> Option<ffmpeg::Stream<'_>> {
        self.inner.streams().into_iter().nth(index)
    }

    /// Get the inner context for direct access
    pub fn inner(&self) -> &ffmpeg::format::context::Input {
        &self.inner
    }

    /// Iterate over all streams
    pub fn streams(&self) -> impl Iterator<Item = ffmpeg::Stream<'_>> + '_ {
        self.inner.streams().into_iter()
    }

    /// Find the best video stream
    pub fn best_video_stream(&self) -> Option<usize> {
        self.inner
            .streams()
            .best(ffmpeg::media::Type::Video)
            .map(|s| s.index())
    }

    /// Find the best audio stream
    pub fn best_audio_stream(&self) -> Option<usize> {
        self.inner
            .streams()
            .best(ffmpeg::media::Type::Audio)
            .map(|s| s.index())
    }
}
