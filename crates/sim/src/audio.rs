//! Sound. The simulator decides what plays and when it ends; whoever runs
//! it plays the [`Sound`] events it reports, or only logs them.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use degus_set::config::Buzzer;

/// Clips play at this rate, in Hz.
pub const CLIP_RATE: u32 = degus_set::wae::SAMPLE_RATE;

/// Full volume of `0x00A0`, in 1/64 steps.
pub const FULL_VOLUME: u8 = 0x40;

/// The buzzer's clock, in Hz: its tone is this / (`div1` × `div2`).
const BUZZER_CLOCK: f64 = 825_753_600.0;

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
        wae: u8,
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
