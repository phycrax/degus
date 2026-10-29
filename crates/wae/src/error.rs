use std::path::PathBuf;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors produced while building WAE files.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to decode audio: {0}")]
    Decode(#[from] symphonia::core::errors::Error),
    #[error("no audio track found")]
    NoAudioTrack,
    #[error("audio sample rate is unknown")]
    UnknownSampleRate,
    #[error("failed to resample audio: {0}")]
    Resample(String),
    #[error("audio contains no samples")]
    EmptyAudio,
    #[error("clip {id}: {source}")]
    Clip {
        id: u16,
        #[source]
        source: Box<Error>,
    },
    #[error("no clips to write")]
    Empty,
    #[error("a WAE file holds at most 65535 clips")]
    TooManyClips,
    #[error("WAE data exceeds 4 GiB")]
    FileTooLarge,
}
