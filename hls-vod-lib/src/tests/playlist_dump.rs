use crate::media::StreamIndex;
use std::path::PathBuf;

#[test]
fn debug_index_video_alex() {
    let video_path = PathBuf::from("/Users/mikevs/Devel/hls-server/video-alex.mp4");
    if !video_path.exists() {
        println!("File not found: {:?}", video_path);
        return;
    }

    let index = StreamIndex::open(&video_path, None).expect("Failed to scan file");

    println!("Stream ID: {}", index.stream_id);
    println!("Duration: {} s", index.duration_secs);
    println!("Video streams: {}", index.video_streams.len());
    println!("Audio streams: {}", index.audio_streams.len());
    println!("Segments: {}", index.segments.len());

    for (i, v) in index.video_streams.iter().enumerate() {
        println!(
            "Video {}: codec={:?}, bitrate={}, fps={:?}",
            i, v.codec_id, v.bitrate, v.framerate
        );
    }

    for (i, a) in index.audio_streams.iter().enumerate() {
        println!(
            "Audio {}: codec={:?}, bitrate={}, rate={}, lang={:?}",
            i, a.codec_id, a.bitrate, a.sample_rate, a.language
        );
    }

    if let Some(first) = index.segments.first() {
        println!(
            "Seg 0: start_pts={}, end_pts={}, duration={}",
            first.start_pts, first.end_pts, first.duration_secs
        );
    }

    if let Some(last) = index.segments.last() {
        println!(
            "Last Seg: start_pts={}, end_pts={}, duration={}",
            last.start_pts, last.end_pts, last.duration_secs
        );
        let total_duration = (last.end_pts as f64) * index.video_timebase.numerator() as f64
            / index.video_timebase.denominator() as f64;
        println!(
            "Total calculated duration from segments: {} s",
            total_duration
        );
    }
}
