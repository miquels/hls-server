//! FFmpeg utility functions

use ffmpeg_next as ffmpeg;

/// Convert timestamps from one timebase to another
///
/// This is essential when copying packets between streams with different timebases.
pub fn rescale_ts(ts: i64, from: ffmpeg::Rational, to: ffmpeg::Rational) -> i64 {
    unsafe { ffmpeg::ffi::av_rescale_q(ts, from.into(), to.into()) }
}

/// Get the codec name for a codec ID
#[allow(dead_code)] // we need this for testing and development
pub fn codec_name(codec_id: ffmpeg::codec::Id) -> &'static str {
    codec_id.name()
}

/// Get the media type name
#[allow(dead_code)] // we need this for testing and development
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
#[allow(dead_code)] // we need this for testing and development
pub fn get_stream_language(stream: &ffmpeg::Stream) -> Option<String> {
    stream.metadata().get("language").map(|s| s.to_string())
}

/// Get the title from stream metadata
#[allow(dead_code)] // we need this for testing and development
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

/// Print stream information for debugging
#[allow(dead_code)] // we need this for testing and development
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
}
