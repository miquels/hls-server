//! Audio stream analysis

use crate::error::Result;
use crate::media::AudioStreamInfo;
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
        encoder_delay: 0,
        transcode_to: None,
    })
}

/// Extract language from stream metadata
fn get_stream_language(stream: &ffmpeg::Stream) -> Option<String> {
    stream.metadata().get("language").map(|s| s.to_string())
}
