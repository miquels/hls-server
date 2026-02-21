//! End-to-end integration tests

use std::sync::Arc;

use crate::config::ServerConfig;
use crate::integration::fixtures::TestMediaInfo;
use crate::integration::validation::{
    validate_master_playlist, validate_variant_playlist, validate_webvtt, PlaylistType,
    ValidationResult,
};
use crate::playlist::{
    generate_audio_playlist, generate_master_playlist, generate_subtitle_playlist,
    generate_video_playlist,
};
use crate::segment::generate_init_segment;
use crate::state::AppState;

/// Test the complete stream lifecycle
pub fn test_stream_lifecycle() -> ValidationResult {
    // Use a real asset for the complete lifecycle test
    let mut asset_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    asset_path.push("testvideos");
    asset_path.push("bun33s.mp4");

    if !asset_path.exists() {
        return ValidationResult::success(); // Skip if asset missing
    }

    let index = crate::index::scanner::scan_file(&asset_path).expect("Failed to scan test asset");

    // Create app state and register stream
    let state = Arc::new(AppState::new(ServerConfig::default()));
    let stream_id = index.stream_id.clone();
    state.register_stream(index);

    // Verify stream is registered
    if state.get_stream(&stream_id).is_none() {
        return ValidationResult::fail("Failed to register stream");
    }

    // Generate and validate master playlist
    let master = generate_master_playlist(&state.get_stream(&stream_id).unwrap());
    let master_result = validate_master_playlist(&master);
    if !master_result.is_valid {
        return master_result;
    }

    // Generate and validate video playlist
    let video = generate_video_playlist(&state.get_stream(&stream_id).unwrap());
    let video_result = validate_variant_playlist(&video, PlaylistType::Video);
    if !video_result.is_valid {
        return video_result;
    }

    // Generate init segment
    let init_result = generate_init_segment(&state.get_stream(&stream_id).unwrap());
    if init_result.is_err() {
        return ValidationResult::fail(format!(
            "Failed to generate init segment: {}",
            init_result.unwrap_err()
        ));
    }

    ValidationResult::success()
}

/// Test playlist generation for various configurations
pub fn test_playlist_generation() -> Vec<(&'static str, ValidationResult)> {
    let mut results = Vec::new();

    // Test AAC-only configuration
    {
        let fixture = TestMediaInfo::aac_only();
        let index = fixture.create_mock_index();
        let master = generate_master_playlist(&index);
        let result = validate_master_playlist(&master);
        results.push(("AAC-only master playlist", result));
    }

    // Test AC-3 configuration
    {
        let fixture = TestMediaInfo::ac3_only();
        let index = fixture.create_mock_index();
        let master = generate_master_playlist(&index);
        let result = validate_master_playlist(&master);
        results.push(("AC-3 master playlist", result));
    }

    // Test multi-audio configuration
    {
        let fixture = TestMediaInfo::multi_audio();
        let index = fixture.create_mock_index();
        let master = generate_master_playlist(&index);
        let result = validate_master_playlist(&master);
        results.push(("Multi-audio master playlist", result));
    }

    // Test with subtitles
    {
        let fixture = TestMediaInfo::with_subtitles();
        let index = fixture.create_mock_index();
        let master = generate_master_playlist(&index);
        let result = validate_master_playlist(&master);
        results.push(("With subtitles master playlist", result));
    }

    // Test multi-language
    {
        let fixture = TestMediaInfo::multi_language();
        let index = fixture.create_mock_index();
        let master = generate_master_playlist(&index);
        let result = validate_master_playlist(&master);
        results.push(("Multi-language master playlist", result));
    }

    results
}

/// Test audio track switching
pub fn test_audio_track_switching() -> ValidationResult {
    let fixture = TestMediaInfo::multi_audio();
    let index = fixture.create_mock_index();

    // Generate master playlist
    let master = generate_master_playlist(&index);

    // Verify multiple audio tracks are present
    let audio_count = master.matches("TYPE=AUDIO").count();
    if audio_count < 2 {
        return ValidationResult::fail(format!(
            "Expected at least 2 audio tracks, found {}",
            audio_count
        ));
    }

    // Verify different languages
    if !master.contains("LANGUAGE=\"en\"") {
        return ValidationResult::fail("Missing English audio track");
    }
    if !master.contains("LANGUAGE=\"es\"") {
        return ValidationResult::fail("Missing Spanish audio track");
    }

    // Generate audio playlists for each language
    for track_idx in [1, 2] {
        let audio_playlist = generate_audio_playlist(&index, track_idx, false);
        let result = validate_variant_playlist(&audio_playlist, PlaylistType::Audio);
        if !result.is_valid {
            return ValidationResult::fail(format!(
                "Invalid {} audio playlist: {:?}",
                track_idx, result.errors
            ));
        }
    }

    ValidationResult::success()
}

/// Test subtitle synchronization
pub fn test_subtitle_sync() -> ValidationResult {
    let fixture = TestMediaInfo::with_subtitles();
    let index = fixture.create_mock_index();

    // Generate subtitle playlist
    let sub_idx = index
        .subtitle_streams
        .first()
        .map(|s| s.stream_index)
        .unwrap_or(2);
    let sub_playlist = generate_subtitle_playlist(&index, sub_idx);
    let playlist_result = validate_variant_playlist(&sub_playlist, PlaylistType::Subtitle);
    if !playlist_result.is_valid {
        return playlist_result;
    }

    // Verify segment references are valid
    // Note: With empty segment merging, the mock segments created in `TestMediaInfo`
    // are all populated as `non_empty`, so they do not combine but appear as `X.start-end.vtt`.
    if !sub_playlist.contains(&format!("{}.0-0.vtt", sub_idx)) {
        return ValidationResult::fail(format!(
            "Missing subtitle segment reference, got: \n{}",
            sub_playlist
        ));
    }

    // Generate mock WebVTT content and validate
    let webvtt_content = r#"WEBVTT

X-TIMESTAMP-MAP=LOCAL:00:00:00.000,MPEGTS:9000000000

00:00:00.000 --> 00:00:04.000
Test subtitle segment
"#;
    let webvtt_result = validate_webvtt(webvtt_content);
    if !webvtt_result.is_valid {
        return webvtt_result;
    }

    ValidationResult::success()
}

/// Performance benchmark for playlist generation
pub fn benchmark_playlist_generation(iterations: usize) -> BenchmarkResult {
    use std::time::Instant;

    let fixture = TestMediaInfo::multi_language();
    let index = fixture.create_mock_index();

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = generate_master_playlist(&index);
        let _ = generate_video_playlist(&index);
        let _ = generate_audio_playlist(&index, 1, false);
        let _ = generate_audio_playlist(&index, 2, false);
    }
    let duration = start.elapsed();

    BenchmarkResult {
        name: "Playlist Generation",
        iterations,
        duration_ms: duration.as_millis() as u64,
        avg_ms: (duration.as_millis() as f64 / iterations as f64) as u64,
    }
}

/// Performance benchmark for segment generation
pub fn benchmark_segment_generation(iterations: usize) -> BenchmarkResult {
    use std::time::Instant;

    let fixture = TestMediaInfo::aac_only();
    let index = fixture.create_mock_index();

    let start = Instant::now();
    for _ in 0..iterations {
        let _ = generate_init_segment(&index);
    }
    let duration = start.elapsed();

    BenchmarkResult {
        name: "Init Segment Generation",
        iterations,
        duration_ms: duration.as_millis() as u64,
        avg_ms: (duration.as_millis() as f64 / iterations as f64) as u64,
    }
}

/// Benchmark result
#[derive(Debug, Clone)]
pub struct BenchmarkResult {
    pub name: &'static str,
    pub iterations: usize,
    pub duration_ms: u64,
    pub avg_ms: u64,
}

impl std::fmt::Display for BenchmarkResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {} iterations in {}ms (avg: {}ms)",
            self.name, self.iterations, self.duration_ms, self.avg_ms
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_lifecycle_e2e() {
        let result = test_stream_lifecycle();
        assert!(
            result.is_valid,
            "Stream lifecycle test failed: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_playlist_generation_all_configs() {
        let results = test_playlist_generation();
        for (name, result) in results {
            assert!(result.is_valid, "{} failed: {:?}", name, result.errors);
        }
    }

    #[test]
    fn test_audio_track_switching_e2e() {
        let result = test_audio_track_switching();
        assert!(
            result.is_valid,
            "Audio track switching test failed: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_subtitle_sync_e2e() {
        let result = test_subtitle_sync();
        assert!(
            result.is_valid,
            "Subtitle sync test failed: {:?}",
            result.errors
        );
    }

    #[test]
    fn test_benchmark_playlist_generation() {
        let result = benchmark_playlist_generation(100);
        println!("{}", result);
        // Should complete in reasonable time (< 100ms avg)
        assert!(
            result.avg_ms < 100,
            "Playlist generation too slow: {}ms avg",
            result.avg_ms
        );
    }

    #[test]
    fn test_benchmark_segment_generation() {
        let result = benchmark_segment_generation(100);
        println!("{}", result);
        // Should complete in reasonable time (< 50ms avg)
        assert!(
            result.avg_ms < 50,
            "Segment generation too slow: {}ms avg",
            result.avg_ms
        );
    }
}
