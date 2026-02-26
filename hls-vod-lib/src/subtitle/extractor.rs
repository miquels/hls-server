#![allow(dead_code)]

//! Subtitle text extractor
//!
//! Extracts text content from subtitle packets and AVSubtitle structs.

use crate::error::Result;
use crate::ffmpeg_utils::ffmpeg;

/// A single subtitle cue with timing and text
#[derive(Debug, Clone)]
pub struct SubtitleCue {
    /// Start time in milliseconds
    pub start_ms: i64,
    /// End time in milliseconds
    pub end_ms: i64,
    /// Text content (may contain ASS/SSA markup)
    pub text: String,
}

impl SubtitleCue {
    /// Create a new subtitle cue
    pub fn new(start_ms: i64, end_ms: i64, text: String) -> Self {
        Self {
            start_ms,
            end_ms,
            text,
        }
    }

    /// Get the duration in milliseconds
    pub fn duration_ms(&self) -> i64 {
        self.end_ms - self.start_ms
    }
}

/// Subtitle extractor for converting packets to text cues
pub struct SubtitleExtractor {
    /// Codec ID
    codec_id: ffmpeg::codec::Id,
    /// Timebase for PTS conversion
    timebase: ffmpeg::Rational,
}

impl SubtitleExtractor {
    /// Create a new subtitle extractor
    pub fn new(codec_id: ffmpeg::codec::Id, timebase: ffmpeg::Rational) -> Self {
        Self { codec_id, timebase }
    }

    /// Convert PTS to milliseconds
    fn pts_to_ms(&self, pts: i64) -> i64 {
        let num = self.timebase.numerator() as i64;
        let den = self.timebase.denominator() as i64;
        (pts * num * 1000) / den
    }

    /// Extract subtitle cues from a packet
    ///
    /// This uses FFmpeg's subtitle decoding API to extract text
    /// from the subtitle packet.
    pub fn extract_cues(&self, packet: &ffmpeg::Packet) -> Result<Vec<SubtitleCue>> {
        // For text-based subtitles, we can extract text directly from the packet data
        // For ASS/SSA, we need to parse the ASS format

        let pts = packet.pts().unwrap_or(0);
        let duration = packet.duration();

        // Get the data from the packet (returns Option<&[u8]>)
        let data = packet.data().unwrap_or(&[]);

        match self.codec_id {
            ffmpeg::codec::Id::SUBRIP => {
                // SRT format - plain text with optional markup
                self.extract_srt_cues(data, pts, duration)
            }
            ffmpeg::codec::Id::ASS | ffmpeg::codec::Id::SSA => {
                // ASS/SSA format - contains style information
                self.extract_ass_cues(data, pts, duration)
            }
            ffmpeg::codec::Id::MOV_TEXT | ffmpeg::codec::Id::TEXT => {
                // MOV_TEXT (tx3g) packets have a 2-byte big-endian length prefix
                // followed by UTF-8 text, then optional binary style boxes ("styl" …).
                // Plain TEXT streams are clean UTF-8.
                if self.codec_id == ffmpeg::codec::Id::MOV_TEXT {
                    self.extract_mov_text_cues(data, pts, duration)
                } else {
                    self.extract_text_cues(data, pts, duration)
                }
            }
            ffmpeg::codec::Id::WEBVTT => {
                // Already WebVTT format
                self.extract_webvtt_cues(data, pts, duration)
            }
            _ => {
                // Unknown format - try to extract as plain text
                self.extract_text_cues(data, pts, duration)
            }
        }
    }

    /// Extract SRT subtitle cues
    fn extract_srt_cues(&self, data: &[u8], pts: i64, duration: i64) -> Result<Vec<SubtitleCue>> {
        let text = String::from_utf8_lossy(data).to_string();
        let start_ms = self.pts_to_ms(pts);
        let end_ms = if duration > 0 {
            start_ms + self.pts_to_ms(duration)
        } else {
            start_ms + 2000 // Default 2 second duration
        };

        Ok(vec![SubtitleCue::new(start_ms, end_ms, text)])
    }

    /// Extract ASS/SSA subtitle cues
    fn extract_ass_cues(&self, data: &[u8], pts: i64, duration: i64) -> Result<Vec<SubtitleCue>> {
        let text = String::from_utf8_lossy(data).to_string();
        let start_ms = self.pts_to_ms(pts);
        let end_ms = if duration > 0 {
            start_ms + self.pts_to_ms(duration)
        } else {
            start_ms + 2000
        };

        // ASS format may contain dialogue lines with format:
        // Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
        // We extract just the text portion
        let clean_text = self.clean_ass_text(&text);

        Ok(vec![SubtitleCue::new(start_ms, end_ms, clean_text)])
    }

    /// Extract plain text subtitle cues
    fn extract_text_cues(&self, data: &[u8], pts: i64, duration: i64) -> Result<Vec<SubtitleCue>> {
        let text = String::from_utf8_lossy(data).to_string();
        let start_ms = self.pts_to_ms(pts);
        let end_ms = if duration > 0 {
            start_ms + self.pts_to_ms(duration)
        } else {
            start_ms + 2000
        };

        Ok(vec![SubtitleCue::new(start_ms, end_ms, text)])
    }

    /// Extract MOV_TEXT (tx3g / 3GPP Timed Text) subtitle cues.
    ///
    /// Packet layout:
    /// ```text
    /// [0..2]  uint16_be  text length  (N)
    /// [2..2+N] UTF-8 text
    /// [2+N..]  optional binary style boxes (styl, hlit, etc.) — discard
    /// ```
    fn extract_mov_text_cues(
        &self,
        data: &[u8],
        pts: i64,
        duration: i64,
    ) -> Result<Vec<SubtitleCue>> {
        // Need at least 2 bytes for the length prefix
        if data.len() < 2 {
            return Ok(vec![]);
        }

        let text_len = u16::from_be_bytes([data[0], data[1]]) as usize;

        // Guard: if the declared length exceeds available bytes, clamp it
        let text_bytes = if 2 + text_len <= data.len() {
            &data[2..2 + text_len]
        } else {
            &data[2..]
        };

        let text = String::from_utf8_lossy(text_bytes).trim().to_string();

        // Empty cue (e.g. a blank subtitle event clearing the screen) → skip
        if text.is_empty() {
            return Ok(vec![]);
        }

        let start_ms = self.pts_to_ms(pts);
        let end_ms = if duration > 0 {
            start_ms + self.pts_to_ms(duration)
        } else {
            start_ms + 2000
        };

        Ok(vec![SubtitleCue::new(start_ms, end_ms, text)])
    }

    /// Extract WebVTT subtitle cues
    fn extract_webvtt_cues(
        &self,
        data: &[u8],
        pts: i64,
        duration: i64,
    ) -> Result<Vec<SubtitleCue>> {
        let text = String::from_utf8_lossy(data).to_string();
        let start_ms = self.pts_to_ms(pts);
        let end_ms = if duration > 0 {
            start_ms + self.pts_to_ms(duration)
        } else {
            start_ms + 2000
        };

        Ok(vec![SubtitleCue::new(start_ms, end_ms, text)])
    }

    /// Clean ASS/SSA text by removing style overrides
    fn clean_ass_text(&self, text: &str) -> String {
        // ASS format can contain style overrides like {\pos(100,200)}
        // We strip these for WebVTT output
        let mut result = String::new();
        let mut in_tag = false;

        for ch in text.chars() {
            if ch == '{' {
                in_tag = true;
            } else if ch == '}' {
                in_tag = false;
            } else if !in_tag {
                result.push(ch);
            }
        }

        result.trim().to_string()
    }
}

/// Extract subtitle text from packet data
pub fn extract_subtitle_text(packet: &ffmpeg::Packet) -> String {
    String::from_utf8_lossy(packet.data().unwrap_or(&[])).to_string()
}

/// Convert ASS/SSA style to WebVTT class
pub fn ass_style_to_webvtt_class(style: &str) -> String {
    // Convert ASS style names to valid WebVTT class names
    style
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subtitle_cue() {
        let cue = SubtitleCue::new(1000, 3000, "Hello World".to_string());
        assert_eq!(cue.start_ms, 1000);
        assert_eq!(cue.end_ms, 3000);
        assert_eq!(cue.text, "Hello World");
        assert_eq!(cue.duration_ms(), 2000);
    }

    #[test]
    fn test_extractor_creation() {
        let extractor =
            SubtitleExtractor::new(ffmpeg::codec::Id::SUBRIP, ffmpeg::Rational::new(1, 90000));
        assert_eq!(extractor.timebase.numerator(), 1);
        assert_eq!(extractor.timebase.denominator(), 90000);
    }

    #[test]
    fn test_pts_to_ms() {
        let extractor =
            SubtitleExtractor::new(ffmpeg::codec::Id::SUBRIP, ffmpeg::Rational::new(1, 90000));
        assert_eq!(extractor.pts_to_ms(90000), 1000);
        assert_eq!(extractor.pts_to_ms(45000), 500);
    }

    #[test]
    fn test_clean_ass_text() {
        let extractor =
            SubtitleExtractor::new(ffmpeg::codec::Id::ASS, ffmpeg::Rational::new(1, 90000));

        let ass_text = "{\\pos(100,200)}Hello{\\c&H00FFFF&} World";
        let cleaned = extractor.clean_ass_text(ass_text);
        assert_eq!(cleaned, "Hello World");
    }

    #[test]
    fn test_ass_style_to_webvtt_class() {
        assert_eq!(ass_style_to_webvtt_class("Default"), "Default");
        assert_eq!(ass_style_to_webvtt_class("My Style"), "My_Style");
        assert_eq!(ass_style_to_webvtt_class("Style-1"), "Style_1");
    }
}
