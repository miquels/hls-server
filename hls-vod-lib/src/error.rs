use thiserror::Error;

/// Main error type for the HLS server
#[derive(Error, Debug)]
pub enum HlsError {
    /// An error originating from the underlying FFmpeg library
    #[error("FFmpeg error: {0}")]
    Ffmpeg(#[from] FfmpegError),

    /// A standard I/O error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// A requested stream could not be found in the media file
    #[error("Stream not found: {0}")]
    StreamNotFound(String),

    /// A specific segment sequence could not be found for the given stream type
    #[error("Segment not found: stream={stream_id}, type={segment_type}, seq={sequence}")]
    SegmentNotFound {
        stream_id: String,
        segment_type: String,
        sequence: usize,
    },

    /// The indexing process for the media file timed out
    #[error("Indexing timeout for file: {0}")]
    IndexTimeout(String),

    /// The media file does not contain a supported video stream
    #[error("No video stream found in source file")]
    NoVideoStream,

    /// The requisite stream index could not be loaded or generated
    #[error("No demuxer index: {0}")]
    NoIndex(String),

    /// The media file does not contain a supported audio codec
    #[error("No supported audio codec found")]
    NoSupportedAudio,

    /// A requested text subtitle stream could not be found
    #[error("No text subtitle stream found")]
    NoTextSubtitle,

    /// An error occurred during media transcoding
    #[error("Transcoding error: {0}")]
    Transcode(String),

    /// An error occurred during segment muxing
    #[error("Muxing error: {0}")]
    Muxing(String),

    /// An error occurred while generating a playlist
    #[error("Playlist generation error: {0}")]
    Playlist(String),

    /// An error occurred with the internal index cache
    #[error("Cache error: {0}")]
    Cache(String),

    /// Server configuration error
    #[error("Configuration error: {0}")]
    Config(String),

    /// An HTTP protocol-level error
    #[error("HTTP error: {0}")]
    Http(String),

    /// An unrecognized or unsupported codec was encountered
    #[error("Invalid codec: {0}")]
    InvalidCodec(String),

    /// A bad or unexpected timestamp was processed
    #[error("Invalid timestamp: {0}")]
    InvalidTimestamp(String),

    /// A process or task exceeded the allowed memory limit
    #[error("Memory limit exceeded")]
    MemoryLimit,
}

/// FFmpeg-specific errors
#[derive(Error, Debug)]
pub enum FfmpegError {
    /// Failure during global FFmpeg initialization
    #[error("FFmpeg initialization failed: {0}")]
    InitFailed(String),

    /// Failure opening an input media file
    #[error("Failed to open input file: {0}")]
    OpenInput(String),

    /// Failure locating stream information within a file
    #[error("Failed to find stream info: {0}")]
    FindStreamInfo(String),

    /// The requested decoder for a specific codec ID was not found
    #[error("Failed to find decoder: codec_id={0}")]
    DecoderNotFound(String),

    /// Failure instantiating a decoder
    #[error("Failed to create decoder: {0}")]
    DecoderCreate(String),

    /// The requested encoder for a specific codec ID was not found
    #[error("Failed to find encoder: codec_id={0}")]
    EncoderNotFound(String),

    /// Failure instantiating an encoder
    #[error("Failed to create encoder: {0}")]
    EncoderCreate(String),

    /// Failure applying configuration parameters to an encoder
    #[error("Failed to configure encoder: {0}")]
    EncoderConfigure(String),

    /// Failure creating an audio resampler
    #[error("Failed to create resampler: {0}")]
    ResamplerCreate(String),

    /// Failure creating an output format muxer
    #[error("Failed to create muxer: {0}")]
    MuxerCreate(String),

    /// Failure writing the container header
    #[error("Failed to write header: {0}")]
    WriteHeader(String),

    /// Failure writing a media packet to the container
    #[error("Failed to write packet: {0}")]
    WritePacket(String),

    /// Failure writing the container trailer
    #[error("Failed to write trailer: {0}")]
    WriteTrailer(String),

    /// Failure decoding a single packet into a frame
    #[error("Failed to decode packet: {0}")]
    DecodePacket(String),

    /// Failure encoding a single frame into a packet
    #[error("Failed to encode frame: {0}")]
    EncodeFrame(String),

    /// Failure reading a single frame from the input context
    #[error("Failed to read frame: {0}")]
    ReadFrame(String),

    /// An invalid or unexpected timebase was encountered
    #[error("Invalid timebase")]
    InvalidTimebase,

    /// A required codec was not found
    #[error("Codec not found: {0}")]
    CodecNotFound(String),

    /// Failure configuring stream contexts or parameters
    #[error("Stream configuration failed: {0}")]
    StreamConfig(String),

    /// A general writing error occurred
    #[error("Write error: {0}")]
    WriteError(String),
}

/// Result type alias for convenience
pub type Result<T> = std::result::Result<T, HlsError>;
