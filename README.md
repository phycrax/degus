# degus

Build DWIN T5L display panels from Rust.

Are you tired of using DisGUSting UI editor tools?

- Booting Windows to move a button three pixels to the left?
- Drawing every control by hand, then fixing its coordinates in a property
  panel?
- Dealing with binary files, not even being able to review?
- Nothing to stop an icon library from running into your sound library in
  flash?
- Repeatedly clicking the same settings and waiting for that damn ICL generation?

There has to be a better way. Why spend ATP when you can spend electricity?
Your agent already writes the firmware. Now, with degus, it builds the panel too.

degus replaces the clicking with code. Your panel is a small Rust program
beside its images and sounds: it describes the pages, their controls and
the libraries, and degus writes the `DWIN_SET` folder the display downloads.
The compiler and degus's check catch mistakes before the display does, and
every change shows up in a diff.

## How a project fits together

A typical product has three crates:

- **`ui`**, shared by the other two: page IDs, VP addresses, icon IDs and
  the types behind them. It needs no degus crate, at most `serde` for the
  types the firmware sends, so it builds for the firmware too.
- **The panel**, a program depending on `ui` and `degus-set`. It builds
  the icon and audio libraries from images and sounds, places the controls
  on the pages, checks the result and writes `DWIN_SET`.
- **The firmware**, depending on `ui` and `degus-driver`, which talks to
  the display over its UART.

Because both sides take their VPs from `ui`, the panel and the firmware
cannot disagree about them.

## Install

degus is not on crates.io yet:

```sh
cargo add degus-set --git https://github.com/phycrax/degus
```

The panel program runs with `cargo run`. Copy the `DWIN_SET` folder it
writes to an SD card and download it to the display.

## What degus does

- A `DwinSet` holds everything in `DWIN_SET`: the configuration, the touch
  and display controls of every page, the icon and audio libraries by file
  number, font 0, and any other files, such as the other fonts.
- `DwinSet::check` reports problems by location: controls off the screen,
  icons and libraries a control names but the set lacks, pages without
  a background, icons too large for the chip, files that run into each
  other in flash, font 0 sizes that controls ask for but font 0 lacks,
  digits the set's font 0 doesn't draw, and more.
- `DwinSet::write` checks the set, refuses it on errors and writes the
  folder.
- `DwinSet::read` reads a folder back, including one made by DWIN's tools.

The formats it writes, and reads back:

- ICL image libraries, from PNG, BMP, JPEG and GIF;
- WAE audio libraries, from WAV, MP3, FLAC and Ogg Vorbis;
- Touch (13) and Display (14) control files, with these controls for now:
  return key code, basic touch, data input, variable icon, data variable,
  text display and progress bar. Other controls in files made by DWIN's
  tools are kept as they are;
- panel configuration, `T5LCFG.CFG`;
- font 0, the ASCII font data variables and text controls use, drawn from
  a TrueType or OpenType font with hinting for 1-bit pixels.

Other DGUS controls and the other fonts are TODO, tokens are welcome.

## Simulating the display

`degus-sim` runs a `DwinSet`, one built in Rust or one read from a
`DWIN_SET` folder, without a display. It takes the host's UART bytes,
touches and time, and gives back the display's answers and uploads, the
sounds it plays and the screen it draws, so tests can press buttons and
check VPs and pixels. It draws icons from the set's ICLs and text with the
set's font 0, if it has one; characters in other fonts draw as boxes.
Where DWIN's guide leaves the display's behaviour open,
`crates/sim/ASSUMPTIONS.md` lists what the simulator picks.

The `degus` command opens it in a window:

```sh
cargo install degus-cli --git https://github.com/phycrax/degus
degus sim path/to/DWIN_SET --tcp 127.0.0.1:5000
```

Clicks are touches. The window shows the pages, VP watches, the VP
memory, and a log of the frames between host and display, and plays the
display's sounds. Its reload button reads the folder again, keeping the
page and the VPs. Firmware connects as the host over `--tcp` or
`--serial`, and `--size` gives the screen size when the set doesn't.

`degus-sim-gui` puts the display itself into any eframe app, such as
firmware built for the PC that shows its display beside its own controls.
It looks and acts like a panel on the desk:

- `SimThread` runs the simulator on a thread of its own, as a panel runs
  on its own chip, so that it answers the host in time however busy the
  window is.
- The host talks to it over its `Uart`, which reads and writes like a
  serial port.
- `Screen` shows it, dimmed by the backlight, and turns the pointer into
  touches. It never takes the keyboard focus, so the app's keys stay the
  app's.
- `Speaker` plays its sounds.

Tools that look inside, as `degus sim` does, read the simulator with
`SimThread::lock`, change it with `update`, and watch the UART and the
display's events with `monitor`. The crate re-exports `eframe` and
`egui`, so that the app builds with the same ones.
`crates/sim-gui/examples/embed.rs` shows a set built in code beside a
panel of the app's own.

## Talking to the display

`degus-driver` is the firmware's side of the UART link, for async firmware
with `embassy-sync` and `embedded-io-async`. It is `no_std`.

- `Display::write` writes a value to a VP and waits for the display's
  acknowledgement; `Display::read` reads a value back. Values are encoded
  with serde: numbers big-endian, `bool` and unit enum variants as a word,
  and a struct as its fields over the VPs that follow.
- A `Runner` reads the UART, matches each answer to the request waiting
  for it, and hands the display's uploads, such as a VP a touch control
  changed, to a router.
- A request can be cancelled, as by a timeout, and the next one won't take
  its answer.

`examples/fan` drives a panel from a PC's serial port. Underneath,
`degus-serde` encodes the values and `degus-codec` the frames; both work
on their own, without the driver.

## With coding agents

degus is made so agents can build the panel along with the firmware. To
point an agent at your panel, add something like this to your firmware
repository's `AGENTS.md` or `CLAUDE.md`:

```markdown
## Display panel

`panel/` builds the DWIN T5L display's `DWIN_SET` folder with the degus
library; `cargo run -p panel` checks it and writes the folder. The panel
and the firmware share data only through VPs, which both take from the
`ui` crate. Keep page IDs, VPs and icon IDs there, never as literals in
either side.
```

## Crates

The panel depends on `degus-set` alone, which re-exports the format crates
as `degus_set::icl`, `degus_set::wae`, `degus_set::controls`,
`degus_set::config` and `degus_set::font`. The firmware depends on
`degus-driver`.

| Crate | |
|-------|---|
| `degus-set` | The `DWIN_SET` folder: check, write and read it. |
| `degus-sim` | Simulates a display running a `DWIN_SET`. |
| `degus-sim-gui` | A simulated display in an eframe app: UART, touch screen and speaker. |
| `degus-cli` | The `degus` command: `degus sim` runs a `DWIN_SET` in a window. |
| `degus-icl` | ICL image libraries. |
| `degus-wae` | WAE audio libraries. |
| `degus-controls` | Touch (13) and display (14) control files. |
| `degus-config` | `T5LCFG.CFG`. |
| `degus-font` | Font files: font 0 for now. |
| `degus-glyph` | Draws TrueType and OpenType glyphs for bitmap fonts. |
| `degus-driver` | The firmware's side of the UART link, async. `no_std`. |
| `degus-serde` | VP values, with serde. `no_std`. |
| `degus-codec` | UART frames, for either end of the link. `no_std`. |

degus is an independent project, not affiliated with DWIN Technology.
