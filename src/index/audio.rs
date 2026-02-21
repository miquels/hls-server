//! Audio stream analysis

use ffmpeg_next as ffmpeg;
use crate::state::AudioStreamInfo;
use crate::error::Result;

/// Analyze an audio stream and extract metadata
pub fn analyze_audio_stream(stream: &ffmpeg::Stream, index: usize) -> Result<AudioStreamInfo> {
    let codec_id = stream.parameters().id();
    
    // Get audio info from codec parameters
    let (sample_rate, channels) = unsafe {
        let params_ptr = stream.parameters().as_ptr();
        (
            (*params_ptr).sample_rate as u32,
            (*params_ptr).ch_layout.nb_channels as u16
        )
    };

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
    stream
        .metadata()
        .get("language")
        .map(|s| s.to_string())
}

/// Check if audio codec is AAC (no transcode needed for HLS)
pub fn is_aac_codec(codec_id: ffmpeg::codec::Id) -> bool {
    codec_id == ffmpeg::codec::Id::AAC
}

/// Check if audio codec needs transcoding to AAC
pub fn needs_transcode(codec_id: ffmpeg::codec::Id) -> bool {
    is_audio_codec(codec_id) && !is_aac_codec(codec_id)
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

/// Get audio codec name for HLS manifest
pub fn get_audio_codec_string(codec_id: ffmpeg::codec::Id) -> Option<&'static str> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_aac_codec() {
        assert!(is_aac_codec(ffmpeg::codec::Id::AAC));
        assert!(!is_aac_codec(ffmpeg::codec::Id::AC3));
        assert!(!is_aac_codec(ffmpeg::codec::Id::OPUS));
    }

    #[test]
    fn test_needs_transcode() {
        assert!(!needs_transcode(ffmpeg::codec::Id::AAC));
        assert!(needs_transcode(ffmpeg::codec::Id::AC3));
        assert!(needs_transcode(ffmpeg::codec::Id::OPUS));
    }

    #[test]
    fn test_get_audio_codec_string() {
        assert_eq!(get_audio_codec_string(ffmpeg::codec::Id::AAC), Some("mp4a.40.2"));
        assert_eq!(get_audio_codec_string(ffmpeg::codec::Id::AC3), Some("ac-3"));
        assert_eq!(get_audio_codec_string(ffmpeg::codec::Id::EAC3), Some("ec-3"));
    }
}
