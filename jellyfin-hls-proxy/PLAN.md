# Jellyfin HLS Proxy - Implementation Plan

## Overview

This document outlines the implementation plan for the Jellyfin HLS transmuxing proxy. The proxy will intercept Jellyfin media playback requests and handle transcoding/transmuxing using `hls-vod-lib` instead of relying on Jellyfin's ffmpeg-based approach.

---

## Architecture

```
┌─────────────┐     ┌──────────────────────┐     ┌─────────────┐
│   Client    │────▶│  HLS Proxy (axum)    │────▶│   Jellyfin  │
│  (Browser,  │     │ - Intercepts         │     │   Server    │
│   Mobile,   │◀────│ - Proxies            │◀────│             │
│    etc.)    │     │ - Generates HLS      │     │             │
└─────────────┘     └──────────────────────┘     └─────────────┘
                           │
                           ▼
                  ┌──────────────────────┐
                  │   hls-vod-lib        │
                  │   (workspace crate)  │
                  └──────────────────────┘
```

---

## Milestones

### Milestone 1: Project Setup
**Goal:** Establish the basic project structure and dependencies.

- [ ] 1.1 Create `Cargo.toml` with required dependencies:
  - `axum` ≥ 0.8.0
  - `axum-server` ≥ 0.8.0
  - `tokio` ≥ 1.49.0 (with full features)
  - `clap` ≥ 4.5.60 (with derive feature)
  - `hls-vod-lib` (workspace dependency)
  - `hyper-util` (for proxying)
  - `tower-http` (for middleware)
  - `serde` + `serde_json` (for JSON handling)
  - `tracing` + `tracing-subscriber` (for logging)
  - `thiserror` (for error handling)

- [ ] 1.2 Create basic project structure:
  ```
  src/
  ├── main.rs          # Entry point, CLI parsing
  ├── config.rs        # Configuration structures
  ├── error.rs         # Error types
  ├── proxy.rs         # HTTP proxy logic
  ├── jellyfin/        # Jellyfin API types and handling
  │   ├── mod.rs
  │   ├── types.rs     # API request/response types
  │   └── client.rs    # Jellyfin HTTP client
  ├── handler/         # Request handlers
  │   ├── mod.rs
  │   ├── playback.rs  # PlaybackInfo interception
  │   └── proxymedia.rs # HLS generation endpoint
  └── server.rs        # Axum server setup
  ```

- [ ] 1.3 Implement basic CLI with clap:
  - `--bind` / `-b`: Bind address (default: 127.0.0.1:8096)
  - `--jellyfin` / `-j`: Jellyfin backend URL (default: http://127.0.0.1:8096)
  - `--tls-cert`: Optional TLS certificate path
  - `--tls-key`: Optional TLS key path
  - `--log-level`: Logging verbosity

- [ ] 1.4 Set up basic logging with tracing

**Deliverable:** A compilable binary that starts an HTTP server and logs requests.

---

### Milestone 2: Basic HTTP Proxy
**Goal:** Implement transparent HTTP proxying to Jellyfin backend.

- [ ] 2.1 Implement reverse proxy middleware:
  - Forward all requests to Jellyfin backend
  - Preserve headers, query parameters, and body
  - Handle response forwarding back to client

- [ ] 2.2 Implement connection management:
  - Connection pooling for backend requests
  - Proper timeout handling
  - Error recovery

- [ ] 2.3 Add request/response logging:
  - Log proxied requests (method, path, status)
  - Optional debug logging for headers/bodies

- [ ] 2.4 Handle WebSocket upgrade (for Jellyfin's real-time features):
  - Detect WebSocket upgrade requests
  - Forward WebSocket connections appropriately

**Deliverable:** A working reverse proxy that transparently forwards all traffic to Jellyfin.

---

### Milestone 3: Jellyfin API Types
**Goal:** Define Rust types for Jellyfin API interactions.

- [ ] 3.1 Define PlaybackInfo request types:
  - `PlaybackInfoRequest` struct
  - `DeviceInfo` struct
  - `PlayerProfile` struct
  - `DirectPlayProfile` struct
  - `TranscodingProfile` struct
  - `SubtitleProfile` struct

- [ ] 3.2 Define PlaybackInfo response types:
  - `PlaybackInfoResponse` struct
  - `MediaSourceInfo` struct
  - `MediaStream` struct (audio, video, subtitle tracks)
  - `TranscodingInfo` struct

- [ ] 3.3 Implement serde serialization/deserialization:
  - Handle Jellyfin's PascalCase JSON naming convention
  - Handle optional fields appropriately

- [ ] 3.4 Define additional API types as needed:
  - Authentication types (if needed)
  - Item/User types (if needed)

**Deliverable:** Complete type definitions for Jellyfin API interaction.

---

### Milestone 4: PlaybackInfo Interception
**Goal:** Intercept and modify playback negotiation requests.

- [ ] 4.1 Implement request interception for `POST /Items/{ItemId}/PlaybackInfo`:
  - Route matching for playback info endpoints
  - Extract ItemId and other path parameters

- [ ] 4.2 Modify PlaybackInfo requests:
  - Inject `DirectPlayProfiles`:
    - Containers: mp4, m4v, mkv, webm
    - Video codecs: h264, h265, hevc, vp9
    - Audio codecs: aac, mp3, ac3, eac3, opus
  - Set `TranscodingProfiles` to empty
  - Preserve other request fields

- [ ] 4.3 Parse PlaybackInfo responses:
  - Extract file path from `MediaSourceInfo.Path`
  - Extract media stream information
  - Determine if direct play is possible

- [ ] 4.4 Generate transcoding URLs when needed:
  - Create URL format: `/proxymedia/{path}.{container}.m3u8`
  - Include query parameters for:
    - Audio track selection
    - Video track selection
    - Subtitle track selection
    - Transcoding flags (audio transcoding needed)

- [ ] 4.5 Modify PlaybackInfo response:
  - Replace transcoding URLs with proxy URLs
  - Preserve direct play URLs for compatible media

**Deliverable:** PlaybackInfo requests are intercepted, modified, and responses are rewritten with proxy URLs.

---

### Milestone 5: HLS Generation Endpoint
**Goal:** Implement the `/proxymedia/*` endpoint for HLS playlist generation.

- [ ] 5.1 Implement route handler for `/proxymedia/*path`:
  - Parse path to extract original file path and container
  - Parse query parameters for track selection

- [ ] 5.2 Integrate with `hls-vod-lib`:
  - Initialize HLS generator with file path
  - Configure audio/video track selection
  - Configure subtitle handling
  - Set up audio transcoding if needed (to AAC)

- [ ] 5.3 Implement master playlist generation:
  - Generate HLS master playlist (.m3u8)
  - Include variant streams for different qualities
  - Include subtitle renditions if applicable

- [ ] 5.4 Implement media playlist generation:
  - Generate media playlists for each variant
  - Handle segment requests
  - Stream segments to client

- [ ] 5.5 Implement segment streaming:
  - Generate segments on-the-fly
  - Stream to client with proper content-type
  - Handle byte-range requests for seeking

**Deliverable:** Working HLS endpoint that generates playlists and streams segments.

---

### Milestone 6: Audio Transcoding Support
**Goal:** Add audio transcoding capability for incompatible audio codecs.

- [ ] 6.1 Detect when audio transcoding is needed:
  - Compare source audio codec with client capabilities
  - Flag streams requiring transcoding

- [ ] 6.2 Integrate audio transcoding with `hls-vod-lib`:
  - Configure transcoder for AAC output
  - Handle supported input codecs (ac3, eac3, opus, etc.)

- [ ] 6.3 Implement transmuxing path:
  - For compatible codecs, pass through without transcoding
  - For incompatible codecs, transcode to AAC

- [ ] 6.4 Handle subtitle burning (optional):
  - Detect when subtitles need to be burned in
  - Configure subtitle rendering

**Deliverable:** Audio transcoding works for incompatible codecs.

---

### Milestone 7: TLS Support
**Goal:** Add optional HTTPS/TLS support.

- [ ] 7.1 Integrate `axum-server` for TLS:
  - Load certificate and key from files
  - Configure TLS settings

- [ ] 7.2 Handle both HTTP and HTTPS:
  - Support HTTP-only mode (no cert/key provided)
  - Support HTTPS-only mode (cert/key provided)
  - Optional: Support both simultaneously

- [ ] 7.3 Update proxy to handle HTTPS backend:
  - Support proxying to HTTPS Jellyfin instances
  - Handle certificate validation options

**Deliverable:** Server can run with TLS encryption.

---

### Milestone 8: Production Readiness
**Goal:** Polish the implementation for production use.

- [ ] 8.1 Error handling and recovery:
  - Graceful error messages to clients
  - Proper HTTP status codes
  - Fallback to direct Jellyfin proxying on errors

- [ ] 8.2 Performance optimizations:
  - Buffer management for streaming
  - Connection pooling tuning
  - Memory-efficient segment handling

- [ ] 8.3 Configuration validation:
  - Validate Jellyfin backend URL
  - Validate TLS certificate paths
  - Validate bind address

- [ ] 8.4 Health check endpoint:
  - Implement `/health` endpoint
  - Check backend connectivity

- [ ] 8.5 Logging and diagnostics:
  - Structured logging
  - Request tracing
  - Performance metrics (optional)

- [ ] 8.6 Documentation:
  - README with usage instructions
  - Configuration examples
  - Deployment guide

**Deliverable:** Production-ready proxy server.

---

### Milestone 9: Testing
**Goal:** Comprehensive testing of all functionality.

- [ ] 9.1 Unit tests:
  - JSON serialization/deserialization
  - URL parsing and generation
  - Configuration parsing

- [ ] 9.2 Integration tests:
  - PlaybackInfo interception
  - HLS generation
  - Proxy functionality

- [ ] 9.3 Client compatibility testing:
  - Test with Jellyfin web client
  - Test with mobile apps
  - Test with external players (VLC, etc.)

**Deliverable:** Test suite with good coverage.

---

## Dependencies Reference

### Jellyfin API Endpoints Used

| Method | Endpoint | Purpose |
|--------|----------|---------|
| POST | `/Items/{ItemId}/PlaybackInfo` | Get playback information |
| GET | `/Videos/{ItemId}/stream` | Direct stream (proxied) |
| GET | `/Videos/{ItemId}/stream.m3u8` | HLS stream (intercepted) |

### DirectPlay Profile Format

```json
{
  "Container": "mp4,m4v,mkv,webm",
  "Type": "Video",
  "VideoCodec": "h264,h265,hevc,vp9",
  "AudioCodec": "aac,mp3,ac3,eac3,opus"
}
```

### Proxy URL Format

```
/proxymedia/{encoded_path}.{container}.m3u8?
  audio={stream_index}&
  video={stream-index}&
  subtitle={stream-index}&
  transcodeAudio={true|false}
```

---

## Risk Assessment

| Risk | Impact | Mitigation |
|------|--------|------------|
| `hls-vod-lib` API changes | High | Close coordination with workspace, pin versions |
| Jellyfin API changes | Medium | Test against multiple Jellyfin versions |
| Client compatibility issues | High | Extensive testing with various clients |
| Performance bottlenecks | Medium | Early profiling, efficient streaming |

---

## Future Enhancements (Out of Scope)

- Video transcoding (currently only audio transcoding to AAC)
- Adaptive bitrate streaming with multiple quality variants
- Caching of generated segments
- Authentication/authorization layer
- Rate limiting
- Multi-server load balancing
