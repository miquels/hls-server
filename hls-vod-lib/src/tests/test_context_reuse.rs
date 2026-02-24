#[cfg(test)]
mod tests {
    use ffmpeg_next as ffmpeg;
    use std::sync::{Arc, Mutex};
    use std::thread;

    #[test]
    fn test_context_reuse() {
        ffmpeg::init().unwrap();
        let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.pop(); // Go up to workspace root since testvideos is not inside hls-vod-lib
        path.push("tests");
        path.push("assets");
        path.push("bun33s.mp4");
        let context = ffmpeg::format::input(&path).unwrap();
        let shared_ctx = Arc::new(Mutex::new(context));

        let mut handles = vec![];
        for i in 0..5 {
            let ctx_clone = shared_ctx.clone();
            handles.push(thread::spawn(move || {
                let mut ctx = ctx_clone.lock().unwrap();
                // Seek to a random position
                let stream_idx = 0;
                let target_ts = i * 10000;
                ctx.seek(target_ts, 0..target_ts).unwrap();
                let mut pkts = 0;
                for (stream, _packet) in ctx.packets() {
                    if stream.index() == stream_idx {
                        pkts += 1;
                        if pkts > 10 {
                            break;
                        }
                    }
                }
                pkts
            }));
        }

        for h in handles {
            let pkts = h.join().unwrap();
            assert!(pkts > 0);
        }
    }
}
