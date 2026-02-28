# HLS Streaming Server Implementation Plan

## Project Overview

Build a Rust-based HLS streaming server that serves MP4/MKV files as fMP4/CMAF segments without transcoding video, with intelligent audio track handling and on-the-fly subtitle conversion to WebVTT. All operations occur in-memory with no disk writes.

---

## 1. Technology Stack

| Component | Technology | Rationale |
|-----------|------------|-----------|
| **Language** | Rust 2021 Edition | Memory safety, performance, async ecosystem |
| **FFmpeg Bindings** | `ffmpeg-next` v7.0+ | Direct library access (no CLI), supports all codecs |
| **HTTP Server** | `axum` v0.7 + `tokio` | Async, type-safe routing, middleware support |
| **Concurrency** | `tokio` + `dashmap` v5.5 | Async runtime + thread-safe in-memory cache |
| **Logging** | `tracing` + `tracing-subscriber` | Structured logging for debugging |
| **UUID** | `uuid` v1.6 | Unique session/stream identifiers |
| **Build** | `cargo` + `pkg-config` | FFmpeg library discovery |

### System Dependencies
```bash
# Ubuntu/Debian
sudo apt-get install clang libavcodec-dev libavformat-dev libavutil-dev \
     libavfilter-dev libswscale-dev libswresample-dev libswresample-dev \
     pkg-config build-essential

# macOS
brew install ffmpeg pkg-config
```

---

## 2. Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           HTTP Server (axum)                            │
├─────────────────────────────────────────────────────────────────────────┤
│  Routes:                                                                │
│  GET /streams/{id}/master.m3u8  → Master Playlist                       │
│  GET /streams/{id}/video.m3u8   → Video Variant Playlist                │
│  GET /streams/{id}/audio_*.m3u8 → Audio Variant Playlists               │
│  GET /streams/{id}/sub_*.m3u8   → Subtitle Variant Playlists            │
│  GET /streams/{id}/init.mp4     → Initialization Segment                │
│  GET /streams/{id}/video_*.m4s  → Video Segments                        │
│  GET /streams/{id}/audio_*.m4s  → Audio Segments                        │
│  GET /streams/{id}/sub_*.vtt    → Subtitle Segments                     │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                         Stream Manager (AppState)                       │
├─────────────────────────────────────────────────────────────────────────┤
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────────┐ │
│  │  Stream Index   │  │  Segment Cache  │  │  Transcoder Pool        │ │
│  │  (metadata)     │  │  (LRU, in-RAM)  │  │  (audio AAC encoder)    │ │
│  └─────────────────┘  └─────────────────┘  └─────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                         FFmpeg Processing Layer                         │
├─────────────────────────────────────────────────────────────────────────┤
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────────┐ │
│  │  Input Context  │  │  Output Muxer   │  │  Audio Encoder (AAC)    │ │
│  │  (demux)        │  │  (fMP4 in-RAM)  │  │  (if needed)            │ │
│  └─────────────────┘  └─────────────────┘  └─────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                         Source File (MP4/MKV)                           │
│                         (Read-Only, No Writes)                          │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## 3. Core Data Structures

### 3.1 Stream Index (Generated at Startup)

```rust
struct StreamIndex {
    stream_id: String,              // UUID for this stream session
    source_path: PathBuf,
    duration_secs: f64,
    
    // Video
    video_streams: Vec<VideoStreamInfo>,
    
    // Audio (may include original + transcoded variants)
    audio_streams: Vec<AudioStreamInfo>,
    
    // Subtitles
    subtitle_streams: Vec<SubtitleStreamInfo>,
    
    // Segment boundaries (calculated from keyframes)
    segments: Vec<SegmentInfo>,
    
    // Playlist generation timestamp
    indexed_at: SystemTime,
}

struct VideoStreamInfo {
    stream_index: usize,
    codec_id: AVCodecID,
    width: u32,
    height: u32,
    bitrate: u64,
    framerate: Rational,
    language: Option<String>,
}

struct AudioStreamInfo {
    stream_index: usize,
    codec_id: AVCodecID,
    sample_rate: u32,
    channels: u16,
    bitrate: u64,
    language: Option<String>,
    is_transcoded: bool,            // true if this is an AAC transcode of AC-3
    source_stream_index: Option<usize>, // Original stream if transcoded
}

struct SubtitleStreamInfo {
    stream_index: usize,
    codec_id: AVCodecID,
    language: Option<String>,
    format: SubtitleFormat,         // TTXT, ASS, SRT, etc.
}

struct SegmentInfo {
    sequence: usize,
    start_pts: i64,
    end_pts: i64,
    duration_secs: f64,
    is_keyframe: bool,
    video_byte_offset: u64,         // For optimization
}
```

### 3.2 Application State

```rust
struct AppState {
    // Active streams (path -> StreamIndex)
    streams: DashMap<String, Arc<StreamIndex>>,
    
    // Segment cache (stream_id + segment_type + sequence -> Bytes)
    segment_cache: DashMap<String, Bytes>,
    
    // Cache configuration
    cache_config: CacheConfig,
    
    // Audio encoder pool (shared across streams)
    audio_encoders: RwLock<HashMap<AudioEncoderKey, AudioEncoder>>,
    
    // Server shutdown signal
    shutdown: AtomicBool,
}

struct CacheConfig {
    max_memory_mb: usize,
    max_segments: usize,
    ttl_secs: u64,
}
```

---

## 4. Module Breakdown

```
src/
├── main.rs                 # Entry point, CLI parsing, server startup
├── config.rs               # Configuration (cache size, segment duration, etc.)
├── state.rs                # AppState definition and management
├── index/
│   ├── mod.rs              # Index module exports
│   ├── scanner.rs          # FFmpeg index scanning logic
│   ├── video.rs            # Video stream analysis
│   ├── audio.rs            # Audio stream analysis + transcode planning
│   └── subtitle.rs         # Subtitle stream analysis
├── playlist/
│   ├── mod.rs              # Playlist module exports
│   ├── master.rs           # master.m3u8 generation
│   ├── media.rs            # Variant playlist generation (video/audio/sub)
│   └── vtt.rs              # WebVTT segment generation
├── segment/
│   ├── mod.rs              # Segment module exports
│   ├── muxer.rs            # fMP4 muxing with custom IO
│   ├── video.rs            # Video segment generation
│   ├── audio.rs            # Audio segment generation (copy + transcode)
│   └── cache.rs            # LRU cache management
├── http/
│   ├── mod.rs              # HTTP module exports
│   ├── routes.rs           # Axum route definitions
│   ├── handlers.rs         # Request handlers
│   └── middleware.rs       # Logging, CORS, etc.
├── ffmpeg/
│   ├── mod.rs              # FFmpeg module exports
│   ├── context.rs          # FFmpeg context wrappers
│   ├── io.rs               # Custom AVIOContext for in-memory writing
│   └── utils.rs            # Helper functions (timebase conversion, etc.)
└── error.rs                # Custom error types
```

---

## 5. Implementation Milestones

### Milestone 1: Project Setup & FFmpeg Integration
**Goal:** Verify FFmpeg libraries work and can open media files.

**Deliverables:**
- [ ] `Cargo.toml` with all dependencies
- [ ] Build script for FFmpeg library discovery
- [ ] Basic FFmpeg init test (`ffmpeg::init()`)
- [ ] File opening test (open MP4, print stream info)
- [ ] Error handling framework (`thiserror` or custom)

**Tests:**
```rust
#[test]
fn test_ffmpeg_init() {
    assert!(ffmpeg::init().is_ok());
}

#[test]
fn test_open_mp4_file() {
    let ctx = ffmpeg::format::input("test.mp4").unwrap();
    assert!(ctx.streams().len() > 0);
}
```

---

### Milestone 2: Stream Indexing
**Goal:** Extract all metadata from source file without reading full payload.

**Deliverables:**
- [ ] `StreamIndex` struct implementation
- [ ] Video stream detection (codec, resolution, keyframes)
- [ ] Audio stream detection (codec, sample rate, channels, language)
- [ ] Subtitle stream detection (codec, language, format)
- [ ] Segment boundary calculation (keyframe-based, ~4s segments)
- [ ] Timeout handling for unindexed MKV files

**Tests:**
```rust
#[test]
fn test_index_mp4_file() {
    let index = scan_file("test.mp4").unwrap();
    assert!(index.duration_secs > 0.0);
    assert!(index.video_streams.len() > 0);
}

#[test]
fn test_index_detects_audio_codecs() {
    let index = scan_file("test_ac3.mp4").unwrap();
    let audio = &index.audio_streams[0];
    assert_eq!(audio.codec_id, AVCodecID::AV_CODEC_ID_AC3);
}

#[test]
fn test_index_timeout_mkv() {
    // Unindexed MKV should timeout and return partial index
    let result = scan_file_with_timeout("test_unindexed.mkv", Duration::from_secs(5));
    assert!(result.is_err()); // Or returns partial data
}
```

---

### Milestone 3: Audio Track Planning
**Goal:** Determine which audio variants to serve (original + transcode).

**Deliverables:**
- [ ] Audio codec capability detection (AAC, AC-3, E-AC-3, Opus, etc.)
- [ ] Transcode requirement logic:
  - AAC exists → serve AAC (no transcode)
  - AC-3 exists alone → serve AC-3 + transcode to AAC
  - AC-3 + AAC exist → serve both (no transcode needed)
- [ ] Audio encoder initialization (AAC, 48kHz, stereo/5.1)
- [ ] Language track grouping (same language = same group in HLS)

**Tests:**
```rust
#[test]
fn test_audio_plan_aac_only() {
    let plan = plan_audio_tracks(&index);
    assert_eq!(plan.variants.len(), 1);
    assert!(!plan.variants[0].requires_transcode);
}

#[test]
fn test_audio_plan_ac3_only() {
    let plan = plan_audio_tracks(&index);
    assert_eq!(plan.variants.len(), 2); // AC-3 + AAC transcode
    assert!(plan.variants.iter().any(|v| v.requires_transcode));
}

#[test]
fn test_audio_plan_ac3_and_aac() {
    let plan = plan_audio_tracks(&index);
    assert_eq!(plan.variants.len(), 2); // Both served, no transcode
    assert!(!plan.variants.iter().any(|v| v.requires_transcode));
}
```

---

### Milestone 4: fMP4 Muxer (In-Memory)
**Goal:** Create valid fMP4 segments in RAM without disk writes.

**Deliverables:**
- [ ] Custom `AVIOContext` wrapper (writes to `Vec<u8>`)
- [ ] Output format context configuration (`mov` format)
- [ ] Muxer flags: `frag_keyframe`, `empty_moov`, `default_base_moof`
- [ ] Initialization segment (`init.mp4`) generation
- [ ] Media segment (`segment_N.m4s`) generation
- [ ] Proper PTS/DTS handling and timebase conversion

**Tests:**
```rust
#[test]
fn test_init_segment_generation() {
    let init = generate_init_segment(&index).unwrap();
    assert!(init.len() > 0);
    assert!(validate_mp4_box(&init, b"ftyp"));
    assert!(validate_mp4_box(&init, b"moov"));
}

#[test]
fn test_media_segment_generation() {
    let segment = generate_media_segment(&index, 0).unwrap();
    assert!(segment.len() > 0);
    assert!(validate_mp4_box(&segment, b"moof"));
    assert!(validate_mp4_box(&segment, b"mdat"));
}

#[test]
fn test_segment_playable_in_vlc() {
    // Integration test: write to temp file, play in VLC
    let segment = generate_media_segment(&index, 0).unwrap();
    std::fs::write("/tmp/test.m4s", &segment).unwrap();
    // Manual verification or use ffprobe
}
```

---

### Milestone 5: Audio Transcoding Pipeline
**Goal:** Transcode non-AAC audio to AAC in-memory.

**Deliverables:**
- [ ] Audio decoder initialization (from source stream)
- [ ] Audio resampler (`SwrContext`) for 48kHz conversion
- [ ] AAC encoder initialization (`libfdk_aac` or `aac`)
- [ ] Packet interleaving (audio packets aligned with video segments)
- [ ] Encoder flush between segments (or persistent encoder)
- [ ] Memory buffer management for encoded packets

**Tests:**
```rust
#[test]
fn test_ac3_to_aac_transcode() {
    let encoded = transcode_audio_segment("test_ac3.mp4", 0).unwrap();
    assert!(encoded.len() > 0);
    // Verify AAC codec via ffprobe
}

#[test]
fn test_audio_sample_rate_conversion() {
    // 44.1kHz source should become 48kHz
    let info = get_audio_info(&encoded).unwrap();
    assert_eq!(info.sample_rate, 48000);
}

#[test]
fn test_audio_sync_with_video() {
    // PTS of audio segment should match video segment
    let video_pts = get_video_segment_pts(0).unwrap();
    let audio_pts = get_audio_segment_pts(0).unwrap();
    assert!((video_pts - audio_pts).abs() < 1000); // Within 1ms
}
```

---

### Milestone 6: Subtitle Extraction & WebVTT Conversion
**Goal:** Extract subtitle text and convert to WebVTT on-the-fly.

**Deliverables:**
- [ ] Subtitle packet decoding (`avcodec_decode_subtitle2`)
- [ ] Text extraction from `AVSubtitle` structs
- [ ] HTML entity escaping (`&`, `<`, `>`, etc.)
- [ ] WebVTT format generation (timestamps, text, styles)
- [ ] `X-TIMESTAMP-MAP` header for HLS sync
- [ ] ASS/SSA style conversion (optional, strip if complex)
- [ ] Bitmap subtitle detection (PGS/DVB → exclude from HLS)

**Tests:**
```rust
#[test]
fn test_subtitle_extraction() {
    let vtt = generate_vtt_segment("test.mp4", 0).unwrap();
    assert!(vtt.contains("WEBVTT"));
    assert!(vtt.contains("X-TIMESTAMP-MAP"));
}

#[test]
fn test_subtitle_html_escaping() {
    let vtt = generate_vtt_segment("test_special_chars.mp4", 0).unwrap();
    assert!(vtt.contains("&amp;")); // & should be escaped
    assert!(vtt.contains("&lt;"));  // < should be escaped
}

#[test]
fn test_bitmap_subtitle_exclusion() {
    let index = scan_file("test_pgs.mp4").unwrap();
    let subs: Vec<_> = index.subtitle_streams.iter()
        .filter(|s| is_text_subtitle(s.codec_id))
        .collect();
    assert_eq!(subs.len(), 0); // PGS should be excluded
}
```

---

### Milestone 7: Playlist Generation
**Goal:** Generate all HLS playlist files (master + variants).

**Deliverables:**
- [ ] `master.m3u8` generation:
  - Video variant entries (CODECS, BANDWIDTH, RESOLUTION)
  - Audio `#EXT-X-MEDIA` entries (GROUP-ID, LANGUAGE, DEFAULT)
  - Subtitle `#EXT-X-MEDIA` entries (TYPE=SUBTITLES)
- [ ] Video variant playlist (`video.m3u8`):
  - `#EXT-X-VERSION:7`
  - `#EXT-X-TARGETDURATION`
  - `#EXT-X-MEDIA-SEQUENCE:0`
  - `#EXT-X-PLAYLIST-TYPE:VOD`
  - Segment entries with `#EXTINF`
  - `#EXT-X-ENDLIST`
- [ ] Audio variant playlists (`audio_en.m3u8`, etc.)
- [ ] Subtitle variant playlists (`sub_en.m3u8`, etc.)
- [ ] Codec string generation (`avc1.42001e`, `mp4a.40.2`, `ac-3`, etc.)

**Tests:**
```rust
#[test]
fn test_master_playlist_structure() {
    let master = generate_master_playlist(&index).unwrap();
    assert!(master.contains("#EXTM3U"));
    assert!(master.contains("#EXT-X-STREAM-INF"));
    assert!(master.contains("TYPE=AUDIO"));
    assert!(master.contains("TYPE=SUBTITLES"));
}

#[test]
fn test_codec_strings() {
    let master = generate_master_playlist(&index).unwrap();
    assert!(master.contains("CODECS=\"avc1."));
    assert!(master.contains("CODECS=\"mp4a."));
}

#[test]
fn test_vod_playlist_type() {
    let variant = generate_variant_playlist(&index, VariantType::Video).unwrap();
    assert!(variant.contains("#EXT-X-PLAYLIST-TYPE:VOD"));
    assert!(variant.contains("#EXT-X-ENDLIST"));
}
```

---

### Milestone 8: HTTP Server & Caching
**Goal:** Serve all endpoints with proper caching and headers.

**Deliverables:**
- [ ] Axum router setup with all routes
- [ ] Request handlers for all endpoint types
- [ ] LRU cache implementation (`segment_cache` in AppState)
- [ ] Cache eviction policy (memory limit + TTL)
- [ ] HTTP headers:
  - `Content-Type` (application/vnd.apple.mpegurl, video/mp4, text/vtt)
  - `Cache-Control` (no-cache for playlists, max-age for segments)
  - `Accept-Ranges` (for potential byte-range support)
- [ ] CORS middleware (for web player testing)
- [ ] Graceful shutdown handling

**Tests:**
```rust
#[test]
fn test_master_playlist_endpoint() {
    let response = client.get("/streams/abc/master.m3u8").send().await;
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["content-type"], "application/vnd.apple.mpegurl");
}

#[test]
fn test_segment_caching() {
    // First request should be slow (disk read)
    let t1 = measure_time(|| client.get("/streams/abc/video_0.m4s").send().await);
    // Second request should be fast (cache hit)
    let t2 = measure_time(|| client.get("/streams/abc/video_0.m4s").send().await);
    assert!(t2 < t1 / 2);
}

#[test]
fn test_cache_eviction() {
    // Request more segments than cache limit
    // Verify old segments are evicted
}
```

---

### Milestone 9: Integration Testing
**Goal:** End-to-end testing with real media files and players.

**Deliverables:**
- [ ] Test suite with sample MP4/MKV files:
  - MP4 with AAC audio
  - MP4 with AC-3 audio
  - MP4 with AAC + AC-3 (multiple tracks)
  - MKV with subtitles (TTXT, ASS, SRT)
  - MKV with PGS subtitles (should be excluded)
- [ ] VLC playback verification (manual or automated)
- [ ] Browser playback verification (hls.js or native Safari)
- [ ] Seek functionality testing
- [ ] Multi-track selection testing (audio/subtitle switching)
- [ ] Performance benchmarks (startup time, segment latency, memory usage)

**Tests:**
```rust
#[tokio::test]
async fn test_full_stream_aac_mp4() {
    let stream_id = start_stream("test_aac.mp4").await;
    let master = fetch_master_playlist(&stream_id).await;
    assert!(master.contains("video.m3u8"));
    
    // Play through all segments
    for i in 0..10 {
        let segment = fetch_video_segment(&stream_id, i).await;
        assert!(segment.len() > 0);
    }
}

#[tokio::test]
async fn test_audio_track_switching() {
    let stream_id = start_stream("test_multi_audio.mp4").await;
    let master = fetch_master_playlist(&stream_id).await;
    assert!(master.contains("GROUP-ID=\"audio\""));
    assert!(master.contains("LANGUAGE=\"en\""));
    assert!(master.contains("LANGUAGE=\"es\""));
}

#[tokio::test]
async fn test_subtitle_sync() {
    let stream_id = start_stream("test_subs.mp4").await;
    let vtt = fetch_subtitle_segment(&stream_id, 0).await;
    assert!(vtt.contains("X-TIMESTAMP-MAP"));
    // Verify timestamps align with video segment
}
```

---

### Milestone 10: Production Hardening
**Goal:** Prepare for real-world deployment.

**Deliverables:**
- [ ] Configuration file support (TOML/YAML)
- [ ] Logging configuration (JSON output, log levels)
- [ ] Metrics endpoint (Prometheus-compatible)
- [ ] Health check endpoint (`/health`)
- [ ] Rate limiting (prevent abuse)
- [ ] Connection limits (max concurrent streams)
- [ ] Memory limit enforcement (OOM prevention)
- [ ] Docker containerization
- [ ] Documentation (README, API docs, deployment guide)

**Tests:**
```rust
#[tokio::test]
async fn test_memory_limit_enforcement() {
    // Start many concurrent streams
    // Verify server doesn't OOM, evicts cache properly
}

#[tokio::test]
async fn test_health_endpoint() {
    let response = client.get("/health").send().await;
    assert_eq!(response.status(), 200);
}
```

---

## 6. Key Technical Decisions

### 6.1 FFmpeg Muxer Configuration for fMP4
```rust
// Critical flags for HLS-compatible fMP4
let mut opts = AVDictionary::new();
opts.set("movflags", "frag_keyframe+empty_moov+default_base_moof", 0);
opts.set("frag_duration", "0", 0);  // Fragment at keyframes only
opts.set("segment_format", "mp4", 0);
```

### 6.2 Audio Transcoding Strategy
| Source Codec | Action | Output Codec |
|--------------|--------|--------------|
| AAC | Copy | AAC (no transcode) |
| AC-3 (alone) | Copy + Transcode | AC-3 + AAC |
| AC-3 + AAC | Copy Both | AC-3 + AAC (no transcode) |
| E-AC-3 | Copy + Transcode | E-AC-3 + AAC |
| Opus/MP3 | Transcode | AAC |
| FLAC/PCM | Transcode | AAC |

### 6.3 Cache Strategy
```rust
CacheConfig {
    max_memory_mb: 512,        // Adjust based on deployment
    max_segments: 100,         // ~400 seconds of content at 4s/segment
    ttl_secs: 300,             // 5 minutes
    eviction_policy: "LRU",
}
```

### 6.4 Segment Duration
- **Default:** 4 seconds (HLS recommendation)
- **Adjustment:** Align to keyframe intervals (GOP)
- **Tolerance:** 3-6 seconds acceptable (update `TARGETDURATION`)

---

## 7. Risk Mitigation

| Risk | Mitigation |
|------|------------|
| **FFmpeg library version mismatch** | Pin `ffmpeg-next` version, document required system libraries |
| **MKV indexing timeout** | Implement timeout, fallback to dynamic playlist |
| **Memory exhaustion** | LRU cache with hard memory limit, segment eviction |
| **Audio/video sync drift** | Use PTS from source, don't reset timestamps between segments |
| **Subtitle encoding issues** | UTF-8 validation, HTML entity escaping, fallback to plain text |
| **HDD seek performance** | Sequential background scan, aggressive caching |
| **Client compatibility** | Test with Safari, Chrome, VLC, hls.js |

---

## 8. Success Criteria

1. **Startup Time:** < 5 seconds for indexed files (MP4), < 30 seconds for unindexed (MKV with fallback)
2. **Segment Latency:** < 100ms for cached segments, < 500ms for cache miss (HDD)
3. **Memory Usage:** < 1GB for 2-hour movie with 512MB cache
4. **CPU Usage:** < 10% for direct copy, < 50% with audio transcoding
5. **Compatibility:** Plays in Safari (iOS/macOS), Chrome (Android), VLC
6. **No Disk Writes:** Verified via file system monitoring

---

## 9. Next Steps for qwen-code

1. **Start with Milestone 1** - Get FFmpeg working in Rust
2. **Proceed sequentially** - Each milestone builds on the previous
3. **Run tests after each milestone** - Don't accumulate technical debt
4. **Document as you go** - Update README with working features
5. **Test with real media** - Use actual MP4/MKV files early and often

This plan provides a complete roadmap for implementing the HLS streaming server. Each milestone is self-contained and testable, allowing for iterative development with clear success criteria.
