use crate::codec::SampleFormat;

/// How multi-channel audio becomes the mono clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Downmix {
    /// Keep only the first (left) channel, as DWIN's tools do.
    #[default]
    Left,
    /// Average all channels.
    Average,
}

/// Settings for converting audio into WAE clips.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WaeOptions {
    pub format: SampleFormat,
    pub downmix: Downmix,
}
