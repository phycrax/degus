use std::path::PathBuf;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors produced while building ICL files.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to decode image: {0}")]
    Decode(#[from] image::ImageError),
    #[error("failed to encode JPEG: {0}")]
    Encode(#[from] jpeg_encoder::EncodingError),
    #[error("unsupported JPEG: {0}")]
    Jpeg(#[from] JpegError),
    #[error("image is {width}x{height}; ICL icons are limited to 65535x65535")]
    ImageTooLarge { width: u32, height: u32 },
    #[error("JPEG is {size} bytes, over the {limit}-byte per-icon limit")]
    IconTooLarge { size: usize, limit: usize },
    #[error("icon {id}: {source}")]
    Icon {
        id: u16,
        #[source]
        source: Box<Error>,
    },
    #[error("no icons to write")]
    Empty,
    #[error("ICL data exceeds 4 GiB")]
    FileTooLarge,
}

/// Reasons a JPEG cannot be stored in an ICL record as-is.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum JpegError {
    #[error("not a JPEG (missing SOI marker)")]
    NotJpeg,
    #[error("truncated JPEG data")]
    Truncated,
    #[error("expected a marker at offset {0}")]
    BadMarker(usize),
    #[error("SOF{0} frames are not supported; only baseline (SOF0)")]
    UnsupportedFrame(u8),
    #[error("missing frame header (SOF0)")]
    MissingFrame,
    #[error("missing scan header (SOS)")]
    MissingScan,
    #[error("{0}-bit samples are not supported; only 8-bit")]
    Precision(u8),
    #[error("{0} colour components are not supported; only 3 (YCbCr)")]
    Components(u8),
    #[error("sampling factors are not supported; only 4:4:4 and 4:2:0")]
    Sampling,
    #[error("Adobe APP14 colour transforms are not supported")]
    Adobe,
    #[error("multi-scan JPEGs are not supported; only a single interleaved scan")]
    MultiScan,
    #[error("the scan uses component {0}, which the frame header does not define")]
    UnknownComponent(u8),
    #[error("image has a zero dimension")]
    ZeroSize,
    #[error("JPEG header (up to the scan data) exceeds 65535 bytes")]
    HeaderTooLarge,
}
