
# routing and paths

Currently we use routes as documented in README.md, with POST to /streams
to 'register' a video, which returns the path to the master m3u8. Everything
is based on a unique ID that's generated when the video is registered.
Paths look like `/streams/{id}/.....`. After playing, we need to deregister
the video using DELETE.

Let's consider an alternative approach.

## Playback without explicitly registering the video.

Consider a video located at filesystem path /media/movies/bigbucks.mp4.

When we send a request for a playlist, say /media/movies/bigbucks.m3u8:

- check if a video file exists with the same basename as the m3u8, so check for:
  * /media/movies/bigbucks.mp4
  * /media/movies/bigbucks.mkv
  * /media/movies/bigbucks.webm
- if not, return 404
- if it does, do the same as we now do when registering a video:
  - parse video file (if not in cache yet)
  - keep in memory cache
  - but return the master.m3u8 playlist contents.

Now, for video/audio playlists and segments, we use a strategy where
we always use the path to the movie, but then _add_ a subpath to it:

- video playlist:
  /media/movies/bigbucks.mp4/v/media.m3u8

- video segments:
  /media/movies/bigbucks.mp4/v/N.X.m4s, where:
  * N is the video track (usually 1)
  * X is the segment number (currently, 0, 1, 2 etc)

- audio playlist:
  /media/movies/bigbucks.mp4/a/media.m3u8

- audio segments:
  /media/movies/bigbucks.mp4/a/N.X.m4s, where:
  * N is the audio track (with multiple audio tracks, could be 1, 2, 3, 4 etc)
  * X is the segment number (currently, 0, 1, 2 etc)

- subtitle playlist:
  /media/movies/bigbucks.mp4/s/media.m3u8

- subtitle segments:
  /media/movies/bigbucks.mp4/s/N.X.vtt, where:
  * N is the subtitle track (with multiple subtitle tracks, could be 1, 2, 3, 4 etc)
  * X is the segment number (currently, 0, 1, 2 etc)

Track numbers don't have to start at one, if it's handy to use the track
number or track index from the .mp4 file, do so.

### relative paths in playlist

With this setup, we can, in the master playlist, simply refer to the
video/audio/subtitles playlists as for example:

- bigbucks.mp4/v/media.m3u8
- bigbucks.mp4/a/media.m3u8
- bigbucks.mp4/s/media.m3u8

## Caching and timeouts

If for a certain time no requests have been made for any segment of the video,
we should remove it from the in-memory cache. User might have paused the video
and gone to bed. 5 minutes seems to be a good timeout.

However, if we get a request for any of the above paths and we don't have the
data for the video in cache, we can regenerate it. This works because every
request contains the path to the video file!

This is pretty easy to detect, we split the path in it's parts (on '/') and check if:
- it has at least 3 parts
- the part two parts before last ends in .mp4 / .mkv / .webm
- the part before last is 'a', 'v' or 's'.

## Segment number encoding

**DO NOT IMPLEMENT THIS YET**

Currently we keep a mapping from segment number to the location in the video
in memory as part of the cache. We might be able to store this information
in the segment number instead. This needs to be investigated if it is
efficient and feasible.

Example for a video segment:

  /media/movies/bigbucks.mp4/a/1.0.1-100.m4s, where:
  1: video track number
  0: segment number
  1-100: sample numbers of samples in this segment.

Instead of sample number we could also store a timestamp in the video's timescale, maybe

  /media/movies/bigbucks.mp4/a/1.0.0-90000.m4s, where:
  1: video track number
  0: segment number
  0-90000: exact timestamp of start and end of this segment.

If it's easier or more efficient to keep this mapping in memory, don't bother with this.

