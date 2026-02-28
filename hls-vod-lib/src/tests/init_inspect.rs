use crate::media::StreamIndex;
use std::path::PathBuf;

#[test]
fn inspect_init_segments_video_alex() {
    let video_path = PathBuf::from("/Users/mikevs/Devel/hls-server/video-alex.mp4");
    if !video_path.exists() {
        println!("File not found: {:?}", video_path);
        return;
    }

    let index = StreamIndex::open(&video_path, None).expect("Failed to scan file");
    let video_idx = index.primary_video().unwrap().stream_index;

    println!("Generating Video Init Segment...");
    let video_init = crate::segment::generator::generate_video_init_segment(&index)
        .expect("Failed to generate video init");

    inspect_boxes("Video Init", &video_init);

    if let Some(audio) = index.audio_streams.get(0) {
        println!(
            "Generating Audio Init Segment (track {})...",
            audio.stream_index
        );
        let audio_init =
            crate::segment::generator::generate_audio_init_segment(&index, audio.stream_index)
                .expect("Failed to generate audio init");
        inspect_boxes("Audio Init", &audio_init);
    }
}

fn inspect_boxes(name: &str, data: &[u8]) {
    println!("--- {} Boxes ---", name);
    // Search for interesting boxes
    for box_name in &[b"mvhd", b"tkhd", b"mdhd", b"hdlr", b"elst", b"edts"] {
        let mut start = 0;
        while let Some(pos) = data[start..].windows(4).position(|w| w == *box_name) {
            let actual_pos = start + pos;
            println!(
                "Found {:?} at offset {}",
                std::str::from_utf8(*box_name).unwrap(),
                actual_pos
            );
            // Print some bytes of the box
            let end = (actual_pos + 40).min(data.len());
            println!("  Data: {:02x?}", &data[actual_pos..end]);
            start = actual_pos + 4;
        }
    }
}
