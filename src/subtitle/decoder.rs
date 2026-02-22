#![allow(dead_code)]

//! Subtitle decoder for extracting subtitle packets

use ffmpeg_next as ffmpeg;
use crate::error::{FfmpegError, Result, HlsError};

/// Subtitle decoder for extracting text from source streams
pub struct SubtitleDecoder {
    /// Codec ID
    codec_id: ffmpeg::codec::Id,
    /// Source stream index
    stream_index: usize,
    /// Timebase
    timebase: ffmpeg::Rational,
    /// Whether this is a text-based subtitle format
    is_text_format: bool,
}

impl SubtitleDecoder {
    /// Create a new subtitle decoder
    pub fn new(codec_id: ffmpeg::codec::Id, stream_index: usize, timebase: ffmpeg::Rational) -> Result<Self> {
        // Verify decoder exists
        if !crate::ffmpeg::helpers::decoder_exists(codec_id) {
            return Err(HlsError::Ffmpeg(
                FfmpegError::DecoderNotFound(format!("Subtitle codec {:?} not found", codec_id))
            ));
        }

        // Check if this is a text-based format (not bitmap)
        let is_text_format = is_text_subtitle_codec(codec_id);

        Ok(Self {
            codec_id,
            stream_index,
            timebase,
            is_text_format,
        })
    }

    /// Get the codec ID
    pub fn codec_id(&self) -> ffmpeg::codec::Id {
        self.codec_id
    }

    /// Get the stream index
    pub fn stream_index(&self) -> usize {
        self.stream_index
    }

    /// Get the timebase
    pub fn timebase(&self) -> ffmpeg::Rational {
        self.timebase
    }

    /// Check if this is a text-based subtitle format
    pub fn is_text_format(&self) -> bool {
        self.is_text_format
    }

    /// Convert PTS to milliseconds
    pub fn pts_to_ms(&self, pts: i64) -> i64 {
        let num = self.timebase.numerator() as i64;
        let den = self.timebase.denominator() as i64;
        (pts * num * 1000) / den
    }
}

/// Check if a codec is a text-based subtitle format
pub fn is_text_subtitle_codec(codec_id: ffmpeg::codec::Id) -> bool {
    matches!(
        codec_id,
        ffmpeg::codec::Id::SUBRIP      // SRT
            | ffmpeg::codec::Id::ASS           // ASS/SSA
            | ffmpeg::codec::Id::SSA           // SSA
            | ffmpeg::codec::Id::MOV_TEXT      // QuickTime TTXT
            | ffmpeg::codec::Id::TEXT          // Plain text
            | ffmpeg::codec::Id::WEBVTT        // WebVTT
    )
}

/// Check if a codec is a bitmap subtitle format (not supported for HLS)
pub fn is_bitmap_subtitle_codec(codec_id: ffmpeg::codec::Id) -> bool {
    matches!(
        codec_id,
        ffmpeg::codec::Id::HDMV_PGS_SUBTITLE  // Blu-ray PGS
            | ffmpeg::codec::Id::DVB_SUBTITLE       // DVB
            | ffmpeg::codec::Id::DVB_TELETEXT       // DVB Teletext
            | ffmpeg::codec::Id::XSUB               // DivX XSUB
    )
}

/// Get subtitle format name
pub fn get_subtitle_format_name(codec_id: ffmpeg::codec::Id) -> &'static str {
    match codec_id {
        ffmpeg::codec::Id::SUBRIP => "SubRip (SRT)",
        ffmpeg::codec::Id::ASS | ffmpeg::codec::Id::SSA => "ASS/SSA",
        ffmpeg::codec::Id::MOV_TEXT => "QuickTime TTXT",
        ffmpeg::codec::Id::WEBVTT => "WebVTT",
        ffmpeg::codec::Id::TEXT => "Plain Text",
        ffmpeg::codec::Id::HDMV_PGS_SUBTITLE => "PGS (Bitmap)",
        ffmpeg::codec::Id::DVB_SUBTITLE => "DVB (Bitmap)",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_text_subtitle_codec() {
        assert!(is_text_subtitle_codec(ffmpeg::codec::Id::SUBRIP));
        assert!(is_text_subtitle_codec(ffmpeg::codec::Id::ASS));
        assert!(is_text_subtitle_codec(ffmpeg::codec::Id::MOV_TEXT));
        assert!(!is_text_subtitle_codec(ffmpeg::codec::Id::HDMV_PGS_SUBTITLE));
    }

    #[test]
    fn test_is_bitmap_subtitle_codec() {
        assert!(is_bitmap_subtitle_codec(ffmpeg::codec::Id::HDMV_PGS_SUBTITLE));
        assert!(is_bitmap_subtitle_codec(ffmpeg::codec::Id::DVB_SUBTITLE));
        assert!(!is_bitmap_subtitle_codec(ffmpeg::codec::Id::SUBRIP));
    }

    #[test]
    fn test_get_subtitle_format_name() {
        assert_eq!(get_subtitle_format_name(ffmpeg::codec::Id::SUBRIP), "SubRip (SRT)");
        assert_eq!(get_subtitle_format_name(ffmpeg::codec::Id::ASS), "ASS/SSA");
        assert_eq!(get_subtitle_format_name(ffmpeg::codec::Id::HDMV_PGS_SUBTITLE), "PGS (Bitmap)");
    }

    #[test]
    fn test_create_subtitle_decoder() {
        let decoder = SubtitleDecoder::new(
            ffmpeg::codec::Id::SUBRIP,
            2,
            ffmpeg::Rational::new(1, 90000),
        );
        assert!(decoder.is_ok());
        let decoder = decoder.unwrap();
        assert!(decoder.is_text_format());
        assert_eq!(decoder.stream_index(), 2);
    }

    #[test]
    fn test_pts_to_ms() {
        let decoder = SubtitleDecoder::new(
            ffmpeg::codec::Id::SUBRIP,
            0,
            ffmpeg::Rational::new(1, 90000),
        ).unwrap();
        
        // 90000 ticks = 1 second = 1000ms
        assert_eq!(decoder.pts_to_ms(90000), 1000);
        assert_eq!(decoder.pts_to_ms(45000), 500);
    }
}
