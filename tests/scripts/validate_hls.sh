#!/bin/bash
# HLS Stream Validator
# Downloads and validates all segments in an HLS stream

set -e

BASE_URL="${1:-http://localhost:3000/streams}"
STREAM_ID="${2:-$(curl -s -X POST "$BASE_URL" -H "Content-Type: application/json" -d '{"path": "/Users/mikevs/Devel/hls-server/testvideos/bun33s.mp4"}' | jq -r '.stream_id')}"

echo "=== HLS Stream Validator ==="
echo "Stream ID: $STREAM_ID"
echo "Base URL: $BASE_URL"
echo ""

# Create temp directory
TEMP_DIR=$(mktemp -d)
echo "Temp directory: $TEMP_DIR"
cd "$TEMP_DIR"

# Fetch master playlist
echo ""
echo "=== Fetching Master Playlist ==="
curl -s "$BASE_URL/$STREAM_ID/master.m3u8" -o master.m3u8
cat master.m3u8

# Fetch video playlist
echo ""
echo "=== Fetching Video Playlist ==="
curl -s "$BASE_URL/$STREAM_ID/video.m3u8" -o video.m3u8
cat video.m3u8

# Fetch audio playlist
echo ""
echo "=== Fetching Audio Playlist ==="
curl -s "$BASE_URL/$STREAM_ID/audio/en.m3u8" -o audio_en.m3u8
cat audio_en.m3u8

# Download init segments
echo ""
echo "=== Downloading Init Segments ==="
curl -s "$BASE_URL/$STREAM_ID/video/init.mp4" -o video_init.mp4
echo "Video init: $(ls -la video_init.mp4 | awk '{print $5}') bytes"
mp4box -info video_init.mp4 2>&1 | head -20

curl -s "$BASE_URL/$STREAM_ID/audio/en/init.mp4" -o audio_init.mp4
echo "Audio init: $(ls -la audio_init.mp4 | awk '{print $5}') bytes"
mp4box -info audio_init.mp4 2>&1 | head -20

# Download video segments
echo ""
echo "=== Downloading Video Segments ==="
grep -oP 'video/\d+\.m4s' video.m3u8 | while read seg; do
    echo "Downloading $seg..."
    curl -s "$BASE_URL/$STREAM_ID/$seg" -o "$seg"
done

# Download audio segments
echo ""
echo "=== Downloading Audio Segments ==="
grep -oP 'en/\d+\.m4s' audio_en.m3u8 | while read seg; do
    echo "Downloading $seg..."
    curl -s "$BASE_URL/$STREAM_ID/audio/$seg" -o "audio_$seg"
done

# Validate video segments
echo ""
echo "=== Validating Video Segments ==="
for seg in video/*.m4s 2>/dev/null; do
    if [ -f "$seg" ]; then
        echo "Validating $seg..."
        ffprobe -v error -show_entries format=duration -of default=noprint_wrappers=1 "$seg" 2>&1 || echo "FAILED: $seg"
    fi
done

# Validate audio segments
echo ""
echo "=== Validating Audio Segments ==="
for seg in audio_*.m4s 2>/dev/null; do
    if [ -f "$seg" ]; then
        echo "Validating $seg..."
        ffprobe -v error -show_entries format=duration -of default=noprint_wrappers=1 "$seg" 2>&1 || echo "FAILED: $seg"
    fi
done

# Try to combine init + first segment for video
echo ""
echo "=== Testing Video Playback (init + segment 0) ==="
cat video_init.mp4 video/0.m4s > video_test.mp4
ffprobe -v error -show_format -show_streams video_test.mp4 2>&1 | head -40

# Try to combine init + first segment for audio
echo ""
echo "=== Testing Audio Playback (init + segment 0) ==="
cat audio_init.mp4 audio_en/0.m4s > audio_test.mp4
ffprobe -v error -show_format -show_streams audio_test.mp4 2>&1 | head -40

# Run Apple validator if available
if command -v mediastreamvalidator &> /dev/null; then
    echo ""
    echo "=== Running Apple Media Stream Validator ==="
    mediastreamvalidator "$BASE_URL/$STREAM_ID/master.m3u8" 2>&1 | head -50
fi

echo ""
echo "=== Validation Complete ==="
echo "Temp directory preserved at: $TEMP_DIR"
