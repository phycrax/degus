//! The simulator behind DGUS frames: the examples in DWIN's guide end to
//! end, and a touch uploaded as a frame.

use std::path::{Path, PathBuf};

use degus_sim::{Codec, Decoded, FrameCodec, Sim};
use image::{Rgb, RgbImage};

fn write(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) {
    let path = path.as_ref();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// A project with one button, which writes 1 to 0x1000. `config` goes into
/// `[config]`.
fn project(name: &str, config: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("degus-sim-frames")
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
{config}

[[icl]]
id = 32
images = [{{ id = 0, path = "home.png" }}]
"#
        ),
    );
    write(
        dir.join("home.toml"),
        r#"
[control.button]
type = "button"
area = { x = 0, y = 0, w = 50, h = 50 }
vp = 0x1000
value = 1
"#,
    );
    RgbImage::from_pixel(100, 100, Rgb([0, 0, 64]))
        .save(dir.join("home.png"))
        .unwrap();
    dir
}

/// Bytes written as the guide writes them: `5AA5 05 82 1000 0002`.
fn hex(text: &str) -> Vec<u8> {
    let digits: String = text.split_whitespace().collect();
    (0..digits.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&digits[i..i + 2], 16).unwrap())
        .collect()
}

/// Sends `frame` from the host and returns what the display answers.
fn exchange(sim: &mut Sim, codec: &mut FrameCodec, frame: &str) -> Vec<u8> {
    let mut decoded = Vec::new();
    codec.decode(&hex(frame), &mut decoded);
    let mut answer = Vec::new();
    for item in decoded {
        let Decoded::Cmd(cmd) = item else {
            panic!("{frame}: {item:?}");
        };
        if let Some(reply) = sim.handle(&cmd) {
            codec.encode(&reply, &mut answer).unwrap();
        }
    }
    answer
}

#[test]
fn answers_the_guides_frames() {
    // §4.2.1's write, read back, and the version read from 0x000C's examples.
    let mut sim = Sim::load(project("plain", "")).unwrap();
    let mut codec = FrameCodec::new(sim.world().config.crc);
    let mut talk = |frame| exchange(&mut sim, &mut codec, frame);
    assert_eq!(talk("5AA5 05 82 1000 0002"), hex("5AA5 03 82 4F4B"));
    assert_eq!(talk("5AA5 04 83 1000 01"), hex("5AA5 06 83 1000 01 0002"));
    assert_eq!(talk("5AA5 04 83 000F 01"), hex("5AA5 06 83 000F 01 6322"));

    // The same with CRC on.
    let mut sim = Sim::load(project("crc", "crc = true")).unwrap();
    let mut codec = FrameCodec::new(sim.world().config.crc);
    let mut talk = |frame| exchange(&mut sim, &mut codec, frame);
    assert_eq!(
        talk("5AA5 07 82 1000 3132 CC9B"),
        hex("5AA5 05 82 4F4B A5EF")
    );
    assert_eq!(
        talk("5AA5 06 83 000F 01 ED90"),
        hex("5AA5 08 83 000F 01 6322 E5D5")
    );
}

#[test]
fn uploads_touches_as_frames() {
    let mut sim = Sim::load(project("touch", "")).unwrap();
    let mut codec = FrameCodec::new(false);
    sim.touch_down(10, 10);
    sim.touch_up();
    let mut sent = Vec::new();
    for msg in sim.take_outbox() {
        codec.encode(&msg, &mut sent).unwrap();
    }
    assert_eq!(sent, hex("5AA5 06 83 1000 01 0001"));
}
