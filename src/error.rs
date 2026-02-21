use thiserror::Error;

/// Main error type for the HLS server
#[derive(Error, Debug)]
pub enum HlsError {
    #[error("FFmpeg error: {0}")]
    Ffmpeg(#[from] FfmpegError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Stream not found: {0}")]
    StreamNotFound(String),

    #[error("Segment not found: stream={stream_id}, type={segment_type}, seq={sequence}")]
    SegmentNotFound {
        stream_id: String,
        segment_type: String,
        sequence: usize,
    },

    #[error("Indexing timeout for file: {0}")]
    IndexTimeout(String),

    #[error("No video stream found in source file")]
    NoVideoStream,

    #[error("No supported audio codec found")]
    NoSupportedAudio,

    #[error("No text subtitle stream found")]
    NoTextSubtitle,

    #[error("Transcoding error: {0}")]
    Transcode(String),

    #[error("Muxing error: {0}")]
    Muxing(String),

    #[error("Playlist generation error: {0}")]
    Playlist(String),

    #[error("Cache error: {0}")]
    Cache(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("HTTP error: {0}")]
    Http(String),

    #[error("Invalid codec: {0}")]
    InvalidCodec(String),

    #[error("Invalid timestamp: {0}")]
    InvalidTimestamp(String),

    #[error("Memory limit exceeded")]
    MemoryLimit,
}

/// FFmpeg-specific errors
#[derive(Error, Debug)]
pub enum FfmpegError {
    #[error("FFmpeg initialization failed: {0}")]
    InitFailed(String),

    #[error("Failed to open input file: {0}")]
    OpenInput(String),

    #[error("Failed to find stream info: {0}")]
    FindStreamInfo(String),

    #[error("Failed to find decoder: codec_id={0}")]
    DecoderNotFound(String),

    #[error("Failed to create decoder: {0}")]
    DecoderCreate(String),

    #[error("Failed to find encoder: codec_id={0}")]
    EncoderNotFound(String),

    #[error("Failed to create encoder: {0}")]
    EncoderCreate(String),

    #[error("Failed to configure encoder: {0}")]
    EncoderConfigure(String),

    #[error("Failed to create resampler: {0}")]
    ResamplerCreate(String),

    #[error("Failed to create muxer: {0}")]
    MuxerCreate(String),

    #[error("Failed to write header: {0}")]
    WriteHeader(String),

    #[error("Failed to write packet: {0}")]
    WritePacket(String),

    #[error("Failed to write trailer: {0}")]
    WriteTrailer(String),

    #[error("Failed to decode packet: {0}")]
    DecodePacket(String),

    #[error("Failed to encode frame: {0}")]
    EncodeFrame(String),

    #[error("Failed to read frame: {0}")]
    ReadFrame(String),

    #[error("Invalid timebase")]
    InvalidTimebase,

    #[error("Codec not found: {0}")]
    CodecNotFound(String),
    #[error("Stream configuration failed: {0}")]
    StreamConfig(String),

    #[error("Write error: {0}")]
    WriteError(String),
}

/// Result type alias for convenience
pub type Result<T> = std::result::Result<T, HlsError>;
