#![allow(dead_code)]

//! Audio decoder for the transcoding pipeline
//!
//! Wraps an FFmpeg `AVCodecContext` to decode compressed audio packets
//! (AC-3, Opus, MP3, FLAC, …) into raw PCM `AVFrame`s.

use crate::error::{FfmpegError, HlsError, Result};
use ffmpeg_next as ffmpeg;

/// Real audio decoder backed by a FFmpeg codec context
pub struct AudioDecoder {
    /// The FFmpeg decoder context
    decoder: ffmpeg::decoder::Audio,
    /// Stream index in the source file
    stream_index: usize,
}

impl AudioDecoder {
    /// Open a decoder for the given stream.
    ///
    /// Uses the stream's own codec parameters to initialise the context so no
    /// external configuration is needed.
    pub fn open(stream: &ffmpeg::format::stream::Stream) -> Result<Self> {
        let stream_index = stream.index();
        let context =
            ffmpeg::codec::Context::from_parameters(stream.parameters()).map_err(|e| {
                HlsError::Ffmpeg(FfmpegError::DecoderNotFound(format!(
                    "Failed to create codec context for stream {}: {}",
                    stream_index, e
                )))
            })?;

        let decoder = context.decoder().audio().map_err(|e| {
            HlsError::Ffmpeg(FfmpegError::DecoderNotFound(format!(
                "Failed to open audio decoder for stream {}: {}",
                stream_index, e
            )))
        })?;

        Ok(Self {
            decoder,
            stream_index,
        })
    }

    /// Send a compressed packet to the decoder.
    ///
    /// `AVERROR_INVALIDDATA` is treated as non-fatal and returns `Ok(())` with
    /// a debug log — the Opus decoder emits this during seek pre-roll (the
    /// `[opus] Could not update timestamps for skipped samples` warning).
    pub fn send_packet(&mut self, packet: &ffmpeg::codec::packet::Packet) -> Result<()> {
        match self.decoder.send_packet(packet) {
            Ok(()) => Ok(()),
            // Opus decoder pre-roll: skip this packet and keep going
            Err(ffmpeg::Error::InvalidData) => {
                tracing::debug!(
                    stream_index = self.stream_index,
                    "send_packet: skipping invalid/pre-roll packet"
                );
                Ok(())
            }
            Err(e) => Err(HlsError::Ffmpeg(FfmpegError::ReadFrame(format!(
                "send_packet error on stream {}: {}",
                self.stream_index, e
            )))),
        }
    }

    /// Send EOF to flush the decoder's internal buffers.
    ///
    /// EAGAIN and EOF responses are silently ignored — they mean the decoder
    /// has nothing buffered or is already finished, which is not an error.
    pub fn send_eof(&mut self) -> Result<()> {
        match self.decoder.send_eof() {
            Ok(()) => Ok(()),
            // Not really errors — decoder is already drained or has no buffered data
            Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => Ok(()),
            Err(ffmpeg::Error::Eof) => Ok(()),
            Err(e) => Err(HlsError::Ffmpeg(FfmpegError::ReadFrame(format!(
                "send_eof error on stream {}: {}",
                self.stream_index, e
            )))),
        }
    }

    /// Receive one decoded PCM frame, or `None` if the decoder needs more
    /// input.
    pub fn receive_frame(&mut self) -> Result<Option<ffmpeg::util::frame::Audio>> {
        let mut frame = ffmpeg::util::frame::Audio::empty();
        match self.decoder.receive_frame(&mut frame) {
            Ok(()) => Ok(Some(frame)),
            Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => Ok(None),
            Err(ffmpeg::Error::Eof) => Ok(None),
            Err(e) => Err(HlsError::Ffmpeg(FfmpegError::ReadFrame(format!(
                "receive_frame error on stream {}: {}",
                self.stream_index, e
            )))),
        }
    }

    /// The source stream index.
    pub fn stream_index(&self) -> usize {
        self.stream_index
    }

    /// Sample rate of decoded frames.
    pub fn sample_rate(&self) -> u32 {
        self.decoder.rate()
    }

    /// Channel count of decoded frames.
    pub fn channels(&self) -> u16 {
        self.decoder.channels()
    }

    /// Sample format of decoded frames.
    pub fn format(&self) -> ffmpeg::util::format::sample::Sample {
        self.decoder.format()
    }

    /// Channel layout of decoded frames.
    pub fn channel_layout(&self) -> ffmpeg::util::channel_layout::ChannelLayout {
        self.decoder.channel_layout()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_aac_decoder() {
        let decoder = ffmpeg::codec::decoder::find(ffmpeg::codec::Id::AAC);
        assert!(decoder.is_some());
    }

    #[test]
    fn test_create_ac3_decoder() {
        let decoder = ffmpeg::codec::decoder::find(ffmpeg::codec::Id::AC3);
        assert!(decoder.is_some());
    }

    #[test]
    fn test_create_opus_decoder() {
        let decoder = ffmpeg::codec::decoder::find(ffmpeg::codec::Id::OPUS);
        assert!(decoder.is_some());
    }

    #[test]
    fn test_decoder_codec_id() {
        let decoder = ffmpeg::codec::decoder::find(ffmpeg::codec::Id::AAC);
        assert!(decoder.is_some());
        assert_eq!(decoder.unwrap().id(), ffmpeg::codec::Id::AAC);
    }

    #[test]
    fn test_decoder_configure() {
        // Verify we can query the decoder without panicking
        let decoder = ffmpeg::codec::decoder::find(ffmpeg::codec::Id::AAC);
        assert!(decoder.is_some());
    }
}
