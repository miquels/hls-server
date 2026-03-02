# Jellyfin HLS proxy.

The Jellyfin server is an open source media server.

When a client wants to play a movie, it negotiates with the server to see
if it can play the media. If it can play it directly (DirectPlay) then there's
no issue. But if it cannot, say in the case of Chrome wanting to play a
mkv video with ac-3 sound, then Jellyfin can do transcoding or transmuxing.

The client indicates what codecs it supports, and what tracks it wants to
play (audio + video + optional subtitle) and Jellyfin generates a HLS playlist
that contains these tracks: audio and video interleaved in one track, and
optionally a subtitle track.

Jellyfin starts an external ffmpeg process to do all this, the ffmpeg process
writes the HLS playlists and segments to disk and jellyfin serves them
to the client. This is inefficient.

## Solution.

We will build a "HLS transmuxing proxy". This is a reverse HTTP proxy we can put
in front of Jellyfin. It will do two main things:

1. It will handle http requests and proxy them to the jellyfin backend as http
2. It will intercept media negotiation and playback requests, and instead
   will handle any transmuxing itself, using 'hls-vod-lib'.

## Tech stack

- hls-vod-lib from this workspace (in ../hls-vod-lib)
- axum 0.8.0 or newer
- axum-server 0.8.0 or newer
- clap 4.5.60 or newer
- tokio 1.49.0 or newer

## hls-vod-lib

We will use:

`hls_vod_lib::media::StreamInfo` for media information (tracks, video, audio subtitles info)
`hls_vod_lib::HlsParams` and `hls_vod_libHklsVideo` to build our HLS path handler and server

We will in this crate _not_ use ffmpeg, not directly as and executable, not via
the ffmpeg libraries. All media and HLS handling will be done by using `hls_vod_lib`.

Transcoding is also not a specific goal for now. We will use what `hls_vod_lib`
provides which is mostly transmuxing.

## How the interceptor will work

- The client sends a POST to /Items/{ItemId}/PlaybackInfo
- The proxy will forward this request to the jellyfin server, with the following changes:
  - it will fill the DirectPlayProfiles with
    - container: mp4,m4v,mkv,webm video codec
    - video codec h264,h265,vp9
    - audio codec aac,mp3,ac3,eac3,opus
  - it will leave TranscodingProfiles empty
- In the reply we will get a Path, indicating where the file lives on the filesystem

Now we have all the data we need:
- what the client supports
- the path to the video file

Then, with this information, we follow the same logic the jellyfin server does to
create a reply to the client. If the client can do directplay for the media, we
will proxy that like everything else. If the client _does_ need transcoding,
we will generate a TranscodingUrl for it. This will be in a separate namespace,
something like /proxymedia/path/to/file/mp4.as.m3u8, adding path parameters
for track selection, interleaving and maybe transcoding (only audio and only TO aac!).

We can use the code in ../hls-vod-server/src/http/dynamic.rs as an example
for the HLS URL handler.

