//! Subtitle stream analysis

use crate::types::{SubtitleFormat, SubtitleStreamInfo};
use ffmpeg_next as ffmpeg;

/// Analyze a subtitle stream and extract metadata
pub fn analyze_subtitle_stream(
    stream: &ffmpeg::Stream,
    index: usize,
) -> Option<SubtitleStreamInfo> {
    let codec_id = stream.parameters().id();

    // Skip bitmap subtitles (PGS, DVB) - they can't be converted to WebVTT
    if is_bitmap_subtitle(codec_id) {
        return None;
    }

    // Only process text-based subtitles
    if !is_text_subtitle(codec_id) {
        return None;
    }

    let mut start_time = stream.start_time();
    if start_time == std::i64::MIN {
        start_time = 0;
    }

    Some(SubtitleStreamInfo {
        stream_index: index,
        codec_id,
        language: get_stream_language(stream),
        format: get_subtitle_format(codec_id),
        non_empty_sequences: Vec::new(), // populated by scanner
        sample_index: Vec::new(),        // populated by scanner
        timebase: stream.time_base(),
        start_time,
    })
}

/// Extract language from stream metadata
fn get_stream_language(stream: &ffmpeg::Stream) -> Option<String> {
    stream.metadata().get("language").map(|s| s.to_string())
}

/// Check if codec is a text-based subtitle format
pub fn is_text_subtitle(codec_id: ffmpeg::codec::Id) -> bool {
    matches!(
        codec_id,
        ffmpeg::codec::Id::SUBRIP      // SRT
            | ffmpeg::codec::Id::ASS           // ASS/SSA
            | ffmpeg::codec::Id::SSA           // SSA
            | ffmpeg::codec::Id::MOV_TEXT      // QuickTime TTXT
            | ffmpeg::codec::Id::TEXT          // Plain text
            | ffmpeg::codec::Id::WEBVTT // WebVTT
    )
}

/// Check if codec is a bitmap subtitle format (not supported for HLS)
pub fn is_bitmap_subtitle(codec_id: ffmpeg::codec::Id) -> bool {
    matches!(
        codec_id,
        ffmpeg::codec::Id::HDMV_PGS_SUBTITLE  // Blu-ray PGS
            | ffmpeg::codec::Id::DVB_SUBTITLE       // DVB
            | ffmpeg::codec::Id::DVB_TELETEXT       // DVB Teletext
            | ffmpeg::codec::Id::XSUB // DivX XSUB
    )
}

/// Get subtitle format enum from codec ID
pub fn get_subtitle_format(codec_id: ffmpeg::codec::Id) -> SubtitleFormat {
    match codec_id {
        ffmpeg::codec::Id::SUBRIP => SubtitleFormat::SubRip,
        ffmpeg::codec::Id::ASS | ffmpeg::codec::Id::SSA => SubtitleFormat::Ass,
        ffmpeg::codec::Id::MOV_TEXT => SubtitleFormat::MovText,
        ffmpeg::codec::Id::WEBVTT => SubtitleFormat::WebVtt,
        ffmpeg::codec::Id::TEXT => SubtitleFormat::Text,
        _ => SubtitleFormat::Unknown,
    }
}

/// Get subtitle codec name for logging
pub fn get_subtitle_codec_name(codec_id: ffmpeg::codec::Id) -> &'static str {
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
    fn test_is_text_subtitle() {
        assert!(is_text_subtitle(ffmpeg::codec::Id::SUBRIP));
        assert!(is_text_subtitle(ffmpeg::codec::Id::ASS));
        assert!(is_text_subtitle(ffmpeg::codec::Id::MOV_TEXT));
        assert!(!is_text_subtitle(ffmpeg::codec::Id::HDMV_PGS_SUBTITLE));
    }

    #[test]
    fn test_is_bitmap_subtitle() {
        assert!(is_bitmap_subtitle(ffmpeg::codec::Id::HDMV_PGS_SUBTITLE));
        assert!(is_bitmap_subtitle(ffmpeg::codec::Id::DVB_SUBTITLE));
        assert!(!is_bitmap_subtitle(ffmpeg::codec::Id::SUBRIP));
    }

    #[test]
    fn test_get_subtitle_format() {
        assert_eq!(
            get_subtitle_format(ffmpeg::codec::Id::SUBRIP),
            SubtitleFormat::SubRip
        );
        assert_eq!(
            get_subtitle_format(ffmpeg::codec::Id::ASS),
            SubtitleFormat::Ass
        );
        assert_eq!(
            get_subtitle_format(ffmpeg::codec::Id::MOV_TEXT),
            SubtitleFormat::MovText
        );
    }
}
