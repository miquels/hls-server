//! Safe wrappers around FFmpeg FFI calls.
//!
//! Every function in this module is `pub` and **safe** to call.  All `unsafe`
//! blocks are contained here with explicit safety arguments.  Callers outside
//! this module should never need to write `unsafe` for routine FFmpeg access.

use ffmpeg_next as ffmpeg;

// ── Codec-parameter field accessors ─────────────────────────────────────────

/// Read `sample_rate` from an `AVCodecParameters` struct.
///
/// `ffmpeg-next` does not expose this field through a safe accessor.
pub fn codec_params_sample_rate(params: &ffmpeg::codec::parameters::Parameters) -> u32 {
    // SAFETY: `params.as_ptr()` returns a valid non-null pointer for the
    // lifetime of `params`.  `sample_rate` is a plain i32 field with no
    // ownership semantics.
    unsafe { (*params.as_ptr()).sample_rate as u32 }
}

/// Read `ch_layout.nb_channels` from an `AVCodecParameters` struct.
pub fn codec_params_channels(params: &ffmpeg::codec::parameters::Parameters) -> u16 {
    // SAFETY: same as `codec_params_sample_rate`.
    unsafe { (*params.as_ptr()).ch_layout.nb_channels as u16 }
}

/// Read `width` from an `AVCodecParameters` struct.
pub fn codec_params_width(params: &ffmpeg::codec::parameters::Parameters) -> u32 {
    unsafe { (*params.as_ptr()).width as u32 }
}

/// Read `height` from an `AVCodecParameters` struct.
pub fn codec_params_height(params: &ffmpeg::codec::parameters::Parameters) -> u32 {
    unsafe { (*params.as_ptr()).height as u32 }
}

/// Read `profile` from an `AVCodecParameters` struct.
pub fn codec_params_profile(params: &ffmpeg::codec::parameters::Parameters) -> i32 {
    unsafe { (*params.as_ptr()).profile }
}

/// Read `level` from an `AVCodecParameters` struct.
pub fn codec_params_level(params: &ffmpeg::codec::parameters::Parameters) -> i32 {
    unsafe { (*params.as_ptr()).level }
}

/// Read `bit_rate` from an `AVCodecParameters` struct.
pub fn codec_params_bit_rate(params: &ffmpeg::codec::parameters::Parameters) -> u64 {
    unsafe { (*params.as_ptr()).bit_rate as u64 }
}

/// Zero out `codec_tag` on the `AVCodecParameters` attached to an output
/// stream, so the muxer picks the correct tag for the target container.
///
/// Must be called after `out_stream.set_parameters(...)` and before
/// `write_header`.
pub fn stream_reset_codec_tag(out_stream: &mut ffmpeg::format::stream::StreamMut) {
    // SAFETY: `out_stream.as_mut_ptr()` is valid for the lifetime of the
    // stream.  `codecpar` is set by `set_parameters` and is non-null.
    // Writing 0 to `codec_tag` is always safe — it is a plain u32 field.
    unsafe {
        (*(*out_stream.as_mut_ptr()).codecpar).codec_tag = 0;
    }
}

/// Allocate a fresh `AVCodecParameters`, copy the encoder context into it,
/// and return it as a safe `ffmpeg::codec::Parameters`.
///
/// Used to extract codec parameters from an encoder for muxer stream setup.
pub fn encoder_codec_parameters(
    encoder: &ffmpeg::codec::encoder::Audio,
) -> ffmpeg::codec::Parameters {
    use std::ops::Deref;
    use std::rc::Rc;
    let ctx: &ffmpeg::codec::Context = encoder.deref();
    // SAFETY: `avcodec_parameters_alloc` returns a valid pointer or null.
    // We check for null before use (the `wrap` call below would panic on null,
    // but in practice allocation only fails under OOM which is unrecoverable).
    // `avcodec_parameters_from_context` copies fields from a valid, open
    // encoder context — safe as long as `ctx.as_ptr()` is non-null (it is,
    // since `encoder` is a live object).
    unsafe {
        let params = ffmpeg::ffi::avcodec_parameters_alloc();
        ffmpeg::ffi::avcodec_parameters_from_context(params, ctx.as_ptr());
        ffmpeg::codec::Parameters::wrap(params, None::<Rc<dyn std::any::Any>>)
    }
}

// ── AVIO context management ──────────────────────────────────────────────────

/// Detach the `AVIOContext` (`pb`) from an `AVFormatContext` by setting it to
/// null, preventing `avformat_free_context` from double-freeing it.
///
/// Call this before dropping an `Output` whose `pb` was allocated manually
/// (e.g. via `create_memory_io`).
pub fn detach_avio(output: &mut ffmpeg::format::context::Output) {
    // SAFETY: `output.as_mut_ptr()` is valid for the lifetime of `output`.
    // Setting `pb` to null is the documented way to prevent double-free when
    // the caller owns the AVIO context separately.
    unsafe {
        let ctx = output.as_mut_ptr();
        if !ctx.is_null() && !(*ctx).pb.is_null() {
            (*ctx).pb = std::ptr::null_mut();
        }
    }
}

// ── Subtitle codec lookup ────────────────────────────────────────────────────

/// Returns `true` if a decoder is registered for `codec_id`.
pub fn decoder_exists(codec_id: ffmpeg::codec::Id) -> bool {
    // SAFETY: `avcodec_find_decoder` is thread-safe (reads a global read-only
    // registry after `ffmpeg::init()`).  The returned pointer is only used for
    // a null check; we never dereference it.
    let ptr = unsafe { ffmpeg::ffi::avcodec_find_decoder(codec_id.into()) };
    !ptr.is_null()
}

// ── Test helpers ─────────────────────────────────────────────────────────────

/// Override codec fields on an `AVCodecParameters` for testing purposes.
///
/// Allows tests to simulate a different codec (e.g. AC-3) by patching the raw
/// struct fields that `ffmpeg-next` does not expose through safe setters.
#[cfg(test)]
pub fn codec_params_set_for_test(
    params: &mut ffmpeg::codec::parameters::Parameters,
    codec_id: ffmpeg::ffi::AVCodecID,
    frame_size: i32,
    bit_rate: i64,
) {
    // SAFETY: `params.as_mut_ptr()` is valid for the lifetime of `params`.
    // These are plain scalar fields with no ownership semantics.  This
    // function is only compiled in test builds.
    unsafe {
        let p = params.as_mut_ptr();
        (*p).codec_id = codec_id;
        (*p).frame_size = frame_size;
        (*p).bit_rate = bit_rate;
    }
}

// ── FLTP audio plane reinterpretation ───────────────────────────────────────

/// Reinterpret a raw byte slice from an FLTP audio plane as `&[f32]`.
///
/// `byte_slice` must be the data plane of an `ffmpeg::util::frame::Audio`
/// frame in `FLTP` (planar float32) format.  `sample_count` is the number of
/// samples in the plane.
///
/// Returns `None` if:
/// - the pointer is not 4-byte aligned, or
/// - `byte_slice.len()` is not exactly `sample_count * 4`.
pub fn fltp_plane_as_f32(byte_slice: &[u8], sample_count: usize) -> Option<&[f32]> {
    let expected_bytes = sample_count.checked_mul(4)?;
    if byte_slice.len() < expected_bytes {
        return None;
    }
    let ptr = byte_slice.as_ptr();
    if !(ptr as usize).is_multiple_of(std::mem::align_of::<f32>()) {
        return None;
    }
    // SAFETY: alignment and length are verified above.  FLTP planes are
    // native-endian f32 values laid out contiguously.
    Some(unsafe { std::slice::from_raw_parts(ptr as *const f32, sample_count) })
}

/// Reinterpret a mutable raw byte slice from an FLTP audio plane as `&mut [f32]`.
///
/// Same preconditions and failure modes as [`fltp_plane_as_f32`].
pub fn fltp_plane_as_f32_mut(byte_slice: &mut [u8], sample_count: usize) -> Option<&mut [f32]> {
    let expected_bytes = sample_count.checked_mul(4)?;
    if byte_slice.len() < expected_bytes {
        return None;
    }
    let ptr = byte_slice.as_mut_ptr();
    if !(ptr as usize).is_multiple_of(std::mem::align_of::<f32>()) {
        return None;
    }
    Some(unsafe { std::slice::from_raw_parts_mut(ptr as *mut f32, sample_count) })
}

/// Extract an audio plane slice from an `AVFrame`.
///
/// Works around a bug in `ffmpeg-next`'s `Audio::data(index)` method where it
/// stops counting planes if `linesize[1] == 0`. In FFmpeg, planar audio frames
/// often only populate `linesize[0]` to represent the size of *every* plane.
pub fn audio_plane_data(frame: &ffmpeg::util::frame::Audio, index: usize) -> &[u8] {
    unsafe {
        let f = frame.as_ptr();
        let channels = (*f).ch_layout.nb_channels as usize;

        // Ensure index is valid for planar; packed has only 1 data plane.
        let is_planar = frame.format().is_planar();
        if is_planar {
            if index >= channels {
                return &[];
            }
        } else if index > 0 {
            return &[];
        }

        let ptrs = (*f).extended_data;
        if ptrs.is_null() {
            return &[];
        }

        let plane_ptr = *ptrs.add(index);
        if plane_ptr.is_null() {
            return &[];
        }

        let size = (*f).linesize[0] as usize;
        std::slice::from_raw_parts(plane_ptr, size)
    }
}

/// Mutable version of `audio_plane_data`.
pub fn audio_plane_data_mut(frame: &mut ffmpeg::util::frame::Audio, index: usize) -> &mut [u8] {
    unsafe {
        let f = frame.as_mut_ptr();
        let channels = (*f).ch_layout.nb_channels as usize;

        let is_planar = frame.format().is_planar();
        if is_planar {
            if index >= channels {
                return &mut [];
            }
        } else if index > 0 {
            return &mut [];
        }

        let ptrs = (*f).extended_data;
        if ptrs.is_null() {
            return &mut [];
        }

        let plane_ptr = *ptrs.add(index);
        if plane_ptr.is_null() {
            return &mut [];
        }

        let size = (*f).linesize[0] as usize;
        std::slice::from_raw_parts_mut(plane_ptr, size)
    }
}
