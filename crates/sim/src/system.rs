//! System variables, `0x0000`–`0x0FFF`, as DWIN's T5L DGUS II application
//! guide describes them (chapter 5). Writes the simulator does not act on
//! are stored and reported once.

use std::time::Duration;

use degus_set::config::Audio;

use crate::audio::{FULL_VOLUME, Samples, Sound};
use crate::sim::{Event, Once, Sim};
use crate::vp::USER_START;

/// Write `55AA 5AA5` to reset.
pub const SYSTEM_RESET: u16 = 0x0004;
/// Application version: `0x000E` CTP driver, `0x000F` GUI and OS.
pub const VERSION: u16 = 0x000E;
/// Four words: year, month, day, weekday, hour, minute, second, 0.
pub const RTC: u16 = 0x0010;
/// The page shown.
pub const PIC_NOW: u16 = 0x0014;
pub const GUI_STATUS: u16 = 0x0015;
/// Four words: `5A` fresh mark and state, X, Y, 0.
pub const TP_STATUS: u16 = 0x0016;
/// `5A` and the backlight brightness.
pub const LED_NOW: u16 = 0x0031;
pub const LCD_HOR: u16 = 0x007A;
pub const LCD_VER: u16 = 0x007B;
/// `5A`, touch panel settings, then the system flags of CFG 0x05.
pub const SYSTEM_CONFIG: u16 = 0x0080;
/// Brightness on and in standby, then the standby time.
pub const LED_CONFIG: u16 = 0x0082;
/// `5A 01` then a page ID: switch page.
pub const PIC_SET: u16 = 0x0084;
/// Four words. Music mode: clip ID and `01` to play it; volume in 1/64
/// steps and the play status (0 stopped, 2 playing); `5A` and a WAE file
/// number to play from instead of the configuration's. Buzzer mode: the
/// low byte of the first word beeps for that many 8 ms.
pub const MUSIC_PLAY: u16 = 0x00A0;
/// `5A` enables switching the background ICL to the one in `0x00DF`.
pub const ICL_SWITCH: u16 = 0x00DE;

/// `0x00A0` play status.
const STOPPED: u8 = 0x00;
const PLAYING: u8 = 0x02;
/// What `0x000E`-`0x000F` read: the guide's example, GUI V63 and OS V22.
const VERSION_WORDS: [u16; 2] = [0x0000, 0x6322];
/// Marks a command word to be acted on once.
const ENABLE: u8 = 0x5A;

/// System VPs whose writes the simulator handles or keeps.
const HANDLED: [(u16, u16); 8] = [
    (SYSTEM_RESET, 4),
    (RTC, 4),
    (TP_STATUS, 4),
    (SYSTEM_CONFIG, 6),
    (LED_CONFIG, 2),
    (PIC_SET, 2),
    (MUSIC_PLAY, 4),
    (ICL_SWITCH, 2),
];
/// System VPs only the display writes.
const READ_ONLY: [(u16, u16); 5] = [
    (VERSION, 2),
    (PIC_NOW, 2),
    (LED_NOW, 1),
    (LCD_HOR, 2),
    (0x0032, 8),
];

fn touches(start: u16, words: usize, (at, len): (u16, u16)) -> bool {
    let end = usize::from(start) + words;
    usize::from(start) < usize::from(at) + usize::from(len) && usize::from(at) < end
}

impl Sim {
    /// Sets the system variables to their power-on values.
    pub(crate) fn init_system(&mut self) {
        let config = &self.config;
        let led = u16::from_be_bytes([
            config.brightness.min(100),
            config.standby_brightness.min(100),
        ]);
        let standby_delay = config.standby_delay;
        let _ = self.vp.write(LED_CONFIG, &[led, standby_delay]);
        // The guide: volume starts at 0x40, 100 %.
        self.vp.set_high(MUSIC_PLAY + 1, FULL_VOLUME);
        self.sync_readonly();
    }

    /// Acts on a write to `0x00A0`: plays a clip in music mode, beeps in
    /// buzzer mode.
    // ASSUME: the display clears the count byte in both modes, a volume
    // above 0x40 plays at 100 %, and a new clip cuts off the one playing.
    fn music_command(&mut self) {
        let count = self.vp.low(MUSIC_PLAY);
        if count != 0 {
            self.vp.set_low(MUSIC_PLAY, 0);
            match self.config.audio {
                Audio::Buzzer => {
                    self.beep(Duration::from_millis(8) * u32::from(count));
                }
                Audio::Music => {
                    let id = self.vp.high(MUSIC_PLAY);
                    let volume = self.vp.high(MUSIC_PLAY + 1).min(FULL_VOLUME);
                    let wae = if self.vp.high(MUSIC_PLAY + 2) == ENABLE {
                        u16::from_be_bytes([
                            self.vp.low(MUSIC_PLAY + 2),
                            self.vp.high(MUSIC_PLAY + 3),
                        ])
                    } else {
                        self.config.music_wae.into()
                    };
                    self.play_clip(wae, id.into(), volume);
                }
            }
        }
        // The status byte is the display's.
        let status = if self.music_until.is_some() {
            PLAYING
        } else {
            STOPPED
        };
        self.vp.set_low(MUSIC_PLAY + 1, status);
    }

    fn play_clip(&mut self, wae: u16, id: u16, volume: u8) {
        let samples = u8::try_from(wae).ok().and_then(|wae| {
            if let Some(samples) = self.clips.get(&(wae, id)) {
                return Some((wae, samples.clone()));
            }
            let clip = self.set.waes.get(&wae)?.clips.get(&id)?;
            let samples = Samples(clip.as_slice().into());
            self.clips.insert((wae, id), samples.clone());
            Some((wae, samples))
        });
        let Some((wae, samples)) = samples else {
            self.note(format!("no clip {id} in {wae}.wae to play"));
            return;
        };
        self.music_until = Some(self.clock + samples.duration());
        self.events.push(Event::Sound(Sound::Clip {
            wae,
            id,
            volume,
            samples,
        }));
    }

    /// Sounds the buzzer for `length`.
    pub(crate) fn beep(&mut self, length: Duration) {
        self.events.push(Event::Sound(Sound::Beep {
            length,
            // A configuration without the block keeps the factory buzzer.
            buzzer: self.config.buzzer.unwrap_or_default(),
        }));
    }

    /// Marks the clip stopped once it has played to its end.
    pub(crate) fn check_music(&mut self) {
        if let Some(until) = self.music_until
            && self.clock >= until
        {
            self.music_until = None;
            self.vp.set_low(MUSIC_PLAY + 1, STOPPED);
        }
    }

    /// Rewrites what only the display writes: version, page, backlight,
    /// resolution and the system flags.
    pub(crate) fn sync_readonly(&mut self) {
        let config = &self.config;
        let flags = u8::from(config.crc) << 7
            | u8::from(config.load_22) << 5
            | u8::from(self.auto_upload) << 4
            | u8::from(self.touch_sound) << 3
            | u8::from(config.standby) << 2
            | config.rotation as u8 & 0x03;
        let screen = self.screen();
        let _ = self.vp.write(VERSION, &VERSION_WORDS);
        self.vp.set(PIC_NOW, self.page);
        self.vp.set(GUI_STATUS, 0);
        self.vp
            .set(LED_NOW, u16::from_be_bytes([ENABLE, self.brightness]));
        self.vp.set(LCD_HOR, screen.w);
        self.vp.set(LCD_VER, screen.h);
        self.vp.set_low(SYSTEM_CONFIG + 1, flags);
    }

    /// The ICL `0x00DE`/`0x00DF` switched to, if enabled.
    // ASSUME: the switch holds while 0x00DE's high byte stays 5A, and 0x00DF
    // holds the whole ICL number.
    pub(crate) fn switched_icl(&self) -> Option<u8> {
        let icl = u8::try_from(self.vp.get(ICL_SWITCH + 1)).ok()?;
        (self.vp.high(ICL_SWITCH) == ENABLE && icl != 0).then_some(icl)
    }

    /// Acts on a write of `words` words from `start`.
    pub(crate) fn after_write(&mut self, start: u16, words: usize) {
        if start >= USER_START {
            return;
        }
        let hit = |range| touches(start, words, range);

        if hit((SYSTEM_RESET, 2)) && self.vp.read(SYSTEM_RESET, 2) == Some(&[0x55AA, 0x5AA5][..]) {
            self.power_on();
            self.events.push(Event::Reset);
            return;
        }
        if hit((LED_CONFIG, 1)) {
            self.brightness = self.vp.high(LED_CONFIG).min(100);
        }
        if hit((SYSTEM_CONFIG, 2)) && self.vp.high(SYSTEM_CONFIG) == ENABLE {
            let flags = self.vp.low(SYSTEM_CONFIG + 1);
            self.auto_upload = flags & 0x10 != 0;
            self.touch_sound = flags & 0x08 != 0;
            self.vp.set_high(SYSTEM_CONFIG, 0);
        }
        if hit((PIC_SET, 2)) && self.vp.high(PIC_SET) == ENABLE {
            let mode = self.vp.low(PIC_SET);
            let page = self.vp.get(PIC_SET + 1);
            if mode != 0x01 {
                self.note(format!("PIC_Set mode {mode:#04X} is not simulated"));
            } else if self.has_background(page) {
                self.set_page(page);
            } else {
                // The guide: a page without an image is ignored.
                self.note(format!(
                    "PIC_Set ignored: no page {page} in {}.icl",
                    self.background_icl()
                ));
            }
            self.vp.set_high(PIC_SET, 0);
        }
        if hit((MUSIC_PLAY, 4)) {
            self.music_command();
        }

        for range in READ_ONLY {
            if hit(range) {
                self.note_once(
                    Once::SystemVp(range.0),
                    format!("{:#06X} is read-only; the write is undone", range.0),
                );
            }
        }
        self.sync_readonly();
        let end = (usize::from(start) + words).min(USER_START.into());
        for vp in usize::from(start)..end {
            let vp = vp as u16;
            let known = HANDLED
                .iter()
                .chain(&READ_ONLY)
                .any(|&(at, len)| (at..at + len).contains(&vp));
            if !known {
                self.note_once(
                    Once::SystemVp(vp),
                    format!("system VP {vp:#06X} is not simulated; the write is only stored"),
                );
            }
        }
    }

    /// Fills VPs a read is about to see.
    pub(crate) fn before_read(&mut self, start: u16, words: usize) {
        if touches(start, words, (RTC, 4)) {
            let seconds = self.rtc_start + self.clock.as_secs();
            let _ = self.vp.write(RTC, &rtc_words(seconds));
        }
    }
}

/// The RTC words for `seconds` since 1970, weeks starting on Sunday as 0.
pub(crate) fn rtc_words(seconds: u64) -> [u16; 4] {
    let days = (seconds / 86_400) as i64;
    let time = seconds % 86_400;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let weekday = (days + 4).rem_euclid(7);
    let byte = |v: i64| v as u8;
    [
        u16::from_be_bytes([byte(year % 100), byte(month)]),
        u16::from_be_bytes([byte(day), byte(weekday)]),
        u16::from_be_bytes([(time / 3600) as u8, (time / 60 % 60) as u8]),
        u16::from_be_bytes([(time % 60) as u8, 0]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_the_clock_to_rtc_words() {
        // 2019-10-01 11:12:13 UTC, a Tuesday.
        assert_eq!(rtc_words(1_569_928_333), [0x130A, 0x0102, 0x0B0C, 0x0D00]);
        // 2000-01-01 00:00:00, a Saturday.
        assert_eq!(rtc_words(946_684_800), [0x0001, 0x0106, 0x0000, 0x0000]);
    }
}
