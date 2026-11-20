//! Sound: `0x00A0` in music and buzzer mode, the play status, and touch
//! sounds.

use std::path::{Path, PathBuf};
use std::time::Duration;

use degus_sim::{Event, Runner, Sim, Sound};
use image::{Rgb, RgbImage};

const BUTTON: &str = r#"
[control.button]
type = "button"
area = { x = 0, y = 0, w = 50, h = 50 }
vp = 0x1000
value = 1
"#;

fn write(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) {
    let path = path.as_ref();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// A 32 kHz mono tone of `ms` milliseconds.
fn wav(path: impl AsRef<Path>, ms: u32) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 32_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for i in 0..32 * ms {
        writer.write_sample(((i % 64) as i16 - 32) * 256).unwrap();
    }
    writer.finalize().unwrap();
}

/// A project with one button and two WAEs: 60 with clips 0 (500 ms) and
/// 1 (250 ms), 61 with clip 0. `config` goes into `[config]`.
fn project(name: &str, config: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("degus-sim-audio")
        .join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    write(
        dir.join("project.toml"),
        format!(
            r#"
screen = {{ w = 100, h = 100 }}
controls = ["home.toml"]
pages = [{{ id = 0, controls = ["button"] }}]

[config]
background_icl = 32
music_wae = 60
{config}

[[icl]]
id = 32
images = [{{ id = 0, path = "home.png" }}]

[[wae]]
id = 60
clips = [{{ id = 0, path = "long.wav" }}, {{ id = 1, path = "short.wav" }}]

[[wae]]
id = 61
format = "companded8"
clips = [{{ id = 0, path = "other.wav" }}]
"#
        ),
    );
    write(dir.join("home.toml"), BUTTON);
    RgbImage::from_pixel(100, 100, Rgb([0, 0, 64]))
        .save(dir.join("home.png"))
        .unwrap();
    wav(dir.join("long.wav"), 500);
    wav(dir.join("short.wav"), 250);
    wav(dir.join("other.wav"), 100);
    dir
}

fn run(sim: &mut Sim, script: &str) -> Vec<String> {
    let mut log = Vec::new();
    let result =
        Runner::new(sim, std::env::temp_dir(), |line| log.push(line.to_owned())).run(script);
    if let Err(error) = result {
        panic!("{error}\n{}", log.join("\n"));
    }
    log
}

fn sounds(sim: &mut Sim) -> Vec<Sound> {
    sim.take_events()
        .into_iter()
        .filter_map(|event| match event {
            Event::Sound(sound) => Some(sound),
            _ => None,
        })
        .collect()
}

/// The one sound since the last call.
fn one(sim: &mut Sim) -> Sound {
    let mut played = sounds(sim);
    assert_eq!(played.len(), 1, "{played:?}");
    played.remove(0)
}

#[test]
fn plays_clips_in_music_mode_and_reports_the_status() {
    let mut sim = Sim::load(project("music", "audio = \"music\"")).unwrap();
    let log = run(
        &mut sim,
        "
        expect 0x00A1 0x4000        # full volume at power-on, stopped
        write 0x00A0 0x0101         # clip 1, play once
        expect sound clip 1
        expect 0x00A0 0x0100        # the display clears the count
        expect 0x00A1 0x4002        # playing
        wait 200ms
        expect 0x00A1 0x4002
        wait 100ms
        expect 0x00A1 0x4000        # 250 ms later, stopped
        write 0x00A0 0x0901         # no clip 9
        expect sound none
        ",
    );
    assert!(
        log.iter()
            .any(|line| line.contains("# sound clip 1 of 60.wae at 100%"))
    );
    assert!(log.iter().any(|line| line.contains("no clip 9 in 60.wae")));
}

#[test]
fn takes_the_volume_and_another_wae_from_0x00a0() {
    let mut sim = Sim::load(project("override", "audio = \"music\"")).unwrap();
    sim.write_vp(0x00A0, &[0x0001, 0x2000]).unwrap();
    let Sound::Clip {
        wae,
        id,
        volume,
        samples,
    } = one(&mut sim)
    else {
        panic!("expected one clip");
    };
    assert_eq!((wae, id, volume), (60, 0, 0x20));
    // 500 ms at 32 kHz, and the build's one trailing zero.
    assert_eq!(samples.0.len(), 16_001);
    assert_eq!(samples.duration().as_millis(), 500);

    // 5A then WAE 61 in D2:D1; a volume over 0x40 plays at 100 %.
    sim.write_vp(0x00A0, &[0x0001, 0x7000, 0x5A00, 0x3D00])
        .unwrap();
    let Sound::Clip {
        wae,
        id,
        volume,
        samples,
    } = one(&mut sim)
    else {
        panic!("expected one clip");
    };
    assert_eq!((wae, id, volume), (61, 0, 0x40));
    // The 8-bit clip plays with its loss: the source holds multiples of
    // 256, and companded codes expand to values between them.
    assert!(samples.0.iter().any(|&s| s % 256 != 0));
}

#[test]
fn music_mode_touches_are_silent_without_voice_ids() {
    let mut sim = Sim::load(project("music-touch", "audio = \"music\"")).unwrap();
    let log = run(
        &mut sim,
        "
        touch 10 10
        touch 10 10
        expect sound none
        ",
    );
    let notes = log.iter().filter(|line| line.contains("voice IDs")).count();
    assert_eq!(notes, 1);
}

#[test]
fn beeps_in_buzzer_mode() {
    let mut sim = Sim::load(project("buzzer", "")).unwrap();
    run(
        &mut sim,
        "
        write 0x00A0 0x007D         # 125 x 8 ms
        expect sound beep 1000ms
        expect 0x00A0 0x0000
        touch 10 10                 # a touch control: the factory 100 ms
        expect sound beep 100ms
        touch 90 90                 # bare screen: silent
        expect sound none
        write 0x0080 0x5A00 0x0010  # touch sound off, auto-upload on
        touch 10 10
        expect sound none
        ",
    );
}

#[test]
fn takes_the_buzzer_from_the_configuration() {
    let config =
        "touch_sound = true\n[config.buzzer]\ndiv1 = 0x6E\ndiv2 = 3000\nduty = 1500\ntime = 5";
    let mut sim = Sim::load(project("buzzer-config", config)).unwrap();
    sim.touch_down(10, 10);
    let Sound::Beep { length, buzzer } = one(&mut sim) else {
        panic!("expected one beep");
    };
    assert_eq!(length, Duration::from_millis(50));
    assert_eq!(degus_sim::buzzer_tone(&buzzer).1, 0.5);

    // Without the setting, touches are silent.
    let mut sim = Sim::load(project("quiet", "touch_sound = false")).unwrap();
    sim.touch_down(10, 10);
    assert!(sounds(&mut sim).is_empty());
}
