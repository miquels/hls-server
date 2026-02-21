#! /usr/bin/env python3

import sys
import urllib.request
import re

def parse_m3u8(content):
    """Extracts VTT segment URLs and their specific durations."""
    lines = content.splitlines()
    segments = []
    for i, line in enumerate(lines):
        if line.startswith('#EXTINF:'):
            # Extract duration from #EXTINF:6.673,
            try:
                duration_match = re.search(r'#EXTINF:([\d.]+)', line)
                duration = float(duration_match.group(1))
                # The URL is always the next non-comment line
                for next_line in lines[i+1:]:
                    if not next_line.startswith('#'):
                        segments.append({'duration': duration, 'url': next_line.strip()})
                        break
            except (AttributeError, ValueError):
                continue
    return segments

def parse_vtt_time(time_str):
    """Converts HH:MM:SS.mmm or MM:SS.mmm to total seconds."""
    parts = time_str.split(':')
    if len(parts) == 2:
        m, s = parts
        h = 0
    else:
        h, m, s = parts
    return int(h) * 3600 + int(m) * 60 + float(s)

def check_vtt_segment(url, window_start, window_end, segment_index):
    """Checks if VTT cues fall within the [window_start, window_end] range."""
    try:
        req = urllib.request.Request(url, headers={'User-Agent': 'Mozilla/5.0'})
        with urllib.request.urlopen(req) as response:
            content = response.read().decode('utf-8')
            
        # Regex to find timestamp lines: 00:29:13.335 --> 00:29:14.169
        cue_pattern = re.compile(r'(\d{2}:?\d{2}:\d{2}\.\d{3})\s+-->\s+(\d{2}:?\d{2}:\d{2}\.\d{3})')
        lines = content.splitlines()
        
        cues_found = 0
        errors = []

        for line in lines:
            match = cue_pattern.search(line)
            if match:
                cues_found += 1
                cue_start = parse_vtt_time(match.group(1))
                cue_end = parse_vtt_time(match.group(2))

                # Validation logic: Cue must be within the segment's time window
                # We allow a tiny epsilon (0.001) for float rounding math
                if cue_start < (window_start - 0.001) or cue_end > (window_end + 0.001):
                    errors.append(
                        f"  [!] Out of Bounds: {match.group(0)}\n"
                        f"      Segment Window: {window_start:.3f}s to {window_end:.3f}s"
                    )

        status = "OK" if not errors else "FAIL"
        print(f"Segment {segment_index:03d}: {cues_found} cues found. [{status}]")
        for err in errors:
            print(err)
            
    except Exception as e:
        print(f"Segment {segment_index:03d}: Error downloading/parsing: {e}")

def main():
    if len(sys.argv) < 2:
        print("Usage: python vtt_check.py <m3u8_url>")
        sys.exit(1)

    m3u8_url = sys.argv[1]
    base_url = m3u8_url.rsplit('/', 1)[0] + '/'

    print(f"Analyzing Playlist: {m3u8_url}\n" + "="*50)
    
    try:
        with urllib.request.urlopen(m3u8_url) as response:
            playlist_content = response.read().decode('utf-8')
    except Exception as e:
        print(f"Critical Error: Could not fetch playlist - {e}")
        sys.exit(1)

    segments = parse_m3u8(playlist_content)
    
    # Global timeline tracking
    current_time_offset = 0.0
    
    for i, seg in enumerate(segments):
        # Resolve URL
        seg_url = seg['url'] if seg['url'].startswith('http') else base_url + seg['url']
        
        # Calculate the valid window for this segment
        seg_duration = seg['duration']
        window_end = current_time_offset + seg_duration
        
        check_vtt_segment(seg_url, current_time_offset, window_end, i)
        
        # Advance the "wall clock"
        current_time_offset += seg_duration

if __name__ == "__main__":
    main()
