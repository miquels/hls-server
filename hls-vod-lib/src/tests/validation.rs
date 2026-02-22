//! Playlist and segment validation utilities

/// Validate master playlist structure
pub fn validate_master_playlist(content: &str) -> ValidationResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // Check for required tags
    if !content.contains("#EXTM3U") {
        errors.push("Missing #EXTM3U header".to_string());
    }

    if !content.contains("#EXT-X-VERSION") {
        errors.push("Missing #EXT-X-VERSION tag".to_string());
    } else {
        // Check version is 7 or higher for HLS with fMP4
        if let Some(version_line) = content.lines().find(|l| l.contains("#EXT-X-VERSION")) {
            if let Some(version) = version_line.split(':').nth(1) {
                if let Ok(v) = version.trim().parse::<u32>() {
                    if v < 7 {
                        warnings.push(format!("HLS version {} may not support fMP4", v));
                    }
                }
            }
        }
    }

    // Check for audio/subtitle media entries if present
    let _has_audio = content.contains("TYPE=AUDIO");
    let _has_subtitles = content.contains("TYPE=SUBTITLES");
    let has_video = content.contains("#EXT-X-STREAM-INF");

    if !has_video {
        errors.push("No video stream variants found".to_string());
    }

    // Validate AUDIO entries have required attributes
    for line in content.lines() {
        if line.starts_with("#EXT-X-MEDIA:TYPE=AUDIO") {
            if !line.contains("GROUP-ID=") {
                errors.push("AUDIO entry missing GROUP-ID".to_string());
            }
            if !line.contains("LANGUAGE=") {
                errors.push("AUDIO entry missing LANGUAGE".to_string());
            }
            if !line.contains("NAME=") {
                errors.push("AUDIO entry missing NAME".to_string());
            }
            if !line.contains("URI=") {
                errors.push("AUDIO entry missing URI".to_string());
            }
        }

        if line.starts_with("#EXT-X-MEDIA:TYPE=SUBTITLES") {
            if !line.contains("GROUP-ID=") {
                errors.push("SUBTITLES entry missing GROUP-ID".to_string());
            }
            if !line.contains("URI=") {
                errors.push("SUBTITLES entry missing URI".to_string());
            }
        }
    }

    // Validate STREAM-INF entries
    for line in content.lines() {
        if line.starts_with("#EXT-X-STREAM-INF") {
            if !line.contains("BANDWIDTH=") {
                errors.push("STREAM-INF missing BANDWIDTH".to_string());
            }
            if !line.contains("RESOLUTION=") {
                errors.push("STREAM-INF missing RESOLUTION".to_string());
            }
        }
    }

    ValidationResult {
        is_valid: errors.is_empty(),
        errors,
        warnings,
    }
}

/// Validate variant playlist (video/audio/subtitle)
pub fn validate_variant_playlist(content: &str, _playlist_type: PlaylistType) -> ValidationResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // Check for required tags
    if !content.contains("#EXTM3U") {
        errors.push("Missing #EXTM3U header".to_string());
    }

    if !content.contains("#EXT-X-VERSION") {
        errors.push("Missing #EXT-X-VERSION tag".to_string());
    }

    if !content.contains("#EXT-X-TARGETDURATION") {
        errors.push("Missing #EXT-X-TARGETDURATION tag".to_string());
    }

    if !content.contains("#EXT-X-MEDIA-SEQUENCE") {
        errors.push("Missing #EXT-X-MEDIA-SEQUENCE tag".to_string());
    }

    // Check for VOD playlist type
    if !content.contains("#EXT-X-PLAYLIST-TYPE:VOD") {
        warnings.push("Missing #EXT-X-PLAYLIST-TYPE:VOD (may be live stream)".to_string());
    }

    // Check for end list
    if !content.contains("#EXT-X-ENDLIST") {
        errors.push("Missing #EXT-X-ENDLIST tag".to_string());
    }

    // Count segment entries
    let segment_count = content.matches("#EXTINF:").count();
    if segment_count == 0 {
        errors.push("No segment entries found".to_string());
    }

    // Validate segment entries have corresponding URIs
    let mut has_extinf = false;
    for line in content.lines() {
        if line.starts_with("#EXTINF:") {
            has_extinf = true;
        } else if has_extinf && !line.starts_with('#') && !line.trim().is_empty() {
            // This should be a URI
            has_extinf = false;
        }
    }

    // Check codec declarations if present
    if content.contains("#EXT-X-CODECS") {
        // Validate codec string format
        for line in content.lines() {
            if line.starts_with("#EXT-X-CODECS:") {
                let codecs = line.trim_start_matches("#EXT-X-CODECS:");
                if codecs.is_empty() {
                    errors.push("Empty CODECS attribute".to_string());
                }
            }
        }
    }

    ValidationResult {
        is_valid: errors.is_empty(),
        errors,
        warnings,
    }
}

/// Validate WebVTT subtitle file
pub fn validate_webvtt(content: &str) -> ValidationResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    // Check for WEBVTT header
    if !content.starts_with("WEBVTT") {
        errors.push("Missing WEBVTT header".to_string());
    }

    // Check for X-TIMESTAMP-MAP (required for HLS sync)
    if !content.contains("X-TIMESTAMP-MAP") {
        warnings.push("Missing X-TIMESTAMP-MAP (HLS sync may be affected)".to_string());
    } else {
        // Validate X-TIMESTAMP-MAP format
        for line in content.lines() {
            if line.starts_with("X-TIMESTAMP-MAP") {
                if !line.contains("LOCAL:") {
                    errors.push("X-TIMESTAMP-MAP missing LOCAL timestamp".to_string());
                }
                if !line.contains("MPEGTS:") {
                    errors.push("X-TIMESTAMP-MAP missing MPEGTS timestamp".to_string());
                }
            }
        }
    }

    // Validate cue timestamps
    let timestamp_pattern = regex::Regex::new(r"\d{2}:\d{2}:\d{2}\.\d{3}").unwrap();
    for line in content.lines() {
        if line.contains("-->") {
            if !timestamp_pattern.is_match(line) {
                errors.push(format!("Invalid cue timestamp format: {}", line));
            }
        }
    }

    // Check for HTML entity escaping
    if content.contains('<') && !content.contains("&lt;") {
        warnings.push("Unescaped '<' character found (should be &lt;)".to_string());
    }
    if content.contains('>') && !content.contains("&gt;") && !content.contains("-->") {
        warnings.push("Unescaped '>' character found (should be &gt;)".to_string());
    }

    ValidationResult {
        is_valid: errors.is_empty(),
        errors,
        warnings,
    }
}

/// Validate fMP4 segment structure
#[allow(dead_code)]
pub fn validate_fmp4_segment(data: &[u8]) -> ValidationResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if data.len() < 8 {
        errors.push("Segment too small".to_string());
        return ValidationResult {
            is_valid: false,
            errors,
            warnings,
        };
    }

    // Check for valid box at start
    let box_type = &data[4..8];
    if box_type != b"ftyp" && box_type != b"moov" && box_type != b"moof" {
        errors.push(format!(
            "Invalid box type at start: {:?}",
            std::str::from_utf8(box_type)
        ));
    }

    // Check for required boxes
    let has_ftyp = find_box(data, b"ftyp").is_some();
    let has_moov_or_moof = find_box(data, b"moov").is_some() || find_box(data, b"moof").is_some();

    if !has_ftyp {
        warnings.push("Missing ftyp box".to_string());
    }

    if !has_moov_or_moof {
        errors.push("Missing moov or moof box".to_string());
    }

    ValidationResult {
        is_valid: errors.is_empty(),
        errors,
        warnings,
    }
}

/// Find a specific box in MP4 data
fn find_box(data: &[u8], box_type: &[u8]) -> Option<usize> {
    let mut pos = 0;
    while pos + 8 <= data.len() {
        let size =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;

        if size < 8 {
            return None;
        }

        if pos + 8 > data.len() {
            return None;
        }

        let current_type = &data[pos + 4..pos + 8];
        if current_type == box_type {
            return Some(pos);
        }

        pos += size;
    }
    None
}

/// Type of playlist
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlaylistType {
    #[allow(dead_code)]
    Master,
    Video,
    Audio,
    Subtitle,
}

/// Validation result
#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub errors: Vec<String>,
    #[allow(dead_code)]
    pub warnings: Vec<String>,
}

impl ValidationResult {
    pub fn success() -> Self {
        Self {
            is_valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }

    pub fn fail(error: impl Into<String>) -> Self {
        Self {
            is_valid: false,
            errors: vec![error.into()],
            warnings: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_valid_master_playlist() {
        let content = r#"#EXTM3U
#EXT-X-VERSION:7
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID="audio-en",LANGUAGE="en",NAME="English",URI="audio_en.m3u8"
#EXT-X-STREAM-INF:BANDWIDTH=5500000,RESOLUTION=1920x1080,CODECS="avc1.42001e,mp4a.40.2"
video.m3u8
"#;
        let result = validate_master_playlist(content);
        assert!(result.is_valid);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn test_validate_invalid_master_playlist() {
        let content = "invalid content";
        let result = validate_master_playlist(content);
        assert!(!result.is_valid);
        assert!(!result.errors.is_empty());
    }

    #[test]
    fn test_validate_variant_playlist() {
        let content = r#"#EXTM3U
#EXT-X-VERSION:7
#EXT-X-TARGETDURATION:6
#EXT-X-MEDIA-SEQUENCE:0
#EXT-X-PLAYLIST-TYPE:VOD
#EXTINF:4.000,
segment_0.m4s
#EXT-X-ENDLIST
"#;
        let result = validate_variant_playlist(content, PlaylistType::Video);
        assert!(result.is_valid);
    }

    #[test]
    fn test_validate_webvtt() {
        let content = r#"WEBVTT

X-TIMESTAMP-MAP=LOCAL:00:00:00.000,MPEGTS:9000000000

00:00:01.000 --> 00:00:03.000
Hello World
"#;
        let result = validate_webvtt(content);
        assert!(result.is_valid);
    }

    #[test]
    fn test_validate_webvtt_missing_header() {
        let content = "00:00:01.000 --> 00:00:03.000\nHello";
        let result = validate_webvtt(content);
        assert!(!result.is_valid);
    }

    #[test]
    fn test_find_box() {
        let mut data = vec![0; 24];
        data[0..4].copy_from_slice(&24u32.to_be_bytes());
        data[4..8].copy_from_slice(b"ftyp");

        assert!(find_box(&data, b"ftyp").is_some());
        assert!(find_box(&data, b"moov").is_none());
    }

    #[test]
    fn test_validation_result() {
        let success = ValidationResult::success();
        assert!(success.is_valid);
        assert!(success.errors.is_empty());

        let fail = ValidationResult::fail("test error");
        assert!(!fail.is_valid);
        assert_eq!(fail.errors.len(), 1);
    }
}
