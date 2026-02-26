# supported codecs and the audio planner.

We're going to change the way we handle audio and webvtt tracks.

We're going to add a codecs argument to MediaInfo::open:

`fn open(path: &Path, codecs: &[impl AsRef<str>], stream_id: Option<String>) -> Result<Arc<MediaInfo>>`

if the codecs slice is empty, we don't do anything special. otherwise:

For each track in the video:

- for video: we don't check for now, always leave video as-is
- for audio: we'll check if it's present in the list of codecs. if it's not, don't add the audio track
- for subtitles, same as audio. for now only one codec is supported, wvtt

If, after we have done this, there are no audio tracks left, we'll take the first
group of audio tracks, and add them marked as 'transcode to aac' IF and only IF
'aac' is present in the list of codecs.

Check if you can re-use the audio planner for this, or remove the audio planner and build a replacement.


Once this is done, update the `hls_vod_server` code to accept a query parameter called `codecs` that
has a list of comma-separated codecs, and use them in the call to MediaInfo::open().

