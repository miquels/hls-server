//! Audio track planner - determines which audio variants to serve

use crate::index::audio::{is_aac_codec, is_audio_codec};
use crate::state::AudioStreamInfo;
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
    /// Sample rate in Hz
    pub sample_rate: u32,
    /// Number of channels
    pub channels: u16,
    /// Whether this variant requires transcoding to AAC
    pub requires_transcode: bool,
    /// HLS codec string (e.g., "mp4a.40.2", "ac-3")
    pub codec_string: Option<String>,
    /// Display name for the track
    pub name: Option<String>,
    /// Whether this is the default track
    pub is_default: bool,
}

/// Transcode decision for an audio stream
#[derive(Debug, Clone, PartialEq)]
pub enum TranscodeDecision {
    /// Stream can be copied directly (AAC)
    Copy,
    /// Stream needs transcoding to AAC
    TranscodeToAac,
    /// Stream should be excluded from HLS
    Exclude,
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

    /// Get the number of audio variants
    pub fn variant_count(&self) -> usize {
        self.variants.len()
    }

    /// Get variants for a specific language
    pub fn variants_by_language(&self, language: &str) -> Vec<&AudioVariant> {
        self.variants
            .iter()
            .filter(|v| {
                v.language
                    .as_ref()
                    .map(|l| l.to_lowercase() == language.to_lowercase())
                    .unwrap_or(false)
            })
            .collect()
    }

    /// Get the default audio variant
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

/// Determine the transcode decision for an audio codec
pub fn decide_transcode(codec_id: ffmpeg::codec::Id) -> TranscodeDecision {
    if is_hls_supported_codec(codec_id) {
        TranscodeDecision::Copy
    } else if is_audio_codec(codec_id) {
        TranscodeDecision::TranscodeToAac
    } else {
        TranscodeDecision::Exclude
    }
}

/// Get HLS codec string for an audio codec
pub fn get_codec_string(codec_id: ffmpeg::codec::Id) -> Option<String> {
    match codec_id {
        ffmpeg::codec::Id::AAC => Some("mp4a.40.2"),
        ffmpeg::codec::Id::AC3 => Some("ac-3"),
        ffmpeg::codec::Id::EAC3 => Some("ec-3"),
        ffmpeg::codec::Id::OPUS => Some("Opus"),
        ffmpeg::codec::Id::VORBIS => Some("vorbis"),
        ffmpeg::codec::Id::MP3 => Some("mp4a.40.34"),
        ffmpeg::codec::Id::FLAC => Some("flac"),
        _ => None,
    }
    .map(|s| s.to_string())
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
                    sample_rate: stream.sample_rate,
                    channels: stream.channels,
                    requires_transcode: false,
                    codec_string: get_codec_string(stream.codec_id),
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
                    sample_rate: stream.sample_rate,
                    channels: stream.channels,
                    requires_transcode: true,
                    codec_string: Some("mp4a.40.2".to_string()),
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
                    sample_rate: stream.sample_rate,
                    channels: stream.channels,
                    requires_transcode: true,
                    codec_string: Some("mp4a.40.2".to_string()),
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

/// Get the preferred audio codec for a language group
/// Priority: AAC > AC-3 > E-AC-3 > others
pub fn get_preferred_codec(streams: &[&AudioStreamInfo]) -> Option<ffmpeg::codec::Id> {
    // Prefer AAC
    if let Some(s) = streams.iter().find(|s| is_aac_codec(s.codec_id)) {
        return Some(s.codec_id);
    }
    // Then AC-3
    if let Some(s) = streams
        .iter()
        .find(|s| s.codec_id == ffmpeg::codec::Id::AC3)
    {
        return Some(s.codec_id);
    }
    // Then E-AC-3
    if let Some(s) = streams
        .iter()
        .find(|s| s.codec_id == ffmpeg::codec::Id::EAC3)
    {
        return Some(s.codec_id);
    }
    // Fall back to first audio stream
    streams.first().map(|s| s.codec_id)
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
    fn test_decide_transcode_aac() {
        assert_eq!(
            decide_transcode(ffmpeg::codec::Id::AAC),
            TranscodeDecision::Copy
        );
    }

    #[test]
    fn test_decide_transcode_ac3() {
        assert_eq!(
            decide_transcode(ffmpeg::codec::Id::AC3),
            TranscodeDecision::Copy
        );
    }

    #[test]
    fn test_decide_transcode_unknown() {
        assert_eq!(
            decide_transcode(ffmpeg::codec::Id::H264),
            TranscodeDecision::Exclude
        );
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
    fn test_get_codec_string() {
        assert_eq!(
            get_codec_string(ffmpeg::codec::Id::AAC),
            Some("mp4a.40.2".to_string())
        );
        assert_eq!(
            get_codec_string(ffmpeg::codec::Id::AC3),
            Some("ac-3".to_string())
        );
        assert_eq!(
            get_codec_string(ffmpeg::codec::Id::EAC3),
            Some("ec-3".to_string())
        );
    }

    #[test]
    fn test_is_hls_supported_codec() {
        assert!(is_hls_supported_codec(ffmpeg::codec::Id::AAC));
        assert!(is_hls_supported_codec(ffmpeg::codec::Id::AC3));
        assert!(is_hls_supported_codec(ffmpeg::codec::Id::OPUS));
    }

    #[test]
    fn test_get_preferred_codec() {
        let stream1 = create_test_audio_stream(0, ffmpeg::codec::Id::AC3, Some("en"));
        let stream2 = create_test_audio_stream(1, ffmpeg::codec::Id::AAC, Some("en"));
        let streams = vec![&stream1, &stream2];
        assert_eq!(get_preferred_codec(&streams), Some(ffmpeg::codec::Id::AAC));
    }
}
