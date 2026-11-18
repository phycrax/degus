//! Sound. The simulator decides what plays and when it ends; whoever runs
//! it plays the [`Sound`] events it reports, or only logs them.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use degus_config::Buzzer;
use degus_project::Project;
use degus_wae::{ClipSource, WaeOptions};

/// Clips play at this rate, in Hz.
pub const CLIP_RATE: u32 = degus_wae::SAMPLE_RATE;
/// The buzzer's clock, in Hz: its tone is this / (`div1` × `div2`).
const BUZZER_CLOCK: f64 = 825_753_600.0;
/// Full volume of `0x00A0`, in 1/64 steps.
pub const FULL_VOLUME: u8 = 0x40;

/// A clip's samples: mono, 16-bit, at [`CLIP_RATE`].
#[derive(Clone, PartialEq, Eq)]
pub struct Samples(pub Arc<[i16]>);

impl Samples {
    pub fn duration(&self) -> Duration {
        Duration::from_secs_f64(self.0.len() as f64 / f64::from(CLIP_RATE))
    }
}

/// The count, not every sample.
impl fmt::Debug for Samples {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Samples({})", self.0.len())
    }
}

/// Something the display plays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sound {
    /// Clip `id` of `<wae>.wae` at `volume` / 64, cutting off the clip
    /// playing.
    Clip {
        wae: u16,
        id: u16,
        volume: u8,
        samples: Samples,
    },
    /// The buzzer, for `length`.
    Beep { length: Duration, buzzer: Buzzer },
}

/// `clip 3 of 28.wae at 100%`, `beep 100ms at 2502 Hz`.
impl fmt::Display for Sound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Sound::Clip {
                wae, id, volume, ..
            } => {
                let percent = u32::from(*volume) * 100 / u32::from(FULL_VOLUME);
                write!(f, "clip {id} of {wae}.wae at {percent}%")
            }
            Sound::Beep { length, buzzer } => {
                let (hz, _) = buzzer_tone(buzzer);
                write!(f, "beep {}ms at {hz:.0} Hz", length.as_millis())
            }
        }
    }
}

/// The buzzer's frequency in Hz and its duty, 0-1.
pub fn buzzer_tone(buzzer: &Buzzer) -> (f64, f64) {
    let div = f64::from(buzzer.div1.max(1)) * f64::from(buzzer.div2.max(1));
    let duty = f64::from(buzzer.duty) / f64::from(buzzer.div2.max(1));
    (BUZZER_CLOCK / div, duty.clamp(0.0, 1.0))
}

/// Clips by WAE file number and clip ID, decoded on first use.
pub trait AudioSource: Send {
    /// Clip `id` of `<wae>.wae`, as the display plays it.
    fn clip(&self, wae: u16, id: u16) -> Option<Samples>;

    /// Problems met while decoding since the last call.
    fn take_problems(&self) -> Vec<String> {
        Vec::new()
    }
}

/// Decoded clips by WAE and clip ID; `None` for ones that failed.
type ClipCache = HashMap<(u16, u16), Option<Samples>>;

/// The clips of a project's `[[wae]]` tables, read from their source files
/// and converted like the build converts them, the 8-bit format's loss
/// included.
pub struct ProjectAudio {
    waes: BTreeMap<u16, (WaeOptions, BTreeMap<u16, PathBuf>)>,
    cache: RefCell<ClipCache>,
    problems: RefCell<Vec<String>>,
}

impl ProjectAudio {
    pub fn new(project: &Project) -> Self {
        let waes = project
            .config
            .wae
            .iter()
            .map(|spec| {
                let options = WaeOptions {
                    format: spec.format.into(),
                    downmix: spec.downmix.into(),
                };
                let clips = spec
                    .clips
                    .iter()
                    .map(|source| (source.id, project.resolve(&source.path)))
                    .collect();
                (spec.id, (options, clips))
            })
            .collect();
        Self {
            waes,
            cache: RefCell::default(),
            problems: RefCell::default(),
        }
    }
}

impl AudioSource for ProjectAudio {
    fn clip(&self, wae: u16, id: u16) -> Option<Samples> {
        if let Some(cached) = self.cache.borrow().get(&(wae, id)) {
            return cached.clone();
        }
        let (options, clips) = self.waes.get(&wae)?;
        let path = clips.get(&id)?;
        let samples = match degus_wae::clip_samples(&ClipSource::File(path.clone()), options) {
            Ok(samples) => Some(Samples(samples.into())),
            Err(error) => {
                self.problems
                    .borrow_mut()
                    .push(format!("{}: {error}", path.display()));
                None
            }
        };
        self.cache.borrow_mut().insert((wae, id), samples.clone());
        samples
    }

    fn take_problems(&self) -> Vec<String> {
        std::mem::take(&mut self.problems.borrow_mut())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_the_factory_buzzer_tone() {
        let (hz, duty) = buzzer_tone(&Buzzer::default());
        assert_eq!(hz.round(), 2502.0);
        assert_eq!(duty, 0.08);
    }

    #[test]
    fn describes_sounds() {
        let clip = Sound::Clip {
            wae: 28,
            id: 3,
            volume: 0x20,
            samples: Samples(vec![0; 16_000].into()),
        };
        assert_eq!(clip.to_string(), "clip 3 of 28.wae at 50%");
        let beep = Sound::Beep {
            length: Duration::from_millis(100),
            buzzer: Buzzer::default(),
        };
        assert_eq!(beep.to_string(), "beep 100ms at 2502 Hz");
        let Sound::Clip { samples, .. } = clip else {
            unreachable!()
        };
        assert_eq!(samples.duration(), Duration::from_millis(500));
        assert_eq!(format!("{samples:?}"), "Samples(16000)");
    }
}
