//! Audio stream analysis

use crate::error::Result;
use crate::types::AudioStreamInfo;
use ffmpeg_next as ffmpeg;

/// Analyze an audio stream and extract metadata
pub fn analyze_audio_stream(stream: &ffmpeg::Stream, index: usize) -> Result<AudioStreamInfo> {
    let codec_id = stream.parameters().id();

    // Get audio info from codec parameters
    let params = stream.parameters();
    let sample_rate = crate::ffmpeg_utils::helpers::codec_params_sample_rate(&params);
    let channels = crate::ffmpeg_utils::helpers::codec_params_channels(&params);

    Ok(AudioStreamInfo {
        stream_index: index,
        codec_id,
        sample_rate,
        channels,
        bitrate: 0,
        language: get_stream_language(stream),
        is_transcoded: false,
        source_stream_index: None,
        encoder_delay: 0,
    })
}

/// Extract language from stream metadata
fn get_stream_language(stream: &ffmpeg::Stream) -> Option<String> {
    stream.metadata().get("language").map(|s| s.to_string())
}

/// Check if audio codec is AAC (no transcode needed for HLS)
pub fn is_aac_codec(codec_id: ffmpeg::codec::Id) -> bool {
    codec_id == ffmpeg::codec::Id::AAC
}

/// Check if a codec is a supported audio codec
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_aac_codec() {
        assert!(is_aac_codec(ffmpeg::codec::Id::AAC));
        assert!(!is_aac_codec(ffmpeg::codec::Id::AC3));
        assert!(!is_aac_codec(ffmpeg::codec::Id::OPUS));
    }
}
