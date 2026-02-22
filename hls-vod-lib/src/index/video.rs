//! Video stream analysis

use crate::error::Result;
use crate::types::VideoStreamInfo;
use ffmpeg_next as ffmpeg;

/// Analyze a video stream and extract metadata
pub fn analyze_video_stream(stream: &ffmpeg::Stream, index: usize) -> Result<VideoStreamInfo> {
    let codec_id = stream.parameters().id();

    // Get video dimensions, profile, level and bitrate from codec parameters
    let params = stream.parameters();
    let width = crate::ffmpeg_utils::helpers::codec_params_width(&params);
    let height = crate::ffmpeg_utils::helpers::codec_params_height(&params);
    let profile = crate::ffmpeg_utils::helpers::codec_params_profile(&params);
    let level = crate::ffmpeg_utils::helpers::codec_params_level(&params);
    let bitrate = crate::ffmpeg_utils::helpers::codec_params_bit_rate(&params);

    // Get frame rate from stream
    let framerate = stream.avg_frame_rate();

    Ok(VideoStreamInfo {
        stream_index: index,
        codec_id,
        width,
        height,
        bitrate,
        framerate,
        language: get_stream_language(stream),
        profile: if profile != -99 { Some(profile) } else { None },
        level: if level != -99 { Some(level) } else { None },
    })
}

/// Extract language from stream metadata
fn get_stream_language(stream: &ffmpeg::Stream) -> Option<String> {
    stream.metadata().get("language").map(|s| s.to_string())
}

#[cfg(test)]
mod tests {}
