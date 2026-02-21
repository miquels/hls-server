# HLS Streaming Server

A high-performance Rust-based HLS (HTTP Live Streaming) server that serves MP4/MKV files as fragmented MP4 (fMP4/CMAF) segments without video transcoding. Features intelligent audio track handling and on-the-fly subtitle conversion to WebVTT.

## Features

- **No Video Transcoding**: Direct copy of video streams for minimal CPU usage
- **Intelligent Audio Handling**: 
  - AAC audio served directly
  - AC-3/E-AC-3/Opus transcoded to AAC for compatibility
- **Subtitle Conversion**: On-the-fly conversion to WebVTT format
- **In-Memory Operations**: No disk writes for segments
- **HLS Compatible**: Works with Safari, Chrome, VLC, and hls.js
- **Multi-Language Support**: Multiple audio and subtitle tracks
- **LRU Caching**: Configurable memory-limited segment cache
- **Production Ready**: Rate limiting, connection limits, metrics

## Quick Start

### Prerequisites

- Rust 1.75+ 
- FFmpeg 7.0+ development libraries

**Ubuntu/Debian:**
```bash
sudo apt-get install clang libavcodec-dev libavformat-dev libavutil-dev \
     libavfilter-dev libswscale-dev libswresample-dev pkg-config build-essential
```

**macOS:**
```bash
brew install ffmpeg pkg-config
```

### Build

```bash
cargo build --release
```

### Run

```bash
# With default settings
./target/release/hls-server

# With custom config
./target/release/hls-server --config /path/to/config.toml
```

### Docker

```bash
# Build and run
docker-compose up -d

# Or build manually
docker build -t hls-server .
docker run -p 3000:3000 -v ./media:/app/media:ro hls-server
```

## API Endpoints

### Stream Management

| Endpoint | Method | Description |
|----------|--------|-------------|
| `GET /debug/streams` | List all active cached streams |
| `GET /debug/cache` | Get cache statistics |

### Playlists

| Endpoint | Description |
|----------|-------------|
| `GET /{*path}.mp4.as.m3u8` | Master playlist for an MP4 file |
| `GET /{*path}.mkv.as.m3u8` | Master playlist for an MKV file |
| `GET /{*path}.webm.as.m3u8` | Master playlist for a WebM file |
| `GET /{*path}.ext/v/media.m3u8` | Video variant playlist |
| `GET /{*path}.ext/a/{track}.m3u8` | Audio variant playlist |
| `GET /{*path}.ext/s/{track}.m3u8` | Subtitle variant playlist |

### Segments

| Endpoint | Description |
|----------|-------------|
| `GET /{*path}.ext/v/{track}.init.mp4` | Video initialization segment (fMP4 header) |
| `GET /{*path}.ext/v/{track}.{n}.m4s` | Video segment |
| `GET /{*path}.ext/a/{track}.init.mp4` | Audio initialization segment |
| `GET /{*path}.ext/a/{track}.{n}.m4s` | Audio segment |
| `GET /{*path}.ext/s/{track}.{n}.vtt` | Subtitle segment (WebVTT) |

### Monitoring

| Endpoint | Description |
|----------|-------------|
| `GET /health` | Health check |
| `GET /version` | Server version |
| `GET /metrics` | Prometheus metrics |

## Usage Examples

### Play a Stream Directly via File Path

Streams are now implicitly registered when requested! You don't need a `POST` request to start a stream. Ensure you mount your media folder and make the request directly. For example, if you want to stream `/media/movies/video-alex.mp4`, simply append `.as.m3u8`.

**VLC:**
```bash
vlc http://localhost:3000/media/movies/video-alex.mp4.as.m3u8
```

**Browser (hls.js):**
```html
<video id="video" controls></video>
<script src="https://cdn.jsdelivr.net/npm/hls.js@latest"></script>
<script>
  const video = document.getElementById('video');
  const hls = new Hls();
  hls.loadSource('http://localhost:3000/media/movies/video-alex.mp4.as.m3u8');
  hls.attachMedia(video);
</script>
```

**iOS/macOS Safari:**
```html
<video controls src="http://localhost:3000/media/movies/video-alex.mp4.as.m3u8"></video>
```

### List Active Cached Streams

Streams are evicted from cache after an idle period. You can view currently active streams:
```bash
curl http://localhost:3000/debug/streams
```

## Configuration

Create `config.toml` (see `config.example.toml`):

```toml
[server]
host = "0.0.0.0"
port = 3000
cors_enabled = true

[cache]
max_memory_mb = 512
max_segments = 100
ttl_secs = 300

[segment]
target_duration_secs = 4.0

[audio]
target_sample_rate = 48000
aac_bitrate = 128000
enable_transcoding = true

[limits]
max_concurrent_streams = 100
rate_limit_rps = 100
```

## Metrics

Prometheus-compatible metrics at `/metrics`:

- `hls_server_uptime_seconds` - Server uptime
- `hls_requests_total` - Total HTTP requests
- `hls_bytes_served_total` - Total bytes served
- `hls_cache_hits_total` / `hls_cache_misses_total` - Cache statistics
- `hls_cache_hit_ratio` - Cache hit ratio
- `hls_active_streams` - Active stream count
- `hls_transcode_operations_total` - Transcoding operations
- `hls_errors_total` - Errors by type

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    HTTP Server (Axum)                    │
├─────────────────────────────────────────────────────────┤
│  Routes: Playlists, Segments, Metrics, Debug            │
└─────────────────────────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────┐
│                   Stream Manager                         │
├──────────────┬──────────────┬──────────────────────────┤
│ Stream Index │ Segment Cache│ Rate/Connection Limiter  │
└──────────────┴──────────────┴──────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────┐
│                  FFmpeg Processing                       │
├──────────────┬──────────────┬──────────────────────────┤
│   Demuxer    │ Audio Trans  │   WebVTT Converter       │
└──────────────┴──────────────┴──────────────────────────┘
```

## Supported Formats

### Input Containers
- MP4 (.mp4, .m4v)
- Matroska (.mkv)

### Video Codecs (Direct Copy)
- H.264/AVC
- H.265/HEVC
- VP9
- AV1

### Audio Codecs
| Codec | Action | Output |
|-------|--------|--------|
| AAC | Copy | AAC |
| AC-3 | Copy + Transcode | AC-3 + AAC |
| E-AC-3 | Copy + Transcode | E-AC-3 + AAC |
| Opus | Transcode | AAC |
| MP3 | Transcode | AAC |
| FLAC | Transcode | AAC |

### Subtitle Formats
| Format | Support | Output |
|--------|---------|--------|
| SubRip (SRT) | ✅ | WebVTT |
| ASS/SSA | ✅ | WebVTT |
| MOV Text | ✅ | WebVTT |
| WebVTT | ✅ | Pass-through |
| PGS/DVB | ❌ | Excluded |

## Testing

```bash
# Run all tests
cargo test

# Run integration tests
cargo test --test integration

# Run with coverage
cargo tarpaulin --out Html
```

## Performance

Typical performance on modern hardware:

- **Startup Time**: < 5 seconds (indexed MP4)
- **Segment Latency**: < 10ms (cached), < 100ms (cache miss)
- **Memory Usage**: < 1GB for 2-hour movie (512MB cache)
- **CPU Usage**: < 10% (direct copy), < 50% (with transcoding)

## License

MIT License - see LICENSE file for details.

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Run tests: `cargo test`
5. Submit a pull request

## Troubleshooting

### FFmpeg not found
Ensure FFmpeg development libraries are installed and `pkg-config` can find them:
```bash
pkg-config --libs libavcodec
```

### High memory usage
Reduce cache size in config:
```toml
[cache]
max_memory_mb = 256
```

### Playback issues
- Ensure video codec is H.264 or HEVC
- Check audio is AAC or will be transcoded
- Verify segment duration is 3-6 seconds
