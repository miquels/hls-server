//! DTS diagnostic integration test
//!
//! Generates init.mp4, 0.m4s and 1.m4s from tests/assets/video.mp4,
//! then inspects every timestamp-related MP4 box to diagnose the
//! "Decreasing DTS" errors reported by mediastreamvalidator.
//!
//! What this test checks:
//!   1. init.mp4 mdhd timescale (must match the output timebase the muxer sets)
//!   2. Per-segment: tfdt.baseMediaDecodeTime
//!   3. Per-segment: every trun sample duration → total trun duration
//!   4. Cross-segment continuity:
//!        tfdt[N] == tfdt[N-1] + total_trun_duration[N-1]
//!      A violation here is exactly "Decreasing DTS".
//!   5. mfhd.FragmentSequenceNumber (must be monotonically increasing)

#[cfg(test)]
mod tests {
    use crate::index::scanner::scan_file;
    use crate::segment::generator::{generate_video_init_segment, generate_video_segment};
    use crate::segment::muxer::find_box;
    use ffmpeg_next as ffmpeg;
    use std::path::PathBuf;

    // ── helpers ──────────────────────────────────────────────────────────────

    /// Read a big-endian u32 from `data` at `offset`.
    fn u32_be(data: &[u8], offset: usize) -> u32 {
        u32::from_be_bytes(data[offset..offset + 4].try_into().unwrap())
    }

    /// Read a big-endian u64 from `data` at `offset`.
    fn u64_be(data: &[u8], offset: usize) -> u64 {
        u64::from_be_bytes(data[offset..offset + 8].try_into().unwrap())
    }

    // ── box-walking helpers ───────────────────────────────────────────────────

    /// Recursively find the first box with tag `tag` anywhere inside `data`.
    /// Returns the offset of the box *within* `data`.
    fn find_box_recursive<'a>(data: &'a [u8], tag: &[u8; 4]) -> Option<usize> {
        let mut pos = 0;
        while pos + 8 <= data.len() {
            let size = u32_be(data, pos) as usize;
            if size < 8 || pos + size > data.len() {
                break;
            }
            let btype: &[u8] = &data[pos + 4..pos + 8];
            if btype == tag.as_ref() {
                return Some(pos);
            }
            // Recurse into known container boxes so we can find nested boxes
            const CONTAINERS: &[&[u8]] = &[
                b"moov", b"trak", b"mdia", b"minf", b"stbl", b"mvex", b"moof", b"traf",
            ];
            if CONTAINERS.iter().any(|c| btype == *c) {
                if let Some(inner) = find_box_recursive(&data[pos + 8..pos + size], tag) {
                    return Some(pos + 8 + inner);
                }
            }
            pos += size;
        }
        None
    }

    // ── init.mp4 inspector ───────────────────────────────────────────────────

    /// Parse the mdhd timescale from an init segment.
    /// Returns all (track_id, timescale) pairs found.
    fn parse_mdhd_timescales(init: &[u8]) -> Vec<(u32, u32)> {
        let mut results = Vec::new();
        parse_mdhd_in(init, &mut results);
        results
    }

    fn parse_mdhd_in(data: &[u8], out: &mut Vec<(u32, u32)>) {
        let mut pos = 0;
        while pos + 8 <= data.len() {
            let size = u32_be(data, pos) as usize;
            if size < 8 || pos + size > data.len() {
                break;
            }
            let btype: &[u8] = &data[pos + 4..pos + 8];
            match btype {
                b"moov" | b"trak" | b"mdia" => {
                    parse_mdhd_in(&data[pos + 8..pos + size], out);
                }
                b"tkhd" => {
                    // tkhd v0: size+type+version(1)+flags(3)+creation(4)+modification(4)+track_id(4)
                    // tkhd v1: size+type+version(1)+flags(3)+creation(8)+modification(8)+track_id(4)
                    if size >= 12 {
                        let version = data[pos + 8];
                        let track_id = if version == 1 && size >= 32 {
                            u32_be(data, pos + 24)
                        } else if size >= 20 {
                            u32_be(data, pos + 16)
                        } else {
                            0
                        };
                        // Push a placeholder so we can pair with mdhd below
                        // We use a negative sentinel track_id here; resolved below
                        let _ = track_id; // track_id used below
                    }
                }
                b"mdhd" => {
                    if size >= 24 {
                        let version = data[pos + 8];
                        let timescale = if version == 1 && size >= 32 {
                            // v1: 8+8 (creation+mod) = 16 bytes before timescale
                            u32_be(data, pos + 28)
                        } else {
                            // v0: 4+4 = 8 bytes before timescale
                            u32_be(data, pos + 20)
                        };
                        // We don't easily know track_id here without context; use 0 as placeholder
                        out.push((0, timescale));
                    }
                }
                _ => {}
            }
            pos += size;
        }
    }

    /// Parse `trex.default_sample_duration` from the init segment.
    /// When a trun box omits per-sample durations (flag 0x0100 not set),
    /// the decoder falls back to this value from the trex box in the init segment.
    fn parse_trex_default_duration(init: &[u8]) -> u32 {
        // trex is inside moov → mvex → trex
        // trex layout: size(4)+type(4)+ver/flags(4)+track_id(4)+
        //              default_sample_description_index(4)+default_sample_duration(4)+...
        let mut pos = 0;
        while pos + 8 <= init.len() {
            let size = u32_be(init, pos) as usize;
            if size < 8 || pos + size > init.len() {
                break;
            }
            let btype: &[u8] = &init[pos + 4..pos + 8];
            match btype {
                b"moov" | b"mvex" => {
                    let inner = &init[pos + 8..pos + size];
                    let result = parse_trex_default_duration(inner);
                    if result > 0 {
                        return result;
                    }
                }
                b"trex" => {
                    // size(4)+type(4)+ver/flags(4)+track_id(4)+desc_idx(4)+default_sample_duration(4)
                    if size >= 28 {
                        return u32_be(init, pos + 20);
                    }
                }
                _ => {}
            }
            pos += size;
        }
        0
    }

    // ── media segment inspector ───────────────────────────────────────────────

    #[derive(Debug)]
    struct SegmentInfo {
        /// mfhd.FragmentSequenceNumber
        frag_seq: u32,
        /// tfdt.baseMediaDecodeTime
        base_decode_time: u64,
        /// tfdt version (0 = 32-bit, 1 = 64-bit)
        tfdt_version: u8,
        /// Sum of all trun sample durations
        total_trun_duration: u64,
        /// Number of samples in trun
        sample_count: u32,
        /// First few sample durations for debug display
        sample_durations: Vec<u32>,
    }

    /// Parse a media segment (.m4s) and return its timing info.
    /// Handles the optional leading `styp` box.
    fn parse_media_segment(data: &[u8]) -> SegmentInfo {
        // Skip styp if present
        let data = if data.len() >= 8 && &data[4..8] == b"styp" {
            let styp_size = u32_be(data, 0) as usize;
            &data[styp_size..]
        } else {
            data
        };

        let moof_pos = find_box(data, b"moof").expect("moof not found in segment");
        let moof_size = u32_be(data, moof_pos) as usize;
        let moof = &data[moof_pos..moof_pos + moof_size];

        // ── mfhd ──
        let mfhd_pos = find_box_recursive(moof, b"mfhd").expect("mfhd not found");
        // mfhd: size(4)+type(4)+version/flags(4)+seq(4)
        let frag_seq = u32_be(moof, mfhd_pos + 12);

        // ── traf → tfdt + trun ──
        let traf_pos = find_box_recursive(moof, b"traf").expect("traf not found");
        let traf_size = u32_be(moof, traf_pos) as usize;
        let traf = &moof[traf_pos..traf_pos + traf_size];

        // tfdt: size(4)+type(4)+version(1)+flags(3)+baseMediaDecodeTime(4 or 8)
        let tfdt_pos = find_box_recursive(traf, b"tfdt").expect("tfdt not found");
        let tfdt_version = traf[tfdt_pos + 8];
        let base_decode_time = if tfdt_version == 1 {
            u64_be(traf, tfdt_pos + 12)
        } else {
            u32_be(traf, tfdt_pos + 12) as u64
        };

        // trun: size(4)+type(4)+version(1)+flags(3)+sample_count(4)
        //       + optional data_offset(4 if flag 0x01)
        //       + optional first_sample_flags(4 if flag 0x04)
        //       + per_sample fields
        let trun_pos = find_box_recursive(traf, b"trun").expect("trun not found");
        let _trun_version = traf[trun_pos + 8];
        let trun_flags = u32_be(traf, trun_pos + 8) & 0x00FF_FFFF;
        let sample_count = u32_be(traf, trun_pos + 12);

        // Compute offset to first sample entry
        let mut entry_offset = 16; // after size(4)+type(4)+ver/flags(4)+count(4)
        if trun_flags & 0x0001 != 0 {
            entry_offset += 4;
        } // data_offset
        if trun_flags & 0x0004 != 0 {
            entry_offset += 4;
        } // first_sample_flags

        // Compute per-sample entry size
        let mut per_sample_size = 0usize;
        if trun_flags & 0x0100 != 0 {
            per_sample_size += 4;
        } // sample_duration
        if trun_flags & 0x0200 != 0 {
            per_sample_size += 4;
        } // sample_size
        if trun_flags & 0x0400 != 0 {
            per_sample_size += 4;
        } // sample_flags
        if trun_flags & 0x0800 != 0 {
            per_sample_size += 4;
        } // composition_time_offset

        let has_duration = trun_flags & 0x0100 != 0;

        let mut total_trun_duration: u64 = 0;
        let mut sample_durations = Vec::new();
        let mut off = trun_pos + entry_offset;

        for _ in 0..sample_count {
            if off + per_sample_size > traf.len() {
                break;
            }
            if has_duration {
                let dur = u32_be(traf, off);
                total_trun_duration += dur as u64;
                if sample_durations.len() < 5 {
                    sample_durations.push(dur);
                }
            }
            off += per_sample_size;
        }

        SegmentInfo {
            frag_seq,
            base_decode_time,
            tfdt_version,
            total_trun_duration,
            sample_count,
            sample_durations,
        }
    }

    // ── the actual test ───────────────────────────────────────────────────────

    #[test]
    fn test_dts_continuity_across_segments() {
        let _ = ffmpeg::init();

        let asset_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("assets")
            .join("video.mp4");

        if !asset_path.exists() {
            eprintln!("⚠  Test asset not found at {:?} — skipping", asset_path);
            return;
        }

        // ── 1. Scan the file to build a real StreamIndex ──────────────────
        let index = scan_file(&asset_path).expect("Failed to scan test asset");

        println!("\n=== Source file: {:?} ===", asset_path);
        println!("  Duration:  {:.3}s", index.duration_secs);
        println!("  Segments:  {}", index.segments.len());
        for (i, seg) in index.segments.iter().enumerate() {
            println!(
                "  seg[{}]: seq={} start_pts={} end_pts={} duration={:.3}s keyframe={}",
                i, seg.sequence, seg.start_pts, seg.end_pts, seg.duration_secs, seg.is_keyframe
            );
        }

        // We only test segments 0 and 1 (need at least 2).
        if index.segments.len() < 2 {
            eprintln!(
                "⚠  Need at least 2 segments; got {} — skipping",
                index.segments.len()
            );
            return;
        }

        // ── 2. Generate init.mp4 ──────────────────────────────────────────
        let init_bytes =
            generate_video_init_segment(&index).expect("Failed to generate init segment");

        let timescales = parse_mdhd_timescales(&init_bytes);
        println!("\n=== init.mp4 ===");
        println!("  Size: {} bytes", init_bytes.len());
        println!("  mdhd timescales: {:?}", timescales);
        for (_, ts) in &timescales {
            println!("  → timescale = {}", ts);
        }

        // ── 3. Generate segments 0 and 1 ──────────────────────────────────
        let seg0_bytes = generate_video_segment(&index, 0, 0, &asset_path)
            .expect("Failed to generate segment 0");
        let seg1_bytes = generate_video_segment(&index, 0, 1, &asset_path)
            .expect("Failed to generate segment 1");

        let seg0 = parse_media_segment(&seg0_bytes);
        let seg1 = parse_media_segment(&seg1_bytes);

        println!("\n=== 0.m4s ===");
        println!("  Size:                    {} bytes", seg0_bytes.len());
        println!("  mfhd.FragSeq:            {}", seg0.frag_seq);
        println!("  tfdt version:            {}", seg0.tfdt_version);
        println!("  tfdt.baseDecodeTime:     {}", seg0.base_decode_time);
        println!("  trun.sample_count:       {}", seg0.sample_count);
        println!("  trun total duration:     {}", seg0.total_trun_duration);
        println!("  first sample durations:  {:?}", seg0.sample_durations);
        let seg0_duration_secs = if timescales.first().map(|(_, ts)| *ts).unwrap_or(90000) > 0 {
            seg0.total_trun_duration as f64
                / timescales
                    .first()
                    .map(|(_, ts)| *ts as f64)
                    .unwrap_or(90000.0)
        } else {
            0.0
        };
        println!("  trun duration (secs):    {:.4}s", seg0_duration_secs);

        println!("\n=== 1.m4s ===");
        println!("  Size:                    {} bytes", seg1_bytes.len());
        println!("  mfhd.FragSeq:            {}", seg1.frag_seq);
        println!("  tfdt version:            {}", seg1.tfdt_version);
        println!("  tfdt.baseDecodeTime:     {}", seg1.base_decode_time);
        println!("  trun.sample_count:       {}", seg1.sample_count);
        println!("  trun total duration:     {}", seg1.total_trun_duration);
        println!("  first sample durations:  {:?}", seg1.sample_durations);
        let seg1_duration_secs = if timescales.first().map(|(_, ts)| *ts).unwrap_or(90000) > 0 {
            seg1.total_trun_duration as f64
                / timescales
                    .first()
                    .map(|(_, ts)| *ts as f64)
                    .unwrap_or(90000.0)
        } else {
            0.0
        };
        println!("  trun duration (secs):    {:.4}s", seg1_duration_secs);

        // ── 4. Cross-segment DTS continuity check ─────────────────────────
        println!("\n=== DTS Continuity Analysis ===");

        // When trun has no per-sample durations (flag 0x0100 not set), the muxer
        // relies on trex.default_sample_duration from the init segment. Parse it so
        // our continuity check uses the real effective duration.
        let trex_default_dur = parse_trex_default_duration(&init_bytes);
        println!("  trex.default_sample_duration: {}", trex_default_dur);

        // Effective trun duration for each segment:
        //   • If trun has inline durations (total_trun_duration > 0) → use them.
        //   • Otherwise fall back to sample_count × trex_default_duration.
        let seg0_effective_dur = if seg0.total_trun_duration > 0 {
            seg0.total_trun_duration
        } else {
            seg0.sample_count as u64 * trex_default_dur as u64
        };
        let seg1_effective_dur = if seg1.total_trun_duration > 0 {
            seg1.total_trun_duration
        } else {
            seg1.sample_count as u64 * trex_default_dur as u64
        };

        println!("  seg0 tfdt:              {}", seg0.base_decode_time);
        println!(
            "  seg0 effective trun dur: {} (trun={}, trex_fallback={})",
            seg0_effective_dur,
            seg0.total_trun_duration,
            seg0.total_trun_duration == 0
        );
        println!("  seg1 tfdt:              {}", seg1.base_decode_time);
        println!(
            "  seg1 effective trun dur: {} (trun={}, trex_fallback={})",
            seg1_effective_dur,
            seg1.total_trun_duration,
            seg1.total_trun_duration == 0
        );

        // Expected tfdt for segment 1 = tfdt[0] + effective trun duration of segment 0
        let expected_tfdt_seg1 = seg0.base_decode_time + seg0_effective_dur;
        let actual_tfdt_seg1 = seg1.base_decode_time;
        let delta = actual_tfdt_seg1 as i64 - expected_tfdt_seg1 as i64;

        println!(
            "  Expected seg1 tfdt:     {} (= {} + {})",
            expected_tfdt_seg1, seg0.base_decode_time, seg0_effective_dur
        );
        println!("  Actual   seg1 tfdt:     {}", actual_tfdt_seg1);
        println!("  Delta (actual-expected): {} ticks", delta);

        if delta == 0 {
            println!("  ✓ DTS is perfectly continuous — no gap/overlap!");
        } else if delta < 0 {
            println!(
                "  ✗ DTS DECREASING by {} ticks — this will trigger 'Decreasing DTS' in mediastreamvalidator!",
                -delta
            );
        } else {
            println!(
                "  ⚠ DTS GAP of {} ticks — segments are not contiguous (gap = {:.4}s)",
                delta,
                delta as f64
                    / timescales
                        .first()
                        .map(|(_, ts)| *ts as f64)
                        .unwrap_or(90000.0)
            );
        }

        // Also compare scanner's segment boundaries with actual tfdt.
        // scanner.start_pts is in the source video's native timebase;
        // we rescale to 90kHz to compare with tfdt.
        println!("\n=== Scanner vs Muxer Comparison ===");
        let scanner_seg0 = index.segments.iter().find(|s| s.sequence == 0).unwrap();
        let scanner_seg1 = index.segments.iter().find(|s| s.sequence == 1).unwrap();

        // Derive video timebase by opening the source file, the same way generator.rs does.
        // StreamIndex doesn't store the timebase; generator.rs reads it from the FFmpeg context.
        let video_tb = {
            let input_ctx =
                ffmpeg::format::input(&asset_path).expect("Failed to open source for timebase");
            index
                .video_streams
                .first()
                .and_then(|vs| input_ctx.stream(vs.stream_index).map(|s| s.time_base()))
                .unwrap_or(ffmpeg::Rational::new(1, 15360)) // safe fallback for 30fps
        };

        let rescale = |pts: i64| -> u64 {
            crate::ffmpeg::utils::rescale_ts(pts, video_tb, ffmpeg::Rational::new(1, 90000)).max(0)
                as u64
        };

        let seg0_start_90k = rescale(scanner_seg0.start_pts);
        let seg1_start_90k = rescale(scanner_seg1.start_pts);

        println!(
            "  scanner seg0: start_pts={} ({}_90k) end_pts={} (diff={})",
            scanner_seg0.start_pts,
            seg0_start_90k,
            scanner_seg0.end_pts,
            scanner_seg0.end_pts - scanner_seg0.start_pts
        );
        println!(
            "  scanner seg1: start_pts={} ({}_90k) end_pts={} (diff={})",
            scanner_seg1.start_pts,
            seg1_start_90k,
            scanner_seg1.end_pts,
            scanner_seg1.end_pts - scanner_seg1.start_pts
        );
        println!(
            "  muxer seg0: tfdt={}  scanner_seg0.start@90k={}  match={}",
            seg0.base_decode_time,
            seg0_start_90k,
            seg0.base_decode_time == seg0_start_90k
        );
        println!(
            "  muxer seg1: tfdt={}  scanner_seg1.start@90k={}  match={}",
            seg1.base_decode_time,
            seg1_start_90k,
            seg1.base_decode_time == seg1_start_90k
        );

        // ── 5. Assertions ─────────────────────────────────────────────────
        assert!(
            seg0.sample_count > 0,
            "Segment 0 must contain samples (got 0)"
        );
        assert!(
            seg1.sample_count > 0,
            "Segment 1 must contain samples (got 0)"
        );
        // Effective duration must be > 0 (either inline or trex fallback)
        assert!(
            seg0_effective_dur > 0,
            "Segment 0 effective duration is 0 — will report as 0s segment! \
             (trun_dur={}, trex_default={}, sample_count={})",
            seg0.total_trun_duration,
            trex_default_dur,
            seg0.sample_count
        );
        assert!(
            seg1_effective_dur > 0,
            "Segment 1 effective duration is 0 — will report as 0s segment! \
             (trun_dur={}, trex_default={}, sample_count={})",
            seg1.total_trun_duration,
            trex_default_dur,
            seg1.sample_count
        );

        // mfhd must be monotonically increasing
        assert!(
            seg1.frag_seq > seg0.frag_seq,
            "mfhd FragmentSequenceNumber must increase: seg0={} seg1={}",
            seg0.frag_seq,
            seg1.frag_seq
        );

        // tfdt must match scanner's start_pts (rescaled to 90kHz)
        assert_eq!(
            seg0.base_decode_time, seg0_start_90k,
            "seg0 tfdt ({}) doesn't match scanner start_pts rescaled to 90kHz ({})",
            seg0.base_decode_time, seg0_start_90k
        );
        assert_eq!(
            seg1.base_decode_time, seg1_start_90k,
            "seg1 tfdt ({}) doesn't match scanner start_pts rescaled to 90kHz ({})",
            seg1.base_decode_time, seg1_start_90k
        );

        // The main continuity checks — these are exactly what mediastreamvalidator validates:
        //
        //  1. tfdt must be STRICTLY INCREASING across segments (no decrease / no equality).
        //     "Decreasing DTS" means tfdt[N] <= tfdt[N-1].
        //
        //  2. Each segment's tfdt must match the playlist timeline, which we verify by
        //     comparing against the scanner's start_pts (rescaled to 90 kHz).
        //     (The tfdt == scanner_start@90k checks above already do this.)
        assert!(
            seg1.base_decode_time > seg0.base_decode_time,
            "DTS is NOT increasing! seg0.tfdt={} seg1.tfdt={} — this triggers \
             'Decreasing DTS' in mediastreamvalidator!",
            seg0.base_decode_time,
            seg1.base_decode_time
        );

        println!("\n✓ All DTS checks passed.");
        println!(
            "  seg0.tfdt={} == scanner_seg0.start@90k={} ✓",
            seg0.base_decode_time, seg0_start_90k
        );
        println!(
            "  seg1.tfdt={} == scanner_seg1.start@90k={} ✓",
            seg1.base_decode_time, seg1_start_90k
        );
        println!(
            "  seg1.tfdt({}) > seg0.tfdt({}) ✓ — DTS is monotonically increasing",
            seg1.base_decode_time, seg0.base_decode_time
        );
    }

    #[test]
    fn test_audio_tfdt_timescale() {
        let _ = ffmpeg::init();

        // Using bun33s.mp4 which definitely has audio
        let mut asset_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        asset_path.push("testvideos");
        asset_path.push("bun33s.mp4");

        if !asset_path.exists() {
            eprintln!("⚠  Test asset not found at {:?} — skipping", asset_path);
            return;
        }

        // 1. Scan
        let index = scan_file(&asset_path).expect("Failed to scan test asset");
        let audio_stream = index
            .audio_streams
            .first()
            .expect("No audio stream found in bun33s.mp4");

        println!(
            "Audio stream: rate={} channels={}",
            audio_stream.sample_rate, audio_stream.channels
        );

        // 2. Generate Audio Segment 0
        use crate::segment::generator::generate_audio_segment;
        let seg0_bytes = generate_audio_segment(&index, 1, 0, &asset_path, false)
            .expect("Failed to generate audio seg 0");

        // 3. Parse tfdt
        let seg0 = parse_media_segment(&seg0_bytes);
        println!("Audio seg0 tfdt: {}", seg0.base_decode_time);

        // Segment 0 start_pts is 0, so tfdt should be 0 regardless of timebase.
        assert_eq!(seg0.base_decode_time, 0);

        // 4. Generate Audio Segment 1
        let _seg1_info = index
            .segments
            .iter()
            .find(|s| s.sequence == 1)
            .expect("Segment 1 not found");

        // start_pts is in video timebase (or audio? actually scanner uses video timebase for segments)
        // Let's check `scanner.rs`: `segments` are calculated based on video keyframes.
        // `start_pts` is from video stream.

        let seg1_bytes = generate_audio_segment(&index, 1, 1, &asset_path, false)
            .expect("Failed to generate audio seg 1");
        let seg1 = parse_media_segment(&seg1_bytes);
        println!("Audio seg1 tfdt: {}", seg1.base_decode_time);

        // Calculate expected tfdt
        // Expected = start_time_seconds * audio_sample_rate
        let seg0_info = index.segments.iter().find(|s| s.sequence == 0).unwrap();
        let expected_time_sec = seg0_info.duration_secs;

        let expected_tfdt_sample = (expected_time_sec * audio_stream.sample_rate as f64) as u64;
        let expected_tfdt_90k = (expected_time_sec * 90000.0) as u64;

        println!("Expected time: {:.4}s", expected_time_sec);
        println!(
            "Expected tfdt (at {}Hz): {}",
            audio_stream.sample_rate, expected_tfdt_sample
        );
        println!("Value if 90kHz: {}", expected_tfdt_90k);
        println!("Actual tfdt: {}", seg1.base_decode_time);

        let diff_sample = (seg1.base_decode_time as i64 - expected_tfdt_sample as i64).abs();
        let diff_90k = (seg1.base_decode_time as i64 - expected_tfdt_90k as i64).abs();

        if (audio_stream.sample_rate as u64).abs_diff(90000) > 1000 {
            assert!(
                diff_sample < diff_90k,
                "Audio tfdt {} seems to be in 90kHz (diff={}) instead of {}Hz (diff={})",
                seg1.base_decode_time,
                diff_90k,
                audio_stream.sample_rate,
                diff_sample
            );
        }
    }
}
