# HLS Server — Performance & Correctness Report

**Date:** 2026-02-21  
**Scope:** Inefficiencies, blocking operations, race conditions, and correctness issues found by static code review.

---

## 1. Blocking Synchronous FFmpeg Calls on the Async Runtime

**Severity: Critical**

Every segment-generation function (`generate_video_segment`, `generate_audio_segment`, `generate_subtitle_segment`, `generate_transcoded_audio_segment`) is **synchronous and CPU-bound**, yet they are called directly from Axum async handlers without being offloaded to a blocking thread pool.

```@/Users/miquel/Devel/hls-server/src/http/handlers.rs:311-315
    let data =
        crate::segment::generate_video_segment(&index, track_index, sequence, &index.source_path)
            .map_err(|e| {
            HttpError::InternalError(format!("Failed to generate video segment: {}", e))
        })?;
```

FFmpeg demuxing, decoding, and encoding are all blocking I/O + CPU operations. Calling them directly in an `async fn` **starves the Tokio runtime** — all other requests queue behind the current one. With 600 subtitle segments being fetched, this means the runtime is blocked for the entire duration of each segment generation, one at a time.

**Fix:** Wrap every segment-generation call in `tokio::task::spawn_blocking`:

```rust
let data = tokio::task::spawn_blocking(move || {
    crate::segment::generate_subtitle_segment(&index, track_index, start_seq, end_seq, &index.source_path)
})
.await
.map_err(|e| HttpError::InternalError(e.to_string()))??;
```

This also applies to `generate_video_segment`, `generate_audio_segment`, `generate_init_segment`, `generate_video_init_segment`, `generate_audio_init_segment`, and `scan_file` (called from `handle_dynamic_request`).

---

## 2. `cleanup_expired_streams()` Called on Every Single Request

**Severity: High**

```@/Users/miquel/Devel/hls-server/src/http/dynamic.rs:69
    state.cleanup_expired_streams();
```

`cleanup_expired_streams` iterates over **all streams** and then calls `remove_stream`, which itself iterates over `path_to_stream` to find the matching key:

```@/Users/miquel/Devel/hls-server/src/state.rs:317-330
    if let Some(index) = self.streams.remove(stream_id) {
        let (_, arc) = index;
        if let Some(path) = self
            .path_to_stream
            .iter()
            .find(|r| r.value() == stream_id)
            .map(|r| r.key().clone())
        {
            self.path_to_stream.remove(&path);
        }
```

This is O(streams × paths) on every request. With 600 subtitle segment requests, this runs 600 times. Even if it's fast with few streams, it holds DashMap shard locks during iteration on every request.

**Fix:** Run cleanup on a background `tokio::time::interval` task (e.g., every 60 seconds), not per-request. Remove the `cleanup_expired_streams()` call from `handle_dynamic_request`.

Also fix the O(n) reverse-lookup in `remove_stream` by storing the path inside `StreamIndex` or using a bidirectional map.

---

## 3. Subtitle Segment Generation Re-Opens and Re-Seeks the File Every Time

**Severity: High (the primary cause of 100% CPU on subtitle download)**

For each of the ~600 subtitle segment requests, `generate_subtitle_segment` does:

1. `ffmpeg::format::input(&index.source_path)` — opens and parses the container
2. `input.seek(seek_ts, ...)` — seeks to the segment start
3. Iterates all packets from that position until `past_end`

```@/Users/miquel/Devel/hls-server/src/segment/generator.rs:515-628
    let mut input = ffmpeg::format::input(&index.source_path)
        ...
    let _ = input.seek(seek_ts, i64::MIN..seek_ts + 1);
    ...
    for (stream, mut packet) in input.packets() {
        if stream.index() != track_index {
            continue;
        }
```

For a file with many streams (video + audio + subtitles), `input.packets()` returns **all packets from all streams**, and the code discards non-subtitle packets with `continue`. For a 600-segment movie, this means opening the file 600 times and seeking 600 times, each time reading through potentially many video/audio packets before finding the next subtitle packet.

**Fix (short term):** The subtitle data for the entire file is small — it should be extracted **once** at index time and stored in memory (e.g., a `Vec<SubtitleCue>` per subtitle stream on `StreamIndex`). Segment generation then becomes a simple slice/filter over the in-memory cues, with zero FFmpeg I/O.

**Fix (medium term):** If on-demand extraction is kept, use `AVDISCARD_ALL` on all non-subtitle streams before iterating packets to avoid FFmpeg reading and decoding video/audio packet data unnecessarily.

---

## 4. `generate_transcoded_audio_segment` Opens the File Twice

**Severity: Medium**

```@/Users/miquel/Devel/hls-server/src/segment/generator.rs:399-406
    let video_timebase = index
        .video_streams
        .first()
        .and_then(|v| {
            ffmpeg::format::input(&index.source_path)
                .ok()
                .and_then(|input| input.stream(v.stream_index).map(|s| s.time_base()))
        })
        .unwrap_or(ffmpeg::Rational(1, 90000));
```

This opens the file a second time just to read the video stream's timebase — information that is **already stored** in `index.video_timebase` (set by the scanner at `@/Users/miquel/Devel/hls-server/src/index/scanner.rs:130-134`).

**Fix:** Replace with `index.video_timebase` directly. One-line change, eliminates a full file open per transcoded audio segment.

---

## 5. Race Condition: Duplicate Stream Indexing Under Concurrent Requests

**Severity: High**

In `handle_dynamic_request`, the check-then-register pattern is not atomic:

```@/Users/miquel/Devel/hls-server/src/http/dynamic.rs:73-87
    let index = if let Some(index) = state.get_stream_by_path(&path_str) {
        index.touch();
        index
    } else {
        if !media_path.exists() { ... }
        info!("Indexing new file: {:?}", media_path);
        let new_index = scan_file(&media_path)...;
        state.register_stream(new_index)
    };
```

If two requests arrive simultaneously for the same file that hasn't been indexed yet, **both** will call `scan_file` concurrently. `scan_file` is expensive (reads the entire file to find keyframes). The second registration will silently overwrite the first in `DashMap`, but both `scan_file` calls run to completion, wasting CPU and time.

**Fix:** Use a `DashMap<String, Arc<tokio::sync::OnceCell<Arc<StreamIndex>>>>` or a `tokio::sync::Mutex`-guarded pending-index map so that concurrent requests for the same path wait for the first indexing to complete rather than all starting their own.

---

## 6. Cache Race Condition: Double-Computation Under Concurrent Requests

**Severity: Medium**

The cache check and segment generation are not atomic:

```@/Users/miquel/Devel/hls-server/src/http/handlers.rs:296-320
    if let Some(data) = state.segment_cache.get(&stream_id, &cache_key, sequence) {
        return Ok(...);
    }
    // <-- another request for the same segment can pass here simultaneously
    let index = state.get_stream_or_error(&stream_id)?;
    let data = crate::segment::generate_video_segment(...)?;
    state.segment_cache.insert(...);
```

Two concurrent requests for the same uncached segment will both generate it. For video segments this is wasteful CPU; for transcoded audio it's doubly expensive.

**Fix:** Use a `DashMap<CacheKey, Arc<tokio::sync::OnceCell<Bytes>>>` as a "in-flight" tracker, so the second request waits for the first to finish and reuses the result.

---

## 7. `SegmentCache::evict_if_needed` Collects and Sorts All Entries

**Severity: Medium**

```@/Users/miquel/Devel/hls-server/src/http/cache.rs:131-143
    let mut entries: Vec<_> = self.entries.iter().collect();
    entries.sort_by_key(|e| e.value().last_accessed);
```

When eviction is triggered, the entire cache is collected into a `Vec` and sorted — O(n log n) while holding DashMap shard read locks across the entire iteration. With a large cache (hundreds of segments), this creates a significant pause visible to all concurrent requests.

**Fix:** Use a proper LRU structure (e.g., the `lru` crate) instead of a `DashMap` + manual sort. Alternatively, maintain a separate `BTreeMap<SystemTime, CacheKey>` for O(log n) LRU eviction.

---

## 8. `SegmentCache::memory_bytes` Counter Can Go Negative (Underflow)

**Severity: Medium**

`memory_bytes` is an `AtomicUsize`. In `evict_if_needed`, expired entries are removed and `freed` bytes are subtracted:

```@/Users/miquel/Devel/hls-server/src/http/cache.rs:117-125
    self.entries.retain(|_, entry| {
        if entry.is_expired(self.config.ttl_secs) {
            freed += entry.data.len();
            false
        } else { true }
    });
    self.memory_bytes.fetch_sub(freed, Ordering::Relaxed);
```

If a concurrent `insert` adds bytes between the `retain` and the `fetch_sub`, or if the `CacheEntry` is modified between reads, `memory_bytes` can drift from the true total. Over time this causes the counter to underflow (wrap around on `usize`), making the memory limit check permanently false and allowing unbounded cache growth.

**Fix:** Use `fetch_sub` with a saturating check, or recompute the true total from the entries map after eviction rather than tracking it with a separate atomic.

---

## 9. `eprintln!` Debug Logging in Hot Path (Production Code)

**Severity: Medium**

```@/Users/miquel/Devel/hls-server/src/segment/muxer.rs:173-179
    eprintln!(
        "[MUXER] Writing packet: stream={}, pts={:?}, dts={:?}, dur={}",
        out_index,
        packet.pts(),
        packet.dts(),
        packet.duration()
    );
```

`eprintln!` is called for **every packet written** through the muxer. For a 600-segment subtitle download, and especially for video/audio segments with hundreds of packets each, this generates enormous amounts of stderr output and causes unnecessary string formatting and I/O on every packet write. This is a direct contributor to the 100% CPU observation.

**Fix:** Replace with `tracing::trace!(...)` so it is compiled out or filtered at runtime. Check for other `eprintln!` calls in hot paths.

---

## 10. `generate_media_segment_ffmpeg` Duplicates the TFDT Patching Logic

**Severity: Low (maintainability)**

The TFDT/mfhd patching loop in `generate_media_segment_ffmpeg` (`@/Users/miquel/Devel/hls-server/src/segment/generator.rs:898-999`) is a near-identical copy of the `patch_tfdts` function defined at line 223. The standalone `patch_tfdts` function is called from `generate_transcoded_audio_segment` but the video/audio copy path reimplements the same box-walking inline.

**Fix:** Remove the inline copy and call `patch_tfdts` from `generate_media_segment_ffmpeg` as well.

---

## 11. `AV_LOG_SET_LEVEL(DEBUG)` Set Globally in Production

**Severity: Medium**

```@/Users/miquel/Devel/hls-server/src/segment/muxer.rs:23-24
    unsafe {
        ffmpeg::ffi::av_log_set_level(ffmpeg::ffi::AV_LOG_DEBUG);
    }
```

This is called in `Fmp4Muxer::new()`, which runs for every segment. It sets FFmpeg's global log level to `DEBUG`, causing FFmpeg to emit verbose logs to stderr for every muxer operation across the entire process. This is a significant source of unnecessary I/O and CPU on every segment generation.

**Fix:** Remove this line entirely, or set it once at startup to `AV_LOG_WARNING` (or `AV_LOG_ERROR`).

---

## 12. `remove_stream` Has O(n) Reverse Lookup

**Severity: Low**

```@/Users/miquel/Devel/hls-server/src/state.rs:319-325
    if let Some(path) = self
        .path_to_stream
        .iter()
        .find(|r| r.value() == stream_id)
        .map(|r| r.key().clone())
    {
```

`path_to_stream` maps path→stream_id. To remove by stream_id, the code does a full linear scan of all paths. This is fine with few streams but is unnecessary since `StreamIndex` already stores `source_path`.

**Fix:** Use `index.source_path.to_string_lossy()` directly to remove from `path_to_stream` instead of scanning.

---

## Summary Table

| # | Issue | Severity | Impact |
|---|-------|----------|--------|
| 1 | Blocking FFmpeg calls on async runtime | **Critical** | Starves all concurrent requests |
| 2 | `cleanup_expired_streams` on every request | **High** | Lock contention, O(n) per request |
| 3 | Subtitle: file opened + seeked 600× | **High** | Primary cause of 100% CPU |
| 4 | Transcoded audio opens file twice | **Medium** | Wasted file open per segment |
| 5 | Race: duplicate stream indexing | **High** | Wasted CPU, data race |
| 6 | Race: duplicate segment generation | **Medium** | Wasted CPU under concurrency |
| 7 | Cache eviction O(n log n) with lock | **Medium** | Pause under memory pressure |
| 8 | `memory_bytes` counter can underflow | **Medium** | Unbounded cache growth |
| 9 | `eprintln!` on every packet write | **Medium** | Direct CPU/I/O overhead |
| 10 | Duplicated TFDT patching logic | Low | Maintainability |
| 11 | `AV_LOG_DEBUG` set globally per muxer | **Medium** | FFmpeg stderr flood |
| 12 | O(n) reverse lookup in `remove_stream` | Low | Minor, scales poorly |

---

## Recommended Priority Order

1. **Fix #9 and #11 first** — zero-risk, immediate CPU reduction (remove `eprintln!` and `av_log_set_level(DEBUG)`).
2. **Fix #1** — wrap all segment generators in `spawn_blocking` to unblock the async runtime.
3. **Fix #3** — extract subtitle cues once at index time and cache them in `StreamIndex`.
4. **Fix #2** — move `cleanup_expired_streams` to a background task.
5. **Fix #4** — use `index.video_timebase` instead of re-opening the file.
6. **Fix #5 and #6** — add in-flight deduplication for indexing and segment generation.
7. **Fix #7 and #8** — replace the manual LRU with the `lru` crate.
