//! HLS codec string generation
//!
//! Generates proper codec strings for HLS manifests.

use ffmpeg_next as ffmpeg;

/// Get HLS codec string for a video codec
pub fn get_video_codec_string(
    codec_id: ffmpeg::codec::Id,
    width: u32,
    height: u32,
    bitrate: u64,
    profile: Option<i32>,
    level: Option<i32>,
) -> Option<String> {
    match codec_id {
        ffmpeg::codec::Id::H264 => {
            Some(get_h264_profile_level(width, height, bitrate, profile, level).to_string())
        }
        ffmpeg::codec::Id::HEVC => Some("hvc1.1.6.L93.B0".to_string()), // HEVC Main
        ffmpeg::codec::Id::VP9 => Some("vp09.00.10.08".to_string()),    // VP9
        ffmpeg::codec::Id::AV1 => Some("av01.0.04M.08".to_string()),    // AV1 Main
        _ => None,
    }
}

/// Get HLS codec string for an audio codec
pub fn get_audio_codec_string(codec_id: ffmpeg::codec::Id) -> Option<&'static str> {
    match codec_id {
        ffmpeg::codec::Id::AAC => Some("mp4a.40.2"),  // AAC-LC
        ffmpeg::codec::Id::AC3 => Some("ac-3"),       // Dolby Digital
        ffmpeg::codec::Id::EAC3 => Some("ec-3"),      // Dolby Digital Plus
        ffmpeg::codec::Id::OPUS => Some("Opus"),      // Opus
        ffmpeg::codec::Id::VORBIS => Some("vorbis"),  // Vorbis
        ffmpeg::codec::Id::MP3 => Some("mp4a.40.34"), // MP3
        ffmpeg::codec::Id::FLAC => Some("flac"),      // FLAC
        _ => None,
    }
}

/// Build codec attribute for HLS variant
/// Combines video and audio codec strings
pub fn build_codec_attribute(
    video_codec: Option<ffmpeg::codec::Id>,
    video_width: u32,
    video_height: u32,
    video_bitrate: u64,
    video_profile: Option<i32>,
    video_level: Option<i32>,
    audio_codecs: &[ffmpeg::codec::Id],
    has_subtitles: bool,
) -> Option<String> {
    let mut codecs = Vec::new();

    // Add video codec
    if let Some(vid) = video_codec {
        if let Some(codec_str) = get_video_codec_string(
            vid,
            video_width,
            video_height,
            video_bitrate,
            video_profile,
            video_level,
        ) {
            codecs.push(codec_str);
        }
    }

    // Add audio codecs
    for &audio in audio_codecs {
        if let Some(codec_str) = get_audio_codec_string(audio) {
            if !codecs.contains(&codec_str.to_string()) {
                codecs.push(codec_str.to_string());
            }
        }
    }

    // Add subtitle codec
    if has_subtitles {
        codecs.push("wvtt".to_string());
    }

    if codecs.is_empty() {
        None
    } else {
        Some(codecs.join(","))
    }
}

/// Get profile level for H.264
pub fn get_h264_profile_level(
    width: u32,
    height: u32,
    _bitrate: u64,
    profile: Option<i32>,
    level: Option<i32>,
) -> String {
    let profile_byte = match profile {
        Some(66) => 0x42,  // Baseline
        Some(77) => 0x4d,  // Main
        Some(100) => 0x64, // High
        Some(244) => 0xf4, // High 4:4:4 Predictive
        _ => {
            // Fallback profile based on resolution if unknown
            if width * height <= 130000 {
                0x42 // Baseline
            } else if width * height <= 921600 {
                0x4d // Main
            } else {
                0x64 // High
            }
        }
    };

    let level_byte = if let Some(l) = level {
        // FFmpeg level is often integer (e.g. 30, 31, 40, 41, 51)
        // We simply map this to hex. 30 -> 1e, 40 -> 28, 51 -> 33
        // Sometimes it might be passed as the exact byte value?
        // Let's assume standard integer representation (e.g. 51 for 5.1)
        // If it's already a byte-like value (e.g. 51 is 0x33), we can use it directly?
        // No, 51 decimal is 0x33.
        // But what if FFmpeg returns 30 for 3.0?
        // 3.0 level is 30.
        // 30 decimal is 0x1E.
        // checks:
        // Level 3.0 (30) -> 0x1E
        // Level 3.1 (31) -> 0x1F
        // Level 4.0 (40) -> 0x28
        // Level 4.1 (41) -> 0x29
        // Level 5.0 (50) -> 0x32
        // Level 5.1 (51) -> 0x33
        // So we can just use the integer value directly formatted as hex?
        // Wait, 30 as hex is 0x1E? Yes.
        // So we can just use `l` as the byte.
        l as u8
    } else {
        // Fallback level calculation based on resolution
        let pixels = width * height;
        if pixels <= 130000 {
            21 // 2.1
        } else if pixels <= 414720 {
            30 // 3.0
        } else if pixels <= 921600 {
            31 // 3.1
        } else if pixels <= 2073600 {
            40 // 4.0
        } else if pixels <= 8847360 {
            51 // 5.1
        } else {
            52 // 5.2
        }
    };

    format!("avc1.{:02x}00{:02x}", profile_byte, level_byte)
}

pub fn calculate_bandwidth(bitrate: u64, audio_bitrates: &[u32]) -> u64 {
    let mut total = bitrate;
    for &ab in audio_bitrates {
        total += ab as u64;
    }
    // Add 60% overhead: HLS BANDWIDTH must be the peak segment bitrate.
    // FFmpeg's bitrate metadata underestimates actual peak, so a generous
    // margin ensures the declared BANDWIDTH >= any measured segment peak.
    total * 160 / 100
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_video_codec_strings() {
        // Test with explicit level
        assert_eq!(
            get_video_codec_string(
                ffmpeg::codec::Id::H264,
                1920,
                1080,
                5000000,
                Some(100),
                Some(40)
            ),
            Some("avc1.640028".to_string())
        );

        // Test with fallback level
        assert_eq!(
            get_video_codec_string(ffmpeg::codec::Id::H264, 640, 480, 1000000, Some(66), None),
            Some("avc1.42001e".to_string()) // 640x480 > 130000 -> Level 3.0 (0x1e)
        );

        assert_eq!(
            get_video_codec_string(ffmpeg::codec::Id::HEVC, 1920, 1080, 5000000, None, None),
            Some("hvc1.1.6.L93.B0".to_string())
        );
    }

    #[test]
    fn test_build_codec_attribute() {
        let codecs = build_codec_attribute(
            Some(ffmpeg::codec::Id::H264),
            1920,
            1080,
            5000000,
            Some(100),
            Some(41), // Level 4.1 -> 0x29
            &[ffmpeg::codec::Id::AAC],
            true, // has subtitles
        );
        assert!(codecs.is_some());
        assert_eq!(codecs.unwrap(), "avc1.640029,mp4a.40.2,wvtt");
    }

    #[test]
    fn test_h264_profile_level() {
        // High Profile (100 -> 0x64), Level 4.0 (40 -> 0x28)
        assert_eq!(
            get_h264_profile_level(1920, 1080, 5000000, Some(100), Some(40)),
            "avc1.640028"
        );

        // Main Profile (77 -> 0x4d), Level 3.1 (31 -> 0x1f)
        assert_eq!(
            get_h264_profile_level(1280, 720, 3000000, Some(77), Some(31)),
            "avc1.4d001f"
        );

        // Fallback: 1920x1080 -> High Profile (0x64), Level 4.0 (0x28)
        assert_eq!(
            get_h264_profile_level(1920, 1080, 5000000, None, None),
            "avc1.640028"
        );
    }
}
