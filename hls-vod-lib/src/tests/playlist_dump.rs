use crate::media::StreamIndex;
use crate::playlist::master::generate_master_playlist;
use crate::playlist::variant::{generate_audio_playlist, generate_video_playlist};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

#[test]
fn dump_playlists_full_video_alex() {
    let video_path = PathBuf::from("/Users/mikevs/Devel/hls-server/video-alex.mp4");
    if !video_path.exists() {
        println!("File not found: {:?}", video_path);
        return;
    }

    let index = StreamIndex::open(&video_path, None).expect("Failed to scan file");

    let video_url = "video-alex.mp4";
    let session_id = Some("debug-session");
    let codecs = Vec::new();

    // IMPORTANT: Enable all tracks to see the full playlist
    let mut tracks_enabled = HashSet::new();
    for v in &index.video_streams {
        tracks_enabled.insert(v.stream_index);
    }
    for a in &index.audio_streams {
        tracks_enabled.insert(a.stream_index);
    }
    for s in &index.subtitle_streams {
        tracks_enabled.insert(s.stream_index);
    }

    let transcode = HashMap::new();
    let interleaved = false;

    println!("--- MASTER PLAYLIST ---");
    let master = generate_master_playlist(
        &index,
        video_url,
        session_id,
        &codecs,
        &tracks_enabled,
        &transcode,
        interleaved,
    );
    println!("{}", master);

    println!("--- VIDEO PLAYLIST (HEAD) ---");
    let video_playlist = generate_video_playlist(&index);
    for line in video_playlist.lines().take(20) {
        println!("{}", line);
    }
}
