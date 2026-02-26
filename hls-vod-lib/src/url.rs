use std::fmt;
use std::str::FromStr;

use crate::types::StreamIndex;

/// To work with a HlsUrl:
///
/// // Parse url.
/// let hls_url = HlsUrl::parse(url).ok_or(ErrorNotFound)?;
///
/// // Find video file on filesystem.
/// let video = map_url_to_file(&hls_url.video)?;
///
/// // Open video file.
/// let media_info = StreamIndex::open(&video)?;
///
/// // Generate playlist or segment.
/// return hls_url.generate(&media_info);
///
#[derive(Debug)]
pub struct HlsUrl {
    /// Enum of subtype.
    pub url_type: UrlType,
    /// Optional session id. Is only None for the MainPlaylist.
    pub session_id: Option<String>,
    /// URL of the base video file.
    pub video_url: String,
}

/// Different types of encoded URLs.
#[derive(Debug)]
pub enum UrlType {
    MainPlaylist,
    Playlist(Playlist),
    VideoSegment(VideoSegment),
    AudioSegment(AudioSegment),
    VttSegment(VttSegment),
}

// helper.
fn basename(s: &str) -> &'_ str {
    s.split("/").last().unwrap()
}

// helper.
macro_rules! regex {
    ($re:literal $(,)?) => {{
        static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        RE.get_or_init(|| regex::Regex::new($re).unwrap())
    }};
}

// helper.
fn usize_from_str(s: &str) -> usize {
    usize::from_str(s).expect("a number")
}

impl fmt::Display for HlsUrl {
    /// Generate the encoded url, relative to the playlist it's in.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.url_type {
            UrlType::MainPlaylist => write!(f, "{}.as.m3u8", basename(&self.video_url)),
            UrlType::Playlist(s) => {
                // A playlist is included in from the main playlist, and at the same relative
                // position in the URL as the video file / the video.as.m3u8. So, we need
                // to prepend the videos' name, and the session id.
                write!(f, "{}/", basename(&self.video_url))?;
                if let Some(session_id) = &self.session_id {
                    write!(f, "{}/", session_id)?;
                }
                s.fmt(f)
            }
            UrlType::VideoSegment(s) => s.fmt(f),
            UrlType::AudioSegment(s) => s.fmt(f),
            UrlType::VttSegment(s) => s.fmt(f),
        }
    }
}

impl HlsUrl {
    /// Parse a HLS URL.
    pub fn parse(url: &str) -> Option<HlsUrl> {
        // Check for video.mp4.as.m3u8.
        if let Some(caps) = regex!(r"^(.+\.(?:mp4|mkv|webm))\.as\.m3u8$").captures(url) {
            return Some(HlsUrl {
                url_type: UrlType::MainPlaylist,
                session_id: None,
                video_url: caps[1].to_string(),
            });
        }

        // Then something with a session id.
        let caps = regex!(r"^(.+\.(?:mp4|mkv|webm))/([^/]+)/(.+)$").captures(url)?;
        let video_url = caps[1].to_string();
        let session_id = Some(caps[2].to_string());
        let rest = &caps[3];

        // Playlists.
        // t.<track_id>.m3u8
        // t.<track_id>+<audio_track_id>.m3u8
        // t.<track_id>+<audio_track_id>-<codec>.m3u8
        if let Some(caps) = regex!(r"^t.(\d+)(?:\+(\d+))?(?:-(.+))?.(m3u8)").captures(rest) {
            return Some(HlsUrl {
                url_type: UrlType::Playlist(Playlist {
                    track_id: usize_from_str(&caps[1]),
                    audio_track_id: caps.get(2).map(|m| usize_from_str(m.as_str())),
                    audio_transcode_to: caps.get(3).map(|m| m.as_str().to_string()),
                }),
                session_id,
                video_url,
            });
        }

        // Audio URL.
        //
        // a/<track_id>.init.mp4
        // a/<track_id>-<transcode_to>.init.mp4
        //
        // a/<track_id>.<segment_id>.m4s
        // a/<track_id>-<transcode_to>.<segment_id>.m4s
        if let Some(caps) =
            regex!(r"^a/(\d+)(?:-([a-z]+))?(?:\.(\d+))?\.(m4s|init.mp4)$").captures(rest)
        {
            if (&caps[4] == "init.mp4" && caps.get(3).is_some())
                || (&caps[4] == "m4s" && caps.get(3).is_none())
            {
                return None;
            }
            return Some(HlsUrl {
                url_type: UrlType::AudioSegment(AudioSegment {
                    track_id: usize_from_str(&caps[1]),
                    transcode_to: caps.get(2).map(|m| m.as_str().to_string()),
                    segment_id: caps.get(3).map(|m| usize_from_str(m.as_str())),
                }),
                session_id,
                video_url,
            });
        }

        // Video URL.
        //
        // v/<track_id>.init.mp4
        // v/<track_id>+<audio_track_id>.init.mp4
        // v/<track_id>+<audio_track_id>-<audio_transcode_to>.init.mp4
        //
        // v/<track_id>.<segment_id>.m4s
        // v/<track_id>+<audio_track_id>.<segment_id>.m4s
        // v/<track_id>+<audio_track_id>-<audio_transcode_to>.<segment_id>.m4s
        if let Some(caps) =
            regex!(r"^v/(\d+)(?:\+(\d+)(?:-([a-z]+))?)?(?:\.(\d+))?\.(m4s|init.mp4)").captures(rest)
        {
            if (&caps[5] == "init.mp4" && caps.get(4).is_some())
                || (&caps[5] == "m4s" && caps.get(4).is_none())
            {
                return None;
            }
            return Some(HlsUrl {
                url_type: UrlType::VideoSegment(VideoSegment {
                    track_id: usize_from_str(&caps[1]),
                    audio_track_id: caps.get(2).map(|m| usize_from_str(m.as_str())),
                    audio_transcode_to: caps
                        .get(2)
                        .and_then(|_| caps.get(3).map(|m| m.as_str().to_string())),
                    segment_id: caps.get(4).map(|m| usize_from_str(m.as_str())),
                }),
                session_id,
                video_url,
            });
        }

        // Subtitle URL.
        // s/<track_id>.<start_cue>.<end_cue>.vtt
        if let Some(caps) = regex!(r"^s/(\d+)\.(\d+)-(\d+)\.vtt$").captures(rest) {
            return Some(HlsUrl {
                url_type: UrlType::VttSegment(VttSegment {
                    track_id: usize_from_str(&caps[1]),
                    start_cue: usize_from_str(&caps[2]),
                    end_cue: usize_from_str(&caps[3]),
                }),
                session_id,
                video_url,
            });
        }

        None
    }

    /// Encode the HlsUrl to a string.
    pub fn encode_url(&self) -> String {
        self.to_string()
    }

    /// Generate the playlist or segment.
    // TODO: returns Bytes instead of Vec<u8>
    pub fn generate(
        &self,
        media_info: &StreamIndex,
        interleaved: bool,
        force_aac: bool,
    ) -> crate::error::Result<Vec<u8>> {
        // See if it's in the cache.
        let segment_key = self.to_string();
        if let Some(c) = crate::segment::cache::get() {
            if let Some(b) = c.get(&media_info.stream_id, &segment_key) {
                return Ok(b.to_vec());
            }
        }
        let mut cache_it = false;

        let data = match &self.url_type {
            UrlType::MainPlaylist => {
                let playlist = crate::playlist::generate_master_playlist(
                    media_info,
                    &self.video_url,
                    Some(&media_info.stream_id),
                    interleaved,
                    force_aac,
                );
                Ok(playlist.into_bytes())
            }
            UrlType::Playlist(p) => {
                let playlist = if let Some(audio_idx) = p.audio_track_id {
                    // Audio / Video interleaved playlist
                    let force_aac_track = p.audio_transcode_to.as_deref() == Some("aac");
                    crate::playlist::variant::generate_interleaved_playlist(
                        media_info,
                        p.track_id,
                        audio_idx,
                        force_aac_track,
                    )
                } else if media_info
                    .audio_streams
                    .iter()
                    .any(|a| a.stream_index == p.track_id)
                {
                    // Audio only playlist
                    let force_aac_track = p.audio_transcode_to.as_deref() == Some("aac");
                    crate::playlist::variant::generate_audio_playlist(
                        media_info,
                        p.track_id,
                        force_aac_track,
                    )
                } else if media_info
                    // Subtitle only playlist
                    .subtitle_streams
                    .iter()
                    .any(|s| s.stream_index == p.track_id)
                {
                    crate::playlist::variant::generate_subtitle_playlist(media_info, p.track_id)
                } else {
                    // Main video playlist.
                    crate::playlist::variant::generate_video_playlist(media_info)
                };
                Ok(playlist.into_bytes())
            }
            UrlType::VideoSegment(v) => {
                if let Some(audio_idx) = v.audio_track_id {
                    let force_aac_track = v.audio_transcode_to.as_deref() == Some("aac");
                    if let Some(seq) = v.segment_id {
                        // TODO: make segments a HashMap<u64, Segment> ?
                        let segment = media_info.get_segment("video", seq)?;
                        let buf = crate::segment::generator::generate_interleaved_segment(
                            media_info,
                            v.track_id,
                            audio_idx,
                            segment,
                            &media_info.source_path,
                            force_aac_track,
                        )
                        .map(|b| b.to_vec())?;
                        cache_it = true;
                        Ok(buf)
                    } else {
                        crate::segment::generator::generate_interleaved_init_segment(
                            media_info,
                            v.track_id,
                            audio_idx,
                            force_aac_track,
                        )
                        .map(|b| b.to_vec())
                    }
                } else if let Some(seq) = v.segment_id {
                    let buf = crate::segment::generator::generate_video_segment(
                        media_info,
                        v.track_id,
                        seq,
                        &media_info.source_path,
                    )
                    .map(|b| b.to_vec())?;
                    cache_it = true;
                    Ok(buf)
                } else {
                    crate::segment::generator::generate_video_init_segment(media_info)
                        .map(|b| b.to_vec())
                }
            }
            UrlType::AudioSegment(a) => {
                let force_aac_track = a.transcode_to.as_deref() == Some("aac");
                if let Some(seq) = a.segment_id {
                    let buf = crate::segment::generator::generate_audio_segment(
                        media_info,
                        a.track_id,
                        seq,
                        &media_info.source_path,
                        force_aac_track,
                    )
                    .map(|b| b.to_vec())?;
                    cache_it = true;
                    Ok(buf)
                } else {
                    crate::segment::generator::generate_audio_init_segment(
                        media_info,
                        a.track_id,
                        force_aac_track,
                    )
                    .map(|b| b.to_vec())
                }
            }
            UrlType::VttSegment(s) => {
                let buf = crate::segment::generator::generate_subtitle_segment(
                    media_info,
                    s.track_id,
                    s.start_cue,
                    s.end_cue,
                    &media_info.source_path,
                )
                .map(|b| b.to_vec())?;
                cache_it = true;
                Ok(buf)
            }
        }?;

        if cache_it {
            if let Some(c) = crate::segment::cache::get() {
                c.insert(
                    &media_info.stream_id,
                    &segment_key,
                    bytes::Bytes::from(data.clone()),
                );
            }
        }

        Ok(data)
    }
}

/// A video segment.
#[derive(Debug)]
pub struct VideoSegment {
    /// Track id.
    pub track_id: usize,
    /// Extra track id to be interleaved with. Optional. Always audio.
    pub audio_track_id: Option<usize>,
    /// Transcode
    pub audio_transcode_to: Option<String>,
    /// Segment id. If None, this is the init segment.
    pub segment_id: Option<usize>,
}

impl fmt::Display for VideoSegment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "v/{}", self.track_id)?;
        if let Some(audio_track_id) = self.audio_track_id {
            write!(f, "+{}", audio_track_id)?;
            if let Some(audio_transcode_to) = &self.audio_transcode_to {
                write!(f, "-{}", audio_transcode_to)?;
            }
        }
        if let Some(segment_id) = self.segment_id {
            write!(f, ".{}.m4s", segment_id)?;
        } else {
            write!(f, ".init.mp4")?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct AudioSegment {
    /// Track id.
    pub track_id: usize,
    /// Transcode to other codec.
    pub transcode_to: Option<String>,
    /// Segment id. If None, this is the init segment.
    pub segment_id: Option<usize>,
}

impl fmt::Display for AudioSegment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "a/{}", self.track_id)?;
        if let Some(transcode_to) = &self.transcode_to {
            write!(f, "-{}", transcode_to)?;
        }
        if let Some(segment_id) = self.segment_id {
            write!(f, ".{}.m4s", segment_id)?;
        } else {
            write!(f, ".init.mp4")?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct VttSegment {
    /// Track id.
    pub track_id: usize,
    ///
    pub start_cue: usize,
    ///
    pub end_cue: usize,
}

impl fmt::Display for VttSegment {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "s/{}.{}-{}.vtt",
            self.track_id, self.start_cue, self.end_cue
        )
    }
}

#[derive(Debug)]
pub struct Playlist {
    /// Track id.
    pub track_id: usize,
    /// AUdio track to be interleaved with main track.
    pub audio_track_id: Option<usize>,
    /// Transcode audio.
    pub audio_transcode_to: Option<String>,
}

impl fmt::Display for Playlist {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "t.{}", self.track_id)?;
        if let Some(audio_track_id) = self.audio_track_id {
            write!(f, "+{}", audio_track_id)?;
            if let Some(audio_transcode_to) = &self.audio_transcode_to {
                write!(f, "-{}", audio_transcode_to)?;
            }
        }
        write!(f, ".m3u8")
    }
}
