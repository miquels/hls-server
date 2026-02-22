//! Audio track planner - determines which audio variants to serve

use crate::index::audio::{is_aac_codec, is_audio_codec};
use crate::types::AudioStreamInfo;
use ffmpeg_next as ffmpeg;

/// Audio variant for HLS manifest
#[derive(Debug, Clone)]
pub struct AudioVariant {
    /// Stream index in source file
    pub stream_index: usize,
    /// Codec ID
    pub codec_id: ffmpeg::codec::Id,
    /// Language code (e.g., "en", "es")
    pub language: Option<String>,
    /// Whether this variant requires transcoding to AAC
    pub requires_transcode: bool,
    /// Display name for the track
    pub name: Option<String>,
    /// Whether this is the default track
    pub is_default: bool,
}

/// Audio track plan for a stream
#[derive(Debug, Clone)]
pub struct AudioTrackPlan {
    /// All audio variants to serve
    pub variants: Vec<AudioVariant>,
    /// Whether any transcoding is required
    pub requires_transcoding: bool,
}

impl AudioTrackPlan {
    /// Create a new empty audio track plan
    pub fn new() -> Self {
        Self {
            variants: Vec::new(),
            requires_transcoding: false,
        }
    }

    /// Check if there are any audio tracks
    pub fn has_audio(&self) -> bool {
        !self.variants.is_empty()
    }

    /// Get the default audio variant
    #[allow(dead_code)]
    pub fn default_variant(&self) -> Option<&AudioVariant> {
        self.variants
            .iter()
            .find(|v| v.is_default)
            .or(self.variants.first())
    }
}

impl Default for AudioTrackPlan {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
pub fn get_codec_string(_codec_id: ffmpeg::codec::Id) -> Option<String> {
    None
}

/// Plan audio tracks for a stream index
pub fn plan_audio_tracks(audio_streams: &[AudioStreamInfo]) -> AudioTrackPlan {
    let mut plan = AudioTrackPlan::new();

    if audio_streams.is_empty() {
        return plan;
    }

    // Group streams by language
    let mut by_language: std::collections::HashMap<String, Vec<&AudioStreamInfo>> =
        std::collections::HashMap::new();

    for stream in audio_streams {
        let lang = stream.language.clone().unwrap_or_else(|| "und".to_string());
        by_language.entry(lang).or_default().push(stream);
    }

    // Process each language group
    for (language, streams) in by_language {
        let mut native_aac_added = false;
        let mut supported_native_variants = Vec::new();

        // 1. Add all native, HLS-supported streams
        for stream in &streams {
            if is_hls_supported_codec(stream.codec_id) {
                if is_aac_codec(stream.codec_id) {
                    native_aac_added = true;
                }

                let codec_label = match stream.codec_id {
                    ffmpeg::codec::Id::AAC => "AAC",
                    ffmpeg::codec::Id::AC3 => "Dolby Digital",
                    ffmpeg::codec::Id::EAC3 => "Dolby Digital Plus",
                    ffmpeg::codec::Id::MP3 => "MP3",
                    ffmpeg::codec::Id::OPUS => "Opus",
                    _ => "Audio",
                };

                let name = if language == "und" {
                    codec_label.to_string()
                } else {
                    format!("{} {}", language.to_uppercase(), codec_label)
                };

                supported_native_variants.push(AudioVariant {
                    stream_index: stream.stream_index,
                    codec_id: stream.codec_id,
                    language: if language == "und" {
                        None
                    } else {
                        Some(language.clone())
                    },
                    requires_transcode: false,
                    name: Some(name),
                    is_default: false, // will set default later
                });
            }
        }

        // 2. If NO native AAC was found for this language, we MUST add a transcoded AAC version
        // of the first supported native stream (or the first stream if none are supported natively).
        if !native_aac_added {
            // Find a candidate to transcode to AAC for maximum compatibility.
            // Prefer the first supported native stream (e.g. AC-3 -> AAC).
            let candidate = streams
                .iter()
                .find(|s| is_hls_supported_codec(s.codec_id))
                .or_else(|| streams.first());

            if let Some(stream) = candidate {
                let name = if language == "und" {
                    "AAC (Transcoded)".to_string()
                } else {
                    format!("{} AAC (Transcoded)", language.to_uppercase())
                };

                plan.variants.push(AudioVariant {
                    stream_index: stream.stream_index,
                    codec_id: ffmpeg::codec::Id::AAC,
                    language: if language == "und" {
                        None
                    } else {
                        Some(language.clone())
                    },
                    requires_transcode: true,
                    name: Some(name),
                    is_default: plan.variants.is_empty(), // Default if it's the first track in the entire master
                });
                plan.requires_transcoding = true;
            }
        }

        // 3. Add the native variants we collected earlier
        for mut variant in supported_native_variants {
            // If this is the first track in the plan, and it's native, make it default
            if plan.variants.is_empty() {
                variant.is_default = true;
            }
            plan.variants.push(variant);
        }

        // 4. Handle non-HLS-supported streams (Vorbis, FLAC, etc.) - they only get an AAC version
        for stream in &streams {
            if !is_hls_supported_codec(stream.codec_id) && is_audio_codec(stream.codec_id) {
                let codec_label = match stream.codec_id {
                    ffmpeg::codec::Id::VORBIS => "Vorbis",
                    ffmpeg::codec::Id::FLAC => "FLAC",
                    _ => "Audio",
                };
                let name = if language == "und" {
                    format!("{} (Transcoded)", codec_label)
                } else {
                    format!("{} {} (Transcoded)", language.to_uppercase(), codec_label)
                };

                plan.variants.push(AudioVariant {
                    stream_index: stream.stream_index,
                    codec_id: ffmpeg::codec::Id::AAC,
                    language: if language == "und" {
                        None
                    } else {
                        Some(language.clone())
                    },
                    requires_transcode: true,
                    name: Some(name),
                    is_default: plan.variants.is_empty(),
                });
                plan.requires_transcoding = true;
            }
        }
    }

    plan
}

/// Check if a codec is supported for HLS
pub fn is_hls_supported_codec(codec_id: ffmpeg::codec::Id) -> bool {
    matches!(
        codec_id,
        ffmpeg::codec::Id::AAC
            | ffmpeg::codec::Id::AC3
            | ffmpeg::codec::Id::EAC3
            | ffmpeg::codec::Id::MP3
            | ffmpeg::codec::Id::OPUS
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_audio_stream(
        index: usize,
        codec_id: ffmpeg::codec::Id,
        language: Option<&str>,
    ) -> AudioStreamInfo {
        AudioStreamInfo {
            stream_index: index,
            codec_id,
            sample_rate: 48000,
            channels: 2,
            bitrate: 128000,
            language: language.map(|s| s.to_string()),
            is_transcoded: false,
            source_stream_index: None,
            encoder_delay: 0,
        }
    }

    #[test]
    fn test_plan_audio_tracks_aac_only() {
        let streams = vec![create_test_audio_stream(
            0,
            ffmpeg::codec::Id::AAC,
            Some("en"),
        )];
        let plan = plan_audio_tracks(&streams);

        assert_eq!(plan.variants.len(), 1);
        assert!(!plan.requires_transcoding);
        assert!(!plan.variants[0].requires_transcode);
    }

    #[test]
    fn test_plan_audio_tracks_ac3_only() {
        let streams = vec![create_test_audio_stream(
            0,
            ffmpeg::codec::Id::AC3,
            Some("en"),
        )];
        let plan = plan_audio_tracks(&streams);

        // Should have 2 variants: 1 native AC3 and 1 transcoded AAC
        assert_eq!(plan.variants.len(), 2);
        assert!(plan.requires_transcoding);

        let ac3 = plan
            .variants
            .iter()
            .find(|v| v.codec_id == ffmpeg::codec::Id::AC3)
            .unwrap();
        let aac = plan
            .variants
            .iter()
            .find(|v| v.codec_id == ffmpeg::codec::Id::AAC)
            .unwrap();

        assert!(!ac3.requires_transcode);
        assert!(aac.requires_transcode);
    }

    #[test]
    fn test_plan_audio_tracks_aac_and_ac3() {
        let streams = vec![
            create_test_audio_stream(0, ffmpeg::codec::Id::AAC, Some("en")),
            create_test_audio_stream(1, ffmpeg::codec::Id::AC3, Some("en")),
        ];
        let plan = plan_audio_tracks(&streams);

        // Should have 2 variants (both native)
        assert_eq!(plan.variants.len(), 2);
        assert!(!plan.requires_transcoding);
        assert!(!plan.variants[0].requires_transcode);
        assert!(!plan.variants[1].requires_transcode);
    }

    #[test]
    fn test_plan_audio_tracks_multi_language() {
        let streams = vec![
            create_test_audio_stream(0, ffmpeg::codec::Id::AAC, Some("en")),
            create_test_audio_stream(1, ffmpeg::codec::Id::AAC, Some("es")),
        ];
        let plan = plan_audio_tracks(&streams);

        assert_eq!(plan.variants.len(), 2);
        assert!(!plan.requires_transcoding);
    }

    #[test]
    fn test_is_hls_supported_codec() {
        assert!(is_hls_supported_codec(ffmpeg::codec::Id::AAC));
        assert!(is_hls_supported_codec(ffmpeg::codec::Id::AC3));
        assert!(is_hls_supported_codec(ffmpeg::codec::Id::OPUS));
    }
}
