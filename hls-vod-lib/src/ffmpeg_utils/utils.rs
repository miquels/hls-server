//! FFmpeg utility functions

use ffmpeg_next as ffmpeg;

/// Convert timestamps from one timebase to another
///
/// This is essential when copying packets between streams with different timebases.
pub fn rescale_ts(ts: i64, from: ffmpeg::Rational, to: ffmpeg::Rational) -> i64 {
    unsafe { ffmpeg::ffi::av_rescale_q(ts, from.into(), to.into()) }
}

/// Get the codec name for a codec ID
pub fn codec_name(codec_id: ffmpeg::codec::Id) -> &'static str {
    codec_id.name()
}

/// Get the media type name
pub fn media_type_name(media_type: ffmpeg::media::Type) -> &'static str {
    match media_type {
        ffmpeg::media::Type::Video => "video",
        ffmpeg::media::Type::Audio => "audio",
        ffmpeg::media::Type::Subtitle => "subtitle",
        ffmpeg::media::Type::Data => "data",
        ffmpeg::media::Type::Attachment => "attachment",
        _ => "unknown",
    }
}

/// Extract language from stream metadata
pub fn get_stream_language(stream: &ffmpeg::Stream) -> Option<String> {
    stream.metadata().get("language").map(|s| s.to_string())
}

/// Get the title from stream metadata
pub fn get_stream_title(stream: &ffmpeg::Stream) -> Option<String> {
    stream.metadata().get("title").map(|s| s.to_string())
}

/// Check if a codec is a video codec
pub fn is_video_codec(codec_id: ffmpeg::codec::Id) -> bool {
    matches!(
        codec_id,
        ffmpeg::codec::Id::H264
            | ffmpeg::codec::Id::HEVC
            | ffmpeg::codec::Id::VP9
            | ffmpeg::codec::Id::AV1
            | ffmpeg::codec::Id::MPEG4
            | ffmpeg::codec::Id::MPEG2VIDEO
            | ffmpeg::codec::Id::VP8
    )
}

/// Check if a codec is an audio codec
pub fn is_audio_codec(codec_id: ffmpeg::codec::Id) -> bool {
    matches!(
        codec_id,
        ffmpeg::codec::Id::AAC
            | ffmpeg::codec::Id::AC3
            | ffmpeg::codec::Id::EAC3
            | ffmpeg::codec::Id::OPUS
            | ffmpeg::codec::Id::VORBIS
            | ffmpeg::codec::Id::MP3
            | ffmpeg::codec::Id::FLAC
            | ffmpeg::codec::Id::PCM_S16LE
            | ffmpeg::codec::Id::PCM_S24LE
            | ffmpeg::codec::Id::TRUEHD
    )
}

/// Check if a codec is a subtitle codec
pub fn is_subtitle_codec(codec_id: ffmpeg::codec::Id) -> bool {
    matches!(
        codec_id,
        ffmpeg::codec::Id::SUBRIP
            | ffmpeg::codec::Id::ASS
            | ffmpeg::codec::Id::MOV_TEXT
            | ffmpeg::codec::Id::TEXT
            | ffmpeg::codec::Id::WEBVTT
            | ffmpeg::codec::Id::SSA
    )
}

/// Check if a subtitle codec is text-based (vs bitmap)
pub fn is_text_subtitle_codec(codec_id: ffmpeg::codec::Id) -> bool {
    matches!(
        codec_id,
        ffmpeg::codec::Id::SUBRIP
            | ffmpeg::codec::Id::ASS
            | ffmpeg::codec::Id::MOV_TEXT
            | ffmpeg::codec::Id::TEXT
            | ffmpeg::codec::Id::WEBVTT
            | ffmpeg::codec::Id::SSA
    )
}

/// Check if a subtitle codec is bitmap-based (PGS, DVB, etc.)
pub fn is_bitmap_subtitle_codec(codec_id: ffmpeg::codec::Id) -> bool {
    matches!(
        codec_id,
        ffmpeg::codec::Id::HDMV_PGS_SUBTITLE
            | ffmpeg::codec::Id::DVB_SUBTITLE
            | ffmpeg::codec::Id::DVB_TELETEXT
            | ffmpeg::codec::Id::XSUB
    )
}

/// Check if audio codec is AAC (no transcode needed)
pub fn is_aac_codec(codec_id: ffmpeg::codec::Id) -> bool {
    codec_id == ffmpeg::codec::Id::AAC
}

/// Check if audio codec needs transcoding to AAC
pub fn needs_audio_transcode(codec_id: ffmpeg::codec::Id) -> bool {
    !crate::audio_plan::planner::is_hls_supported_codec(codec_id)
}

/// Get the HLS codec string for a video codec
pub fn hls_video_codec_string(codec_id: ffmpeg::codec::Id) -> Option<&'static str> {
    match codec_id {
        ffmpeg::codec::Id::H264 => Some("avc1.42001e"),
        ffmpeg::codec::Id::HEVC => Some("hvc1.1.6.L93.B0"),
        ffmpeg::codec::Id::VP9 => Some("vp09.00.10.08"),
        ffmpeg::codec::Id::AV1 => Some("av01.0.04M.08"),
        _ => None,
    }
}

/// Get the HLS codec string for an audio codec
pub fn hls_audio_codec_string(codec_id: ffmpeg::codec::Id) -> Option<&'static str> {
    match codec_id {
        ffmpeg::codec::Id::AAC => Some("mp4a.40.2"),
        ffmpeg::codec::Id::AC3 => Some("ac-3"),
        ffmpeg::codec::Id::EAC3 => Some("ec-3"),
        ffmpeg::codec::Id::OPUS => Some("Opus"),
        ffmpeg::codec::Id::VORBIS => Some("vorbis"),
        ffmpeg::codec::Id::MP3 => Some("mp3"),
        ffmpeg::codec::Id::FLAC => Some("flac"),
        _ => None,
    }
}

/// Format a timestamp as HLS/WebVTT timestamp (HH:MM:SS.mmm)
pub fn format_hls_timestamp(pts_ms: i64) -> String {
    let total_ms = pts_ms.max(0) as u64;
    let hours = total_ms / 3_600_000;
    let minutes = (total_ms % 3_600_000) / 60_000;
    let seconds = (total_ms % 60_000) / 1000;
    let millis = total_ms % 1000;
    format!("{:02}:{:02}:{:02}.{:03}", hours, minutes, seconds, millis)
}

/// Convert PTS (presentation timestamp) to milliseconds
pub fn pts_to_ms(pts: i64, timebase: ffmpeg::Rational) -> i64 {
    let num = timebase.numerator() as i64;
    let den = timebase.denominator() as i64;
    (pts * num * 1000) / den
}

/// Convert milliseconds to PTS
pub fn ms_to_pts(ms: i64, timebase: ffmpeg::Rational) -> i64 {
    let num = timebase.numerator() as i64;
    let den = timebase.denominator() as i64;
    (ms * den) / (num * 1000)
}

/// Get frame rate as f64
pub fn framerate_to_f64(framerate: ffmpeg::Rational) -> f64 {
    if framerate.denominator() == 0 {
        0.0
    } else {
        framerate.numerator() as f64 / framerate.denominator() as f64
    }
}

/// Calculate segment duration from keyframe positions
pub fn calculate_segment_duration(start_pts: i64, end_pts: i64, timebase: ffmpeg::Rational) -> f64 {
    let duration_pts = end_pts - start_pts;
    let num = timebase.numerator() as f64;
    let den = timebase.denominator() as f64;
    (duration_pts as f64 * num) / den
}

/// Print stream information for debugging
pub fn debug_stream_info(stream: &ffmpeg::Stream, index: usize) {
    let codec_id = stream.parameters().id();
    let media_type = stream.parameters().medium();

    tracing::debug!(
        "Stream {}: type={}, codec={}",
        index,
        media_type_name(media_type),
        codec_name(codec_id)
    );

    if let Some(lang) = get_stream_language(stream) {
        tracing::debug!("  Language: {}", lang);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_hls_timestamp() {
        assert_eq!(format_hls_timestamp(0), "00:00:00.000");
        assert_eq!(format_hls_timestamp(1000), "00:00:01.000");
        assert_eq!(format_hls_timestamp(61000), "00:01:01.000");
        assert_eq!(format_hls_timestamp(3661000), "01:01:01.000");
    }

    #[test]
    fn test_format_hls_timestamp_negative() {
        assert_eq!(format_hls_timestamp(-1000), "00:00:00.000");
    }

    #[test]
    fn test_is_video_codec() {
        assert!(is_video_codec(ffmpeg::codec::Id::H264));
        assert!(is_video_codec(ffmpeg::codec::Id::HEVC));
        assert!(!is_video_codec(ffmpeg::codec::Id::AAC));
    }

    #[test]
    fn test_is_audio_codec() {
        assert!(is_audio_codec(ffmpeg::codec::Id::AAC));
        assert!(is_audio_codec(ffmpeg::codec::Id::AC3));
        assert!(!is_audio_codec(ffmpeg::codec::Id::H264));
    }

    #[test]
    fn test_is_text_subtitle_codec() {
        assert!(is_text_subtitle_codec(ffmpeg::codec::Id::SUBRIP));
        assert!(is_text_subtitle_codec(ffmpeg::codec::Id::MOV_TEXT));
        assert!(!is_text_subtitle_codec(
            ffmpeg::codec::Id::HDMV_PGS_SUBTITLE
        ));
    }

    #[test]
    fn test_needs_audio_transcode() {
        assert!(!needs_audio_transcode(ffmpeg::codec::Id::AAC));
        assert!(!needs_audio_transcode(ffmpeg::codec::Id::AC3));
        assert!(!needs_audio_transcode(ffmpeg::codec::Id::OPUS));
        assert!(needs_audio_transcode(ffmpeg::codec::Id::VORBIS));
    }
}
