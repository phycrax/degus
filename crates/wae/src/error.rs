pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Errors produced while building, writing or reading WAE files.
#[derive(Debug, thiserror::Error)]
pub enum Error {
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
    #[error("not a WAE file: it does not start with DGUS_2")]
    NotWae,
    #[error("WAE version {0} is not supported, only 3")]
    Version(u8),
    #[error("WAE sample format {0} is not known")]
    Format(u8),
    #[error("damaged WAE file: {0}")]
    Damaged(&'static str),
}
