//! FFmpeg stream index entry access and byte-seek helpers.
//!
//! FFmpeg builds an in-memory index table for every stream when it parses the
//! container header (moov for MP4, Cues for MKV, etc.).  Each entry records the
//! byte offset, PTS, and size of one sample/packet.  We use this to:
//!
//!  - Build segment boundaries from keyframe positions without reading any
//!    media data (replaces the full-file packet scan in the scanner).
//!  - Seek directly to the byte offset of a specific sample when generating
//!    subtitle segments (replaces the open+seek+iterate-all-packets loop).
//!  - Seek directly to the byte offset of a video keyframe when generating
//!    video/audio segments (replaces timestamp-based seeking).
//!
//! The relevant C API (`av_index_get_num_entries`, `av_index_get_entry`,
//! `avformat_seek_file` with `AVSEEK_FLAG_BYTE`) is part of libavformat and
//! available in FFmpeg 5.1+.  The `ffmpeg-next` safe wrappers do not expose
//! these, so we call them through `ffmpeg::ffi` directly.

use ffmpeg_next as ffmpeg;

/// A single entry from a stream's internal index table.
#[derive(Debug, Clone)]
pub struct IndexEntry {
    /// Byte offset of this sample in the file.
    pub pos: u64,
    /// Presentation timestamp in the stream's native timebase.
    pub timestamp: i64,
    /// Size of the sample in bytes (may be 0 if unknown).
    pub size: i32,
    /// Flags: `AVINDEX_KEYFRAME = 0x0001`.
    pub flags: i32,
}

impl IndexEntry {
    /// Returns true if this entry is a keyframe.
    pub fn is_keyframe(&self) -> bool {
        self.flags & 0x0001 != 0
    }
}

/// Read all index entries for a stream from FFmpeg's internal index table.
///
/// Returns an empty `Vec` if the stream has no index (e.g. unseekable sources).
/// The entries are in presentation-order (sorted by `timestamp`).
///
/// # Safety
/// `stream_ptr` must be a valid, non-null `*mut AVStream` obtained from an
/// open `AVFormatContext` that has not been freed.
pub fn read_index_entries(stream: &ffmpeg::Stream) -> Vec<IndexEntry> {
    unsafe {
        let stream_ptr = stream.as_ptr() as *mut ffmpeg::ffi::AVStream;
        let n = ffmpeg::ffi::avformat_index_get_entries_count(stream_ptr);
        if n <= 0 {
            return Vec::new();
        }
        let mut entries = Vec::with_capacity(n as usize);
        for i in 0..n {
            let e = ffmpeg::ffi::avformat_index_get_entry(stream_ptr, i);
            if e.is_null() {
                continue;
            }
            entries.push(IndexEntry {
                pos: (*e).pos as u64,
                timestamp: (*e).timestamp,
                size: (*e).size() as i32,
                flags: (*e).flags() as i32,
            });
        }
        entries
    }
}

/// Seek to an exact byte offset in the file using `AVSEEK_FLAG_BYTE`.
///
/// This is the most precise seek available: it positions the AVIO read pointer
/// at `byte_offset` and resets the demuxer state so the next `av_read_frame`
/// call returns the packet starting at that offset.
///
/// Returns `Ok(())` on success, or an error string on failure.
///
/// # Safety
/// `ctx_ptr` must be a valid, non-null `*mut AVFormatContext`.
#[allow(dead_code)] // we'll need this later
pub fn seek_to_byte_offset(
    input: &mut ffmpeg::format::context::Input,
    stream_index: i32,
    byte_offset: u64,
) -> Result<(), String> {
    // AVSEEK_FLAG_BYTE = 2
    const AVSEEK_FLAG_BYTE: i32 = 2;
    let pos = byte_offset as i64;
    let ret = unsafe {
        ffmpeg::ffi::avformat_seek_file(
            input.as_mut_ptr(),
            stream_index,
            pos, // min_ts
            pos, // target_ts
            pos, // max_ts
            AVSEEK_FLAG_BYTE,
        )
    };
    if ret < 0 {
        Err(format!(
            "avformat_seek_file(byte={}) returned {}",
            byte_offset, ret
        ))
    } else {
        Ok(())
    }
}
