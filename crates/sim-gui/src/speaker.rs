//! The display's speaker and buzzer, on the default output device.
//!
//! The device lives on a thread of its own, so playing never waits for it.
//! Without an output device, or without the `audio` feature, the display
//! stays silent and its sounds are still logged.

use degus_sim::Sound;

/// Plays [`Sound`]s, at a master volume. A new clip cuts off the one
/// playing, as on the display; the buzzer plays beside it.
///
/// Clones play on the same device, so that one can go to
/// [`SimThread::set_speaker`](crate::SimThread::set_speaker) and another
/// can set the volume. The device closes with the last of them.
#[derive(Clone)]
pub struct Speaker {
    #[cfg(feature = "audio")]
    commands: std::sync::mpsc::Sender<Command>,
    #[cfg(feature = "audio")]
    problems: std::sync::Arc<parking_lot::Mutex<std::sync::mpsc::Receiver<String>>>,
}

#[cfg(feature = "audio")]
enum Command {
    Play(Sound),
    /// 0-1; 0 mutes.
    Volume(f32),
}

impl Speaker {
    /// Opens the default output device at `volume`, 0-1. Without the
    /// `audio` feature, it plays nothing.
    pub fn new(volume: f32) -> Self {
        cfg_select! {
            feature = "audio" => {
                let (commands, receiver) = std::sync::mpsc::channel();
                let (report, problems) = std::sync::mpsc::channel();
                let thread_report = report.clone();
                let spawned = std::thread::Builder::new()
                    .name("degus sim speaker".into())
                    .spawn(move || device::run(&receiver, &thread_report, volume));
                if let Err(error) = spawned {
                    let _ = report.send(format!("no audio thread, sounds are only logged: {error}"));
                }
                Self {
                    commands,
                    problems: std::sync::Arc::new(parking_lot::Mutex::new(problems)),
                }
            }
            _ => {
                let _ = volume;
                Self {}
            }
        }
    }

    pub fn play(&self, sound: Sound) {
        cfg_select! {
            feature = "audio" => {
                let _ = self.commands.send(Command::Play(sound));
            }
            _ => {
                let _ = sound;
            }
        }
    }

    /// Master volume, 0-1; 0 mutes.
    pub fn set_volume(&self, volume: f32) {
        cfg_select! {
            feature = "audio" => {
                let _ = self.commands.send(Command::Volume(volume));
            }
            _ => {
                let _ = volume;
            }
        }
    }

    /// Problems since the last call, such as a missing output device.
    pub fn problems(&self) -> Vec<String> {
        cfg_select! {
            feature = "audio" => self.problems.lock().try_iter().collect(),
            _ => Vec::new(),
        }
    }
}

#[cfg(feature = "audio")]
mod device {
    use std::num::NonZero;
    use std::sync::mpsc::{Receiver, Sender};
    use std::time::Duration;

    use degus_sim::{CLIP_RATE, FULL_VOLUME, Sound, buzzer_tone};
    use rodio::buffer::SamplesBuffer;
    use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player};

    use super::Command;

    /// The buzzer's square wave is generated at this rate.
    const BEEP_RATE: u32 = 48_000;
    /// Peak-to-peak level of the buzzer's square wave, 0-2.
    const BEEP_LEVEL: f32 = 0.8;

    pub(super) fn run(commands: &Receiver<Command>, report: &Sender<String>, mut master: f32) {
        let errors = report.clone();
        let mut device = match open(errors) {
            Ok(device) => device,
            Err(error) => {
                let _ = report.send(format!("no audio output, sounds are only logged: {error}"));
                return;
            }
        };
        // Its notice on stderr would land beside the window.
        device.log_on_drop(false);

        // Dropping a player stops what it plays.
        let mut music: Option<(Player, f32)> = None;
        let mut buzzer: Option<Player> = None;
        for command in commands {
            match command {
                Command::Volume(volume) => {
                    master = volume;
                    if let Some((player, own)) = &music {
                        player.set_volume(own * master);
                    }
                    if let Some(player) = &buzzer {
                        player.set_volume(master);
                    }
                }
                Command::Play(Sound::Clip {
                    volume, samples, ..
                }) => {
                    let own = f32::from(volume) / f32::from(FULL_VOLUME);
                    let data: Vec<f32> =
                        samples.0.iter().map(|&s| f32::from(s) / 32768.0).collect();
                    let player = Player::connect_new(device.mixer());
                    player.set_volume(own * master);
                    player.append(SamplesBuffer::new(mono(), rate(CLIP_RATE), data));
                    music = Some((player, own));
                }
                Command::Play(Sound::Beep {
                    length,
                    buzzer: config,
                }) => {
                    let (hz, duty) = buzzer_tone(&config);
                    let player = Player::connect_new(device.mixer());
                    player.set_volume(master);
                    player.append(SamplesBuffer::new(
                        mono(),
                        rate(BEEP_RATE),
                        square(hz, duty, length),
                    ));
                    buzzer = Some(player);
                }
            }
        }
    }

    fn open(errors: Sender<String>) -> Result<MixerDeviceSink, rodio::DeviceSinkError> {
        DeviceSinkBuilder::from_default_device()?
            // In place of the default, which prints to stderr.
            .with_error_callback(move |error| {
                let _ = errors.send(format!("audio output: {error}"));
            })
            .open_stream()
    }

    fn mono() -> NonZero<u16> {
        NonZero::new(1).unwrap()
    }

    fn rate(hz: u32) -> NonZero<u32> {
        NonZero::new(hz).unwrap()
    }

    /// A square wave at `hz` with `duty` high, without DC: a narrower pulse
    /// sounds quieter, as on the buzzer.
    // ASSUME: the buzzer sounds like a plain square wave.
    pub(super) fn square(hz: f64, duty: f64, length: Duration) -> Vec<f32> {
        let count = (length.as_secs_f64() * f64::from(BEEP_RATE)) as usize;
        let high = BEEP_LEVEL * (1.0 - duty as f32);
        let low = -BEEP_LEVEL * duty as f32;
        (0..count)
            .map(|i| {
                let phase = (i as f64 * hz / f64::from(BEEP_RATE)).fract();
                if phase < duty { high } else { low }
            })
            .collect()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn square_wave_has_the_length_duty_and_no_dc() {
            let wave = square(2500.0, 0.25, Duration::from_millis(100));
            assert_eq!(wave.len(), 4800);
            let high = wave.iter().filter(|&&s| s > 0.0).count();
            assert!((1199..=1201).contains(&high), "{high} high samples");
            let mean: f32 = wave.iter().sum::<f32>() / wave.len() as f32;
            assert!(mean.abs() < 1e-3, "mean {mean}");
        }

        /// Beeps on this machine's default output device: run with
        /// `--ignored` where there is one.
        #[test]
        #[ignore = "needs an audio output device"]
        fn plays_on_the_default_device() {
            let speaker = crate::Speaker::new(0.5);
            speaker.play(Sound::Beep {
                length: Duration::from_millis(200),
                buzzer: degus_set::config::Buzzer::default(),
            });
            std::thread::sleep(Duration::from_millis(400));
            assert_eq!(speaker.problems(), Vec::<String>::new());
        }
    }
}
