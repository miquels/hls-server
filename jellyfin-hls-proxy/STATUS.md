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
