# Jellyfin HLS Proxy Implementation Plan

This document outlines the implementation plan for the Jellyfin HLS reverse proxy using `axum` and `hls-vod-lib`.

## Milestones

### Milestone 1: Project Initialization & Basic Reverse Proxy
*Goal: Set up the application skeleton and establish a transparent reverse proxy to the Jellyfin backend.*
- Initialize the `jellyfin-hls-proxy` Cargo workspace.
- Add required dependencies: `axum` (>=0.8.0), `tokio` (>=1.49.0), `clap` (>=4.5.60), and an HTTP client (e.g., `reqwest` or `hyper`) for proxying.
- Add `hls-vod-lib` as a path dependency.
- Create the basic CLI structure using `clap` to accept the listen address and the upstream Jellyfin URL.
- Implement a fallback Axum route (`/*path`) that transparently forwards all GET/POST/etc. requests to the upstream server and streams the responses back to the client.

### Milestone 2: Intercepting PlaybackInfo Requests
*Goal: Hook into the media negotiation phase to inject the proxy's capabilities.*
- Add a specific Axum route for `POST /Items/:ItemId/PlaybackInfo` (or a middleware approach if paths vary).
- Deserialize the incoming JSON request from the client.
- Modify the `DirectPlayProfiles` to explicitly support standard web containers and codecs:
  - Containers: `mp4, m4v, mkv, webm`
  - Video codecs: `h264, h265, vp9`
  - Audio codecs: `aac, mp3, ac3, eac3, opus`
- Clear out the `TranscodingProfiles` list to force Jellyfin to rely on the proxy's DirectPlay/Transmuxing capabilities.
- Forward the mutated JSON body to the Jellyfin backend.

### Milestone 3: Processing PlaybackInfo Responses
*Goal: Parse the Jellyfin backend response and rewrite media URLs to point to our transmuxing proxy endpoints.*
- Receive the `PlaybackInfo` JSON response from the Jellyfin backend.
- Extract the underlying file `Path` from the media sources.
- Implement logic to evaluate if the media can be DirectPlayed or if it needs transmuxing/transcoding via our proxy.
- If transmuxing is required, overwrite the media source URLs in the JSON response payload.
- Generate a custom `TranscodingUrl` pointing to our own namespace, for example: `/proxymedia/.../master.m3u8`. The URL path should encode necessary track selections and the target file path.
- Return the modified JSON response to the client.

### Milestone 4: HLS Playlist and Segment Handlers
*Goal: Serve actual HLS playlists and media segments using `hls-vod-lib`.*
- Implement the Axum handlers for the `/proxymedia/*` namespace to handle `.m3u8` (playlists), `.mp4` (init segments), and `.m4s` (media fragments) requests.
- Use `hls_vod_lib::HlsParams` for parsing the playback parameters from the URL.
- Use `hls_vod_lib::media::StreamInfo` and `hls_vod_lib::hlsvideo::HlsVideo` to scan the media file located at the intercepted `Path`.
- Adapt the handler logic from `hls-vod-server/src/http/dynamic.rs` to plug the Axum request context into the `hls_vod_lib` backend.
- Ensure all transmuxing (and audio-to-aac transcoding) is handled purely by `hls-vod-lib` without shelling out to raw `ffmpeg` command lines.

### Milestone 5: Polish & Testing
*Goal: Ensure stability, proper error handling, testing, and performance.*
- Add proper structured logging (`tracing`) for incoming requests, proxied requests, and HLS segment generation times.
- Verify audio/video interleaving and audio transcoding functionality.
- Write unit tests for request mutation (Milestone 2 & 3).
- Perform end-to-end testing with a real Jellyfin backend and web client. 
- Graceful shutdown handling and configuration polishing.
