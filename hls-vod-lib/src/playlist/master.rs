//! Master playlist generator
//!
//! Generates the master.m3u8 playlist that references all variant playlists.

use super::codec::{build_codec_attribute, calculate_bandwidth};
use crate::types::StreamIndex;

/// Generate master playlist content
///
/// The master playlist contains:
/// - One `#EXT-X-MEDIA` per audio track, grouped by codec family
///   (`GROUP-ID="audio-aac"`, `GROUP-ID="audio-ac3"`, etc.)
/// - One `#EXT-X-STREAM-INF` per audio codec group, all referencing the
///   same video variant playlist but differing in `AUDIO=` and `CODECS=`
/// - Subtitle MEDIA entries for text tracks
pub fn generate_master_playlist(index: &StreamIndex, prefix: &str) -> String {
    let mut output = String::new();

    // Header
    output.push_str("#EXTM3U\n");
    output.push_str("#EXT-X-VERSION:7\n");
    output.push_str("\n");

    // Use provided prefix for URLs
    let base_name = prefix;

    // Stream details collected directly from index

    // Convert 3-letter language code to 2-letter (RFC5646)
    fn to_rfc5646(lang: &str) -> &str {
        match lang {
            "eng" => "en",
            "fre" => "fr",
            "ger" => "de",
            "spa" => "es",
            "ita" => "it",
            "jpn" => "ja",
            "kor" => "ko",
            "chi" => "zh",
            "rus" => "ru",
            "por" => "pt",
            _ => lang,
        }
    }

    /// Return the codec-family GROUP-ID for a given stream.
    /// All transcoded variants are placed in the "audio-aac" group.
    fn group_id_for_stream(stream: &crate::types::AudioStreamInfo) -> &'static str {
        if stream.is_transcoded {
            return "audio-aac";
        }

        use ffmpeg_next::codec::Id;
        match stream.codec_id {
            Id::AAC => "audio-aac",
            Id::AC3 => "audio-ac3",
            Id::EAC3 => "audio-eac3",
            Id::MP3 => "audio-mp3",
            Id::OPUS => "audio-opus",
            _ => "audio-aac",
        }
    }

    /// HLS codec string we advertise for a given group.
    fn codec_str_for_group(group_id: &str) -> &'static str {
        match group_id {
            "audio-ac3" => "ac-3",
            "audio-eac3" => "ec-3",
            "audio-mp3" => "mp4a.40.34",
            "audio-opus" => "Opus",
            _ => "mp4a.40.2",
        }
    }

    if !index.audio_streams.is_empty() {
        output.push_str("# Audio Tracks\n");

        // Sort variants for stable output: by group_id then stream_index
        let mut streams_sorted = index.audio_streams.clone();
        streams_sorted.sort_by(|a, b| {
            let ga = group_id_for_stream(a);
            let gb = group_id_for_stream(b);
            ga.cmp(gb).then(a.stream_index.cmp(&b.stream_index))
        });

        // Track which group_ids we've seen so we can mark the first of each as DEFAULT
        let mut seen_groups: std::collections::HashSet<&str> = std::collections::HashSet::new();

        for variant in &streams_sorted {
            let group_id = group_id_for_stream(variant);
            let language = variant.language.as_deref().unwrap_or("und");
            let language_rfc = to_rfc5646(language);
            let codec_label = if variant.is_transcoded {
                "AAC (Transcoded)"
            } else {
                match variant.codec_id {
                    ffmpeg_next::codec::Id::AAC => "AAC",
                    ffmpeg_next::codec::Id::AC3 => "Dolby Digital",
                    ffmpeg_next::codec::Id::EAC3 => "Dolby Digital Plus",
                    ffmpeg_next::codec::Id::MP3 => "MP3",
                    ffmpeg_next::codec::Id::OPUS => "Opus",
                    _ => "Audio",
                }
            };

            let name = if language == "und" {
                codec_label.to_string()
            } else {
                format!("{} {}", language.to_uppercase(), codec_label)
            };

            let is_first_in_group = seen_groups.insert(group_id);
            let default = if is_first_in_group { "YES" } else { "NO" };

            let uri = if variant.is_transcoded {
                format!("{}/a/{}-aac.m3u8", base_name, variant.stream_index)
            } else {
                format!("{}/a/{}.m3u8", base_name, variant.stream_index)
            };

            output.push_str(&format!(
                "#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"{}\",LANGUAGE=\"{}\",NAME=\"{}\",DEFAULT={},AUTOSELECT=YES,URI=\"{}\"\n",
                group_id, language_rfc, name, default, uri
            ));
        }
        output.push_str("\n");
    }

    // ── Subtitle MEDIA groups ──────────────────────────────────────────────
    if !index.subtitle_streams.is_empty() {
        output.push_str("# Subtitle Tracks\n");
        for (i, sub) in index.subtitle_streams.iter().enumerate() {
            let language = sub.language.as_deref().unwrap_or("und");
            let language_rfc = to_rfc5646(language);
            let group_id = "subs";
            let name = format!("{} Subtitles", language.to_uppercase());
            let default = if i == 0 { "YES" } else { "NO" };
            let uri = format!("{}/s/{}.m3u8", base_name, sub.stream_index);

            output.push_str(&format!(
                "#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"{}\",LANGUAGE=\"{}\",NAME=\"{}\",DEFAULT={},AUTOSELECT={},FORCED=NO,URI=\"{}\"\n",
                group_id, language_rfc, name, default, default, uri
            ));
        }
        output.push_str("\n");
    }

    // ── Video Variants ─────────────────────────────────────────────────────
    // Emit one EXT-X-STREAM-INF per unique audio codec group so that clients
    // see all available codec combinations (e.g. AAC + AC-3).
    output.push_str("# Video Variants\n");
    if let Some(video) = index.primary_video() {
        let resolution = format!("{}x{}", video.width, video.height);

        // Subtitle group attribute (same for all variants)
        let subtitle_attr = if !index.subtitle_streams.is_empty() {
            ",SUBTITLES=\"subs\"".to_string()
        } else {
            String::new()
        };

        // Collect distinct audio codec groups (preserving first-seen order)
        let audio_groups: Vec<&'static str> = {
            let mut seen = std::collections::HashSet::new();
            let mut groups = Vec::new();
            for s in &index.audio_streams {
                let g = group_id_for_stream(s);
                if seen.insert(g) {
                    groups.push(g);
                }
            }
            groups
        };

        if audio_groups.is_empty() {
            // No audio: single variant with only video codec
            let codecs = build_codec_attribute(
                Some(video.codec_id),
                video.width,
                video.height,
                video.bitrate,
                video.profile,
                video.level,
                &[],
                !index.subtitle_streams.is_empty(),
            );
            let bandwidth = calculate_bandwidth(video.bitrate.max(100000), &[]);
            let codec_attr = codecs
                .map(|c| format!(",CODECS=\"{}\"", c))
                .unwrap_or_default();

            output.push_str(&format!(
                "#EXT-X-STREAM-INF:BANDWIDTH={},RESOLUTION={}{}{}\n",
                bandwidth, resolution, subtitle_attr, codec_attr
            ));
            output.push_str(&format!("{}/v/media.m3u8\n", base_name));
        } else {
            // One variant per audio codec group
            for group_id in &audio_groups {
                let audio_codec_str = codec_str_for_group(group_id);

                // Build full codec string: video + this audio group's codec
                // Build full codec string: video + audio + subtitles
                let has_subs = !index.subtitle_streams.is_empty();
                let video_codec_str = build_codec_attribute(
                    Some(video.codec_id),
                    video.width,
                    video.height,
                    video.bitrate,
                    video.profile,
                    video.level,
                    &[],
                    false,
                );

                let mut codec_list = Vec::new();
                if let Some(vc) = video_codec_str {
                    codec_list.push(vc);
                }
                codec_list.push(audio_codec_str.to_string());
                if has_subs {
                    codec_list.push("wvtt".to_string());
                }
                let codecs = codec_list.join(",");

                // Bandwidth: video + all audio streams in this group
                let group_audio_bitrates: Vec<u32> = index
                    .audio_streams
                    .iter()
                    .filter(|s| group_id_for_stream(s) == *group_id)
                    .map(|s| s.bitrate as u32)
                    .collect();
                let bandwidth =
                    calculate_bandwidth(video.bitrate.max(100_000), &group_audio_bitrates);

                output.push_str(&format!(
                    "#EXT-X-STREAM-INF:BANDWIDTH={},RESOLUTION={},AUDIO=\"{}\",CODECS=\"{}\"{}\n",
                    bandwidth, resolution, group_id, codecs, subtitle_attr
                ));
                output.push_str(&format!("{}/v/media.m3u8\n", base_name));
            }
        }
    }

    output
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AudioStreamInfo, SubtitleFormat, SubtitleStreamInfo, VideoStreamInfo};
    use ffmpeg_next as ffmpeg;
    use std::path::PathBuf;

    fn create_test_index() -> StreamIndex {
        let mut index = StreamIndex::new(PathBuf::from("/test/video.mp4"));

        index.video_streams.push(VideoStreamInfo {
            stream_index: 0,
            codec_id: ffmpeg::codec::Id::H264,
            width: 1920,
            height: 1080,
            bitrate: 5000000,
            framerate: ffmpeg::Rational::new(30, 1),
            language: None,
            profile: None,
            level: None,
        });

        index.audio_streams.push(AudioStreamInfo {
            stream_index: 1,
            codec_id: ffmpeg::codec::Id::AAC,
            sample_rate: 48000,
            channels: 2,
            bitrate: 128000,
            language: Some("en".to_string()),
            is_transcoded: false,
            source_stream_index: None,
            encoder_delay: 0,
        });

        index
    }

    #[test]
    fn test_generate_master_playlist() {
        let index = create_test_index();
        let playlist = generate_master_playlist(&index, "video.mp4");

        assert!(playlist.contains("#EXTM3U"));
        assert!(playlist.contains("#EXT-X-VERSION:7"));
        assert!(playlist.contains("#EXT-X-STREAM-INF"));
        assert!(playlist.contains("BANDWIDTH="));
        assert!(playlist.contains("RESOLUTION=1920x1080"));
        assert!(playlist.contains("video.mp4/v/media.m3u8"));
    }

    #[test]
    fn test_generate_master_playlist_with_audio() {
        let index = create_test_index();
        let playlist = generate_master_playlist(&index, "video.mp4");

        assert!(playlist.contains("TYPE=AUDIO"));
        assert!(playlist.contains("LANGUAGE=\"en\""));
        assert!(playlist.contains("video.mp4/a/1.m3u8"));
    }

    #[test]
    fn test_generate_master_playlist_with_subtitles() {
        let mut index = create_test_index();
        index.subtitle_streams.push(SubtitleStreamInfo {
            stream_index: 2,
            codec_id: ffmpeg::codec::Id::SUBRIP,
            language: Some("en".to_string()),
            format: SubtitleFormat::SubRip,
            non_empty_sequences: Vec::new(),
            sample_index: Vec::new(),
            timebase: ffmpeg::Rational::new(1, 1000),
            start_time: 0,
        });

        let playlist = generate_master_playlist(&index, "video.mp4");

        assert!(playlist.contains("TYPE=SUBTITLES"));
        assert!(playlist.contains("video.mp4/s/2.m3u8"));
        assert!(playlist.contains("CODECS=\"avc1.640028,mp4a.40.2,wvtt\""));
    }
}
