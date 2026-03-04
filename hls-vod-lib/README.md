# hls-vod-lib 🦀

A high-performance Rust library for on-the-fly HLS (HTTP Live Streaming) transmuxing and transcoding.

## Overview

`hls-vod-lib` is a specialized media processing engine that generates HLS master playlists, variant playlists, and fMP4 media segments directly from source files (MKV, MP4, WebM, etc.) in memory. It eliminates the traditional need to pre-segment files or use external CLI tools like `ffmpeg` to write chunks to disk.

While originally designed as the core engine for `jellyfin-transmux-proxy`, this library is built to be modular and suitable for any standalone Rust project that requires dynamic HLS generation.

## Features

- **In-Memory Transmuxing**: Converts container formats to HLS-compliant fMP4 segments on-the-fly without temporary disk storage.
- **Audio Transcoding**: Built-in support for transcoding audio streams to AAC (using `ffmpeg-next` / C-API) when the source codec is incompatible with the codecs the client can play.
- **Interleaved A/V Streams**: Supports generating single fMP4 segments containing both audio and video tracks, perfectly interleaved by DTS.
- **Dynamic Playlists**: Generates HLS Master and Media playlists based on source file probing and requested constraints.
- **Seeking Support**: Provides frame-accurate segment generation, enabling ultra-low latency seeking in players.
- **FFmpeg Integration**: Integration with the FFmpeg libraries via `ffmpeg-next` for robust demuxing, decoding, and encoding.
- **Threading**: does lookahead caching of audio and video segments so that they are already in memory when the client requests them, and so that they can be generated in parallel- this significantly speeds up audio transccoding on slower CPUs.
- **Multiple Audio Tracks**: Supports multiple audio tracks, accurately multiplexing them into HLS variant playlists.
- **Subtitle Support**: Extracts and serves embedded subtitles (tx3g, srt, vtt) as WebVTT segments.

## Use Cases

- **Media Proxies**: Build lightweight edge servers that "trick" clients into seeing optimized streams (like `jellyfin-transmux-proxy`).
- **Custom Streaming Servers**: Integrate directly into Rust-based media servers or content delivery platforms.
- **Jellyfin Integration**: Could potentially serve as the foundation for a more efficient, native media delivery branch in Rust-compatible Jellyfin forks or associated tools.

## Status

Currently supports **transmuxing** for both Video and Audio, and **transcoding** for Audio. Video transcoding is a not-yet-planned future addition.