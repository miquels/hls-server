# Jellyfin HLS Proxy Status

## Milestone 1: Project Initialization & Basic Reverse Proxy
**Status**: Completed

**Summary:**
- Configured Cargo workspace and initialized the `jellyfin-hls-proxy` project (`Cargo.toml`).
- Added dependencies: `axum`, `tokio`, `clap`, and `reqwest`.
- Implemented `src/main.rs` with a basic CLI using `clap` (listening port and upstream URL).
- Set up an Axum server with a `fallback` route that transparently proxies all HTTP requests to the upstream Jellyfin backend using `reqwest`.
- Fixed `Cargo.toml` workspace missing member error and verified successful compilation (`cargo check`).

## Milestone 2: Intercepting PlaybackInfo Requests
**Status**: Completed

**Summary:**
- Switched the HTTP server implementation from `axum::serve` to `axum_server::bind` (v0.8.0).
- Handled the `/Items/:item_id/PlaybackInfo` route specifically to intercept client device capabilities.
- Deserialized the incoming proxy request using `serde_json` and mutated the root `DeviceProfile`.
- Automatically injected a custom `DirectPlayProfile` for all standard containers (`"mp4,m4v,mkv,webm"`) and codecs (`"h264,h265,vp9"` / `"aac,mp3,ac3,eac3,opus"`).
- Explicitly cleared out the `TranscodingProfiles` to force Jellyfin to rely on the proxy.
- Forwarded the modified request downstream to the backend.

## Milestone 3: Processing PlaybackInfo Responses
**Status**: Completed

**Summary:**
- Intercepted the downstream Jellyfin JSON response in the `playback_info_handler`.
- Parsed the Jellyfin device capability evaluation to locate the `MediaSources` array.
- Extracted the physical file `Path` for each media source.
- Generated a mapped proxy `TranscodingUrl` (`/proxymedia/...`) that points to our standalone HLS handlers.
- Modified proxy response indicating that `TranscodingContainer` is `ts` (to mimic standard behavior) and set `SupportsTranscoding` to `true`, forcing the Jellyfin client to use our server for streaming.
- Successfully repacked and returned the modified JSON body with adjusted `CONTENT_LENGTH` to the requesting client.

## Milestone 4: HLS Playlist and Segment Handlers
**Status**: Completed

**Summary:**
- Implemented a dedicated Axum handler for `GET /proxymedia/*path`.
- Extracted and decoded the upstream media `path` from the URL parameter.
- Integrated `hls_vod_lib::HlsParams::parse` to parse the requested HLS entity (MainPlaylist, Audio/Video Segments, VTT subtitles).
- Invoked `HlsVideo::open` and `hls_video.generate()` to seamlessly scan the video file on-the-fly and perform required segmentation/transmuxing without shelling out to `ffmpeg`.
- Correctly parsed incoming proxy query parameters (`codecs`, `tracks`, `interleave`) to filter and stream the custom track combinations desired by the client.
- Handled MIME types, `CACHE-CONTROL`, and binary HLS generation synchronously within a `tokio::task::spawn_blocking` thread to prevent starving the Axum async executor.
