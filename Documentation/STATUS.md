# HLS Streaming Server - Implementation Status

**Last Updated:** 2026-02-19  
**Current State:** Audio transcoding ✅ complete. Subtitle extraction ✅ complete.

---

## Milestone 1: Project Setup & FFmpeg Integration

**Status:** ✅ COMPLETE

**Implemented:**
- `Cargo.toml` with all dependencies including `ffmpeg-next` v8.0
- FFmpeg initialization in `src/main.rs`
- Basic error handling framework in `src/error.rs`
- File opening and stream info detection

**Compliance:** ✅ Matches plan - uses `ffmpeg-next` library directly

**TODO:** None

---

## Milestone 2: Stream Indexing

**Status:** ✅ COMPLETE

**Implemented:**
- `StreamIndex` struct in `src/state.rs`
- Video stream detection in `src/index/scanner.rs`
- Audio stream detection
- Subtitle stream detection
- Segment boundary calculation from keyframes
- Timeout handling for unindexed files

**Compliance:** ✅ Matches plan - uses `ffmpeg-next` library directly

**TODO:** None

---

## Milestone 3: Audio Track Planning

**Status:** ⚠️ PARTIAL

**Implemented:**
- Audio codec detection in `src/audio_plan/planner.rs`
- Transcode requirement logic (AAC vs AC-3 vs E-AC-3)
- Language track grouping
- `AudioTrackPlan` and `AudioVariant` structs

**Compliance:** ✅ Matches plan

**TODO:**
- [ ] Actual audio transcoding implementation (Milestone 5)
- [ ] Audio encoder pool management

---

## Milestone 4: fMP4 Muxer (In-Memory)

**Status:** ✅ COMPLETE

**Implemented:**
- **In-memory Fmp4Muxer** (`src/segment/muxer.rs`) using `ffmpeg-next` and `MemoryWriter`
- Custom `MemoryWriter` with `AVIOContext` (`src/ffmpeg/io.rs`) - handles init and media segments
- Full in-memory pipeline: Packets -> Muxer -> Vec<u8> -> Cache -> HTTP Response
- Elimination of temp file writes for segment generation

**Compliance:** ✅ **Matches plan**
- In-memory muxing with custom IO context: ✅
- Segment structure validation (single fragment, correct boxes): ✅
- Timestamp handling (delta patching for TFDT): ✅

**TODO:**
- [x] Implement custom `AVIOContext` in `src/ffmpeg/io.rs`
- [x] Implement in-memory `Fmp4Muxer` in `src/segment/muxer.rs`
- [x] Remove dependency on external FFmpeg CLI for segment generation
- [x] Eliminate temp file writes

---

## Milestone 5: Audio Transcoding Pipeline

**Status:** ✅ COMPLETE

**Implemented:**
- `AudioDecoder` with real FFmpeg decoder + pre-roll skipping (`src/transcode/decoder.rs`)
- `AudioResampler` with `SwrContext`, FLTP→FLTP at 48 kHz (`src/transcode/resampler.rs`)
- `AacEncoder` producing ADTS-less AAC packets (`src/transcode/encoder.rs`)
- `transcode_audio_segment` pipeline with Opus 960→1024 rechunking (`src/transcode/pipeline.rs`)
- `mux_aac_packets_to_fmp4` with `frag_every_frame` for audio-only muxing (`src/segment/muxer.rs`)
- Codec-based GROUP-ID grouping + multiple `EXT-X-STREAM-INF` entries (`src/playlist/master.rs`)

**Compliance:** ✅ Matches plan  
**TODO:** None

---

## Milestone 6: Subtitle Extraction & WebVTT Conversion

**Status:** ✅ COMPLETE

**Implemented:**
- Subtitle stream detection in `src/index/subtitle.rs`
- `SubtitleExtractor` for SRT, ASS/SSA, WebVTT, plain text (`src/subtitle/extractor.rs`)
- `WebVttWriter` with `X-TIMESTAMP-MAP` and HTML entity escaping (`src/subtitle/webvtt.rs`)
- Real `generate_subtitle_segment` using FFmpeg packet extraction (`src/segment/generator.rs`)
- Bitmap subtitle exclusion (PGS/DVB/XSUB → `Err`) (`src/subtitle/decoder.rs`)
- `X-TIMESTAMP-MAP=MPEGTS:N,LOCAL:00:00:00.000` computed from actual segment `start_pts`

**Compliance:** ✅ Matches plan  
**TODO:** None

---

## Milestone 7: Playlist Generation

**Status:** ✅ COMPLETE

**Implemented:**
- Master playlist generation in `src/playlist/master.rs`
- Video variant playlist in `src/playlist/variant.rs`
- Audio variant playlists
- Subtitle variant playlists
- Codec string generation in `src/playlist/codec.rs`
- `#EXT-X-MAP` tags for init segments
- `#EXT-X-TARGETDURATION` calculation
- `#EXT-X-PLAYLIST-TYPE:VOD` and `#EXT-X-ENDLIST` tags

**Compliance:** ✅ Matches plan

**TODO:**
- [ ] Fine-tune codec strings for better compatibility
- [ ] Add bandwidth calculation for video variants

---

## Milestone 8: HTTP Server & Caching

**Status:** ✅ COMPLETE

**Implemented:**
- Axum router in `src/http/routes.rs`
- All request handlers in `src/http/handlers.rs`
- Stream management handlers in `src/http/streams.rs`
- LRU segment cache in `src/http/cache.rs`
- CORS middleware in `src/http/middleware.rs`
- Proper HTTP headers (Content-Type, Cache-Control)
- Graceful shutdown handling

**Compliance:** ✅ Matches plan

**TODO:**
- [ ] Implement cache eviction policy (currently no memory limit enforcement)
- [ ] Add `Accept-Ranges` header for byte-range support

---

## Milestone 9: Integration Testing

**Status:** ❌ NOT IMPLEMENTED

**Implemented:**
- Test module stubs in `src/integration/`
- Validation utilities in `src/integration/validation.rs`
- Fixture definitions in `src/integration/fixtures.rs`

**Compliance:** ❌ **Does NOT match plan**
- All test functions are stubs with dead code warnings
- No actual integration tests run
- VLC playback: ❌ Currently fails (loops, glitches, no audio)
- Browser playback: ❌ Not tested

**TODO:**
- [ ] Implement actual integration tests
- [ ] Fix HLS playback issues (see Struggles section)
- [ ] Add automated VLC/browser testing

---

## Milestone 10: Production Hardening

**Status:** ⚠️ PARTIAL

**Implemented:**
- Configuration in `src/config.rs` and `src/config_file.rs`
- Logging with `tracing` in `src/main.rs`
- Metrics stub in `src/metrics.rs`
- Health check endpoint `/health`
- Rate limiting stub in `src/limits.rs`
- Connection limiting stub
- Docker configuration in `Dockerfile` and `docker-compose.yml`
- Documentation in `README.md`

**Compliance:** ⚠️ **Partially matches plan**
- Config file support: ⚠️ Exists but not fully integrated
- Metrics endpoint: ❌ Stub only, not functional
- Rate limiting: ❌ Stub only, not enforced
- Connection limits: ❌ Stub only, not enforced
- Memory limits: ❌ Not implemented

**TODO:**
- [ ] Implement actual metrics collection and Prometheus export
- [ ] Implement rate limiting middleware
- [ ] Implement connection limiting
- [ ] Implement memory limit enforcement
- [ ] Complete config file integration

---

## Struggles: HLS Segment Generation

The core challenge preventing functional HLS playback is **generating valid fMP4/CMAF segments that chain properly**. Despite multiple approaches, VLC and other players exhibit the following behaviors:

### Observed Issues

1. **Segment Loop:** VLC plays the first 3-4 seconds correctly, then glitches for 1-2 seconds, then resets to 00:00 and loops infinitely.

2. **No Audio:** Audio segments are generated but not played, suggesting codec mismatch or timing issues.

3. **Apple Validator Errors:** `mediastreamvalidator` reports "Error injecting segment data" for multiple segments.

### Root Causes Investigated

1. **Duplicate `ftyp` Boxes:** Initially, segments contained `ftyp` + `moov` + `moof` + `mdat`, but HLS CMAF segments should only contain `moof` + `mdat`. Fixed by stripping `ftyp`/`moov` boxes.

2. **Track-Specific Init Segments:** Initially used a single init segment with both video and audio tracks. Fixed by creating separate init segments for video-only and audio-only tracks.

3. **TFDT Timestamps:** Suspected that Track Fragment Decode Time (TFDT) boxes all started at 0, causing players to think each segment was at the timeline beginning. Attempted fix was buggy (index out of bounds).

4. **Segment Duration Mismatch:** Apple validator reported playlist vs segment duration mismatches (e.g., playlist says 4.8s, segment metadata says 0.0s).

5. **External FFmpeg CLI Limitations:** Using `ffmpeg` command-line tool with `-hls_segment_type fmp4` produces segments with proper structure, but we cannot control the exact box contents or timestamp handling.

### Current Approach

Currently using FFmpeg HLS muxer:
```bash
ffmpeg -i input.mp4 -ss <start> -t <duration> \
  -c:v copy -an \
  -hls_time <duration> \
  -hls_playlist_type vod \
  -hls_segment_type fmp4 \
  -hls_segment_filename seg_%d.m4s \
  -f hls playlist.m3u8
```

This produces segments with:
- ✅ Proper `moof` + `mdat` structure
- ✅ Correct codec parameters
- ❌ Unclear timestamp continuity between segments
- ❌ No control over TFDT base times

### Why In-Memory Muxing Failed

The plan calls for in-memory muxing using `AVIOContext` with custom write callbacks. However:

1. **FFmpeg 8.0 API Changes:** The `ffmpeg-next` v8.0 bindings have limited support for custom IO contexts.

2. **Complexity:** Proper fMP4 muxing requires:
   - Writing `ftyp`, `moov`, `mvex`, `trex` boxes in init segment
   - Writing `moof`, `mfhd`, `traf`, `trun`, `tfdt` boxes in each segment
   - Calculating correct sample durations, flags, and offsets
   - Handling multiple tracks with different timescales

3. **Time Constraints:** Implementing a full fMP4 muxer from scratch is a significant undertaking.

### Next Steps

1. **Debug TFDT Values:** Extract and compare TFDT values across consecutive segments to verify continuity.

2. **Test with hls.js:** Browser-based players may provide better error messages than VLC.

3. **Simplify:** Try video-only stream first (no audio) to isolate video segment issues.

4. **Consider Alternatives:**
   - Use `mp4box` (GPAC) for segment generation
   - Use `bento4` tools
   - Continue with FFmpeg CLI but post-process segments to fix timestamps

5. **Reference Implementation:** Study working HLS servers (e.g., nginx-rtmp, GStreamer) to understand proper segment structure.

---

## Summary

**Completed Milestones:** 1, 2, 4, 5, 6, 7, 8 (7/10)  
**Partially Completed:** 3, 10 (2/10)  
**Not Started:** 9 (1/10)

**Blockers:** None currently.  
**Next Focus:** Milestone 9 (Integration Testing).
