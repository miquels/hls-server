use crate::media::StreamIndex;
use crate::segment::generator::{
    generate_audio_init_segment, generate_audio_segment, generate_interleaved_init_segment,
    generate_interleaved_segment, generate_video_init_segment, generate_video_segment,
};
use std::path::PathBuf;

#[test]
fn test_dump_segments() {
    let asset = std::path::PathBuf::from("/Users/mikevs/Devel/hls-server/video-alex.mp4");

    if !asset.exists() {
        println!("Asset does not exist at {:?}", asset);
        return;
    }
    let media = StreamIndex::open(&asset, None).unwrap();

    // === VIDEO ONLY ===
    let video_init = generate_video_init_segment(&media).unwrap();
    std::fs::write("/tmp/vid_init.mp4", &video_init).unwrap();
    println!("Wrote video init segment: {} bytes", video_init.len());

    let video_bytes = generate_video_segment(&media, 0, 0, &asset).unwrap();
    std::fs::write("/tmp/vid0.mp4", &video_bytes).unwrap();
    println!("Wrote video segment 0: {} bytes", video_bytes.len());

    // === INTERLEAVED (AV, no transcoding) ===
    let video_idx = 0;
    let audio_idx = 2; // AC-3 is stream 2 in video-alex.mp4

    let av_init = generate_interleaved_init_segment(&media, video_idx, audio_idx, None).unwrap();
    std::fs::write("/tmp/av_init.mp4", &av_init).unwrap();
    println!("Wrote interleaved init segment: {} bytes", av_init.len());

    let seg0 = media.segments.get(0).unwrap();
    let av_bytes0 =
        generate_interleaved_segment(&media, video_idx, audio_idx, seg0, &asset, None).unwrap();
    std::fs::write("/tmp/av0.mp4", &av_bytes0).unwrap();

    let seg1 = media.segments.get(1).unwrap();
    let av_bytes1 =
        generate_interleaved_segment(&media, video_idx, audio_idx, seg1, &asset, None).unwrap();
    std::fs::write("/tmp/av1.mp4", &av_bytes1).unwrap();

    // Combine for ffprobe
    let mut combined = av_init.to_vec();
    combined.extend_from_slice(&av_bytes0);
    combined.extend_from_slice(&av_bytes1);
    std::fs::write("/tmp/av_combined.mp4", &combined).unwrap();
    println!("Wrote combined interleaved: {} bytes", combined.len());
}
