#![allow(dead_code)]

//! AAC encoder for the transcoding pipeline
//!
//! Wraps an FFmpeg `AVCodecContext` to encode PCM frames (FLTP, 48 kHz,
//! stereo) to AAC-LC packets.

use crate::error::{FfmpegError, HlsError, Result};
use ffmpeg_next as ffmpeg;
use ffmpeg_next::codec;
use ffmpeg_next::util::channel_layout::ChannelLayout;
use ffmpeg_next::util::format::sample::Sample;

/// Target format the encoder expects from the resampler
pub const ENCODER_SAMPLE_FMT: Sample = Sample::F32(ffmpeg::util::format::sample::Type::Planar);
/// AAC encoder frame size (number of samples per channel per frame)
pub const AAC_FRAME_SIZE: usize = 1024;

/// AAC encoder backed by a real FFmpeg codec context
pub struct AacEncoder {
    encoder: ffmpeg::encoder::Audio,
    frame_size: usize,
    output_timebase: ffmpeg::Rational,
    pts: i64,
}

impl AacEncoder {
    /// Open an AAC encoder at the given parameters.
    pub fn open(sample_rate: u32, channels: u16, bitrate: u64) -> Result<Self> {
        let codec = codec::encoder::find(codec::Id::AAC).ok_or_else(|| {
            HlsError::Ffmpeg(FfmpegError::EncoderNotFound(
                "AAC encoder not found in this FFmpeg build".into(),
            ))
        })?;

        let ch_layout = if channels == 1 {
            ChannelLayout::MONO
        } else {
            ChannelLayout::STEREO
        };

        // Build context and configure the audio encoder BEFORE opening
        let mut context = codec::Context::new_with_codec(codec);
        context.set_time_base(ffmpeg::Rational::new(1, sample_rate as i32));

        let mut audio_enc = context.encoder().audio().map_err(|e| {
            HlsError::Ffmpeg(FfmpegError::EncoderNotFound(format!(
                "Cannot get audio encoder handle: {}",
                e
            )))
        })?;

        audio_enc.set_rate(sample_rate as i32);
        audio_enc.set_format(ENCODER_SAMPLE_FMT);
        audio_enc.set_channel_layout(ch_layout);
        audio_enc.set_bit_rate(bitrate as usize);

        let encoder = audio_enc.open_as(codec).map_err(|e| {
            HlsError::Ffmpeg(FfmpegError::EncoderNotFound(format!(
                "Failed to open AAC encoder: {}",
                e
            )))
        })?;

        let frame_size = encoder.frame_size() as usize;
        let output_timebase = ffmpeg::Rational::new(1, sample_rate as i32);

        Ok(Self {
            encoder,
            frame_size: if frame_size == 0 {
                AAC_FRAME_SIZE
            } else {
                frame_size
            },
            output_timebase,
            pts: 0,
        })
    }

    /// Send one PCM frame to the encoder.
    pub fn send_frame(&mut self, frame: &ffmpeg::util::frame::Audio) -> Result<()> {
        self.encoder.send_frame(frame).map_err(|e| {
            HlsError::Ffmpeg(FfmpegError::EncoderNotFound(format!(
                "AAC encoder send_frame error: {}",
                e
            )))
        })
    }

    /// Send EOF to flush the encoder's buffered output.
    pub fn send_eof(&mut self) -> Result<()> {
        self.encoder.send_eof().map_err(|e| {
            HlsError::Ffmpeg(FfmpegError::EncoderNotFound(format!(
                "AAC encoder send_eof error: {}",
                e
            )))
        })
    }

    /// Receive one encoded AAC packet, or `None` if the encoder needs more input.
    pub fn receive_packet(&mut self) -> Result<Option<ffmpeg::codec::packet::Packet>> {
        let mut packet = ffmpeg::codec::packet::Packet::empty();
        match self.encoder.receive_packet(&mut packet) {
            Ok(()) => {
                if packet.pts().is_none() {
                    packet.set_pts(Some(self.pts));
                    packet.set_dts(Some(self.pts));
                }
                self.pts += self.frame_size as i64;
                Ok(Some(packet))
            }
            Err(ffmpeg::Error::Other { errno }) if errno == ffmpeg::error::EAGAIN => Ok(None),
            Err(ffmpeg::Error::Eof) => Ok(None),
            Err(e) => Err(HlsError::Ffmpeg(FfmpegError::EncoderNotFound(format!(
                "AAC encoder receive_packet error: {}",
                e
            )))),
        }
    }

    /// Flush any remaining buffered packets after sending EOF.
    pub fn flush(&mut self) -> Result<Vec<ffmpeg::codec::packet::Packet>> {
        self.send_eof()?;
        let mut packets = Vec::new();
        loop {
            match self.receive_packet()? {
                Some(p) => packets.push(p),
                None => break,
            }
        }
        Ok(packets)
    }

    /// The number of samples per channel the AAC encoder expects per frame.
    pub fn frame_size(&self) -> usize {
        self.frame_size
    }

    /// The output timebase (1 / sample_rate).
    pub fn output_timebase(&self) -> ffmpeg::Rational {
        self.output_timebase
    }

    /// Codec parameters for the encoded stream (for muxer stream setup).
    ///
    /// Built by copying the opened encoder's AVCodecContext back into a fresh
    /// AVCodecParameters struct.
    pub fn codec_parameters(&self) -> ffmpeg::codec::Parameters {
        use std::ops::Deref;
        use std::rc::Rc;
        let ctx: &ffmpeg::codec::Context = self.encoder.deref();
        unsafe {
            let params = ffmpeg::ffi::avcodec_parameters_alloc();
            ffmpeg::ffi::avcodec_parameters_from_context(params, ctx.as_ptr());
            ffmpeg::codec::Parameters::wrap(params, None::<Rc<dyn std::any::Any>>)
        }
    }
}

/// Check whether the FFmpeg build includes an AAC encoder.
pub fn is_aac_encoder_available() -> bool {
    codec::encoder::find(codec::Id::AAC).is_some()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aac_encoder_config_default() {
        assert!(is_aac_encoder_available());
    }

    #[test]
    fn test_get_recommended_bitrate() {
        assert_eq!(get_recommended_bitrate(1), 64_000);
        assert_eq!(get_recommended_bitrate(2), 128_000);
        assert_eq!(get_recommended_bitrate(6), 384_000);
    }

    #[test]
    fn test_aac_encoder_creation() {
        if !is_aac_encoder_available() {
            return;
        }
        let enc = AacEncoder::open(48000, 2, 128_000);
        assert!(enc.is_ok(), "AAC encoder should open: {:?}", enc.err());
    }

    #[test]
    fn test_aac_encoder_with_config() {
        if !is_aac_encoder_available() {
            return;
        }
        let enc = AacEncoder::open(48000, 2, 256_000);
        assert!(enc.is_ok());
        let enc = enc.unwrap();
        assert_eq!(enc.output_timebase(), ffmpeg::Rational::new(1, 48000));
    }
    #[test]
    fn test_aac_encoder_delay() {
        if !is_aac_encoder_available() {
            return;
        }
        let sample_rate = 48000;
        let mut enc = AacEncoder::open(sample_rate, 2, 256_000).unwrap();

        let mut frame =
            ffmpeg::util::frame::Audio::new(ENCODER_SAMPLE_FMT, 1024, ChannelLayout::STEREO);
        frame.set_rate(sample_rate as u32);

        // Zero-initialize the PCM frame data to avoid FFmpeg "Invalid argument" errors
        for ch in 0..2 {
            let data = frame.data_mut(ch);
            for d in data.iter_mut() {
                *d = 0;
            }
        }

        for i in 0..5 {
            frame.set_pts(Some(i * 1024));
            enc.send_frame(&frame).unwrap();

            while let Ok(Some(packet)) = enc.receive_packet() {
                println!("Received packet with pts: {:?}", packet.pts());
            }
        }
    }
}
