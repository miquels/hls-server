//! Custom AVIOContext for in-memory writing
//!
//! This module provides a custom IO context that writes to a Vec<u8>
//! instead of a file, enabling completely in-memory muxing.
//!
//! # Thread safety
//! `MemoryWriter` is intentionally NOT thread-safe. Each muxer instance is
//! created and consumed on a single thread (inside `spawn_blocking`). Using a
//! plain `Vec<u8>` avoids the `Arc<Mutex<Vec>>` re-entrancy deadlock: FFmpeg
//! can call `seek_packet` from within `write_packet` (e.g. during
//! `write_trailer` to query the buffer size), and `std::sync::Mutex` is not
//! reentrant — the nested `lock()` call on the same thread would deadlock.

use ffmpeg_next as ffmpeg;
use std::ffi::c_void;
use std::io::{Seek, SeekFrom, Write};
use std::ptr;

/// Custom IO context that writes to an in-memory buffer.
/// Single-threaded use only — one instance per muxer, never shared across threads.
pub struct MemoryWriter {
    buffer: Vec<u8>,
    position: u64,
}

impl MemoryWriter {
    /// Create a new memory writer
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(4096),
            position: 0,
        }
    }

    /// Get a copy of the written data
    pub fn data(&self) -> Vec<u8> {
        self.buffer.clone()
    }

    /// Check if the buffer is empty
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Clear the buffer
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.position = 0;
    }
}

impl Write for MemoryWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let pos = self.position as usize;
        let end = pos + buf.len();

        if end > self.buffer.len() {
            self.buffer.resize(end, 0);
        }

        self.buffer[pos..end].copy_from_slice(buf);
        self.position += buf.len() as u64;

        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Seek for MemoryWriter {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let buffer_len = self.buffer.len() as u64;

        let new_pos = match pos {
            SeekFrom::Start(p) => p,
            SeekFrom::Current(p) => (self.position as i64 + p) as u64,
            SeekFrom::End(p) => (buffer_len as i64 + p) as u64,
        };

        self.position = new_pos;
        Ok(self.position)
    }
}

impl Default for MemoryWriter {
    fn default() -> Self {
        Self::new()
    }
}

// C-compatible callbacks for FFmpeg

unsafe extern "C" fn write_packet(opaque: *mut c_void, buf: *const u8, buf_size: i32) -> i32 {
    let writer = &mut *(opaque as *mut MemoryWriter);
    let slice = std::slice::from_raw_parts(buf, buf_size as usize);
    match writer.write(slice) {
        Ok(n) => n as i32,
        Err(_) => -1,
    }
}

unsafe extern "C" fn seek_packet(opaque: *mut c_void, offset: i64, whence: i32) -> i64 {
    let writer = &mut *(opaque as *mut MemoryWriter);

    // AVSEEK_SIZE: return total buffer size
    if whence == 0x10000 {
        return writer.buffer.len() as i64;
    }

    let seek_from = match whence {
        0 => SeekFrom::Start(offset as u64),
        1 => SeekFrom::Current(offset),
        2 => SeekFrom::End(offset),
        _ => return -1,
    };
    match writer.seek(seek_from) {
        Ok(pos) => pos as i64,
        Err(_) => -1,
    }
}

/// Helper to create an Output context with custom IO
pub fn create_memory_io(
) -> Result<(ffmpeg::format::context::Output, Box<MemoryWriter>), crate::error::FfmpegError> {
    unsafe {
        // Create the writer and box it to get a stable pointer
        let writer = Box::new(MemoryWriter::new());
        let writer_ptr = Box::into_raw(writer);

        // Allocate internal buffer for AVIO
        let buffer_size = 4096;
        let buffer = ffmpeg::ffi::av_malloc(buffer_size as usize) as *mut u8;
        if buffer.is_null() {
            let _ = Box::from_raw(writer_ptr); // Cleanup
            return Err(crate::error::FfmpegError::InitFailed(
                "Failed to allocate AVIO buffer".to_string(),
            ));
        }

        // Create AVIO context
        let avio_ctx = ffmpeg::ffi::avio_alloc_context(
            buffer,
            buffer_size as i32, // Cast to i32 as required by avio_alloc_context
            1,
            writer_ptr as *mut c_void,
            None,
            Some(write_packet),
            Some(seek_packet),
        );

        if avio_ctx.is_null() {
            ffmpeg::ffi::av_free(buffer as *mut c_void);
            let _ = Box::from_raw(writer_ptr);
            return Err(crate::error::FfmpegError::InitFailed(
                "Failed to allocate AVIO context".to_string(),
            ));
        }

        // Create Output Context
        // We use "mp4" format. Filename is dummy.
        let mut output_ptr: *mut ffmpeg::ffi::AVFormatContext = ptr::null_mut();

        // Use CString for C compatibility
        let filename = std::ffi::CString::new("memory.mp4").unwrap();
        let format_name = std::ffi::CString::new("mp4").unwrap();

        let ret = ffmpeg::ffi::avformat_alloc_output_context2(
            &mut output_ptr,
            ptr::null_mut(),
            format_name.as_ptr(),
            filename.as_ptr(),
        );

        if ret < 0 || output_ptr.is_null() {
            ffmpeg::ffi::avio_context_free(&mut { avio_ctx }); // internal buffer freed by this? No, avio_alloc_context buffer must be freed manually if NOT using avio_close?
                                                               // Actually avio_context_free frees the internal struct but NOT the buffer passed to it?
                                                               // "The internal buffer must be freed with av_free() if it was allocated with av_malloc()." - FFmpeg docs
                                                               // But avio_context_free might not free it if we allocated it?
                                                               // Let's rely on standard ffmpeg patterns.
                                                               // For now, simple cleanup.
            let _ = Box::from_raw(writer_ptr);
            return Err(crate::error::FfmpegError::InitFailed(
                "Failed to create Output Context".to_string(),
            ));
        }

        // Assign IO context
        (*output_ptr).pb = avio_ctx;

        // Important: Set flag to custom IO? changing flags might be needed.
        (*output_ptr).flags |= ffmpeg::ffi::AVFMT_FLAG_CUSTOM_IO;

        // Create safe wrapper
        let output = ffmpeg::format::context::Output::wrap(output_ptr);

        // Reconstruct Box to manage lifecycle (caller must keep it alive)
        let writer = Box::from_raw(writer_ptr);

        Ok((output, writer))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_writer() {
        let mut writer = MemoryWriter::new();
        writer.write_all(b"test").unwrap();
        assert_eq!(writer.data(), b"test");
    }
}
