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
  the types behind them. It needs no dependencies, not even degus, so it
  builds for the firmware too.
- **The panel**, a program depending on `ui` and `degus`. It builds the
  icon and audio libraries from images and sounds, places the controls on
  the pages, checks the result and writes `DWIN_SET`.
- **The firmware**, depending on `ui` and `degus-codec`, which builds and
  parses the frames the display speaks on its UART.

Because both sides take their VPs from `ui`, the panel and the firmware
cannot disagree about them.

## Install

degus is not on crates.io yet:

```sh
cargo add degus --git https://github.com/phycrax/degus
```

The panel program runs with `cargo run`. Copy the `DWIN_SET` folder it
writes to an SD card and download it to the display.

## What degus does

- A `Bundle` holds everything in `DWIN_SET`: the configuration, the touch
  and display controls of every page, the icon and audio libraries by file
  number, and any other files, such as fonts.
- `Bundle::check` reports problems by location: controls off the screen,
  icons and libraries a control names but the bundle lacks, pages without
  a background, icons too large for the chip, files that run into each
  other in flash, and more.
- `Bundle::write` checks the bundle, refuses it on errors and writes the
  folder.
- `Bundle::read` reads a folder back, including one made by DWIN's tools.

The formats it writes, and reads back:

- ICL image libraries, from PNG, BMP, JPEG and GIF;
- WAE audio libraries, from WAV, MP3, FLAC and Ogg Vorbis;
- Touch (13) and Display (14) control files, with these controls for now:
  return key code, basic touch, data input, variable icon, data variable,
  text display and progress bar. Other controls in files made by DWIN's
  tools are kept as they are;
- panel configuration, `T5LCFG.CFG`;
- font 0, the ASCII font data variables and text controls use, drawn from
  a TrueType or OpenType font with hinting for 1-bit pixels. It goes in the
  bundle's other files for now.

Other DGUS controls and the other fonts are TODO, tokens are welcome. A
simulator that runs a built `DWIN_SET` is planned.

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

The panel depends on `degus` alone, which re-exports the format crates as
`degus::icl`, `degus::wae`, `degus::controls`, `degus::config` and
`degus::font`.

| Crate | |
|-------|---|
| `degus` | Bundles: check, write and read `DWIN_SET`. |
| `degus-icl` | ICL image libraries. |
| `degus-wae` | WAE audio libraries. |
| `degus-controls` | Touch (13) and display (14) control files. |
| `degus-config` | `T5LCFG.CFG`. |
| `degus-font` | Font files: font 0 for now. |
| `degus-glyph` | Draws TrueType and OpenType glyphs for bitmap fonts. |
| `degus-codec` | UART frames, for either end of the link. `no_std`, for the firmware. |

degus is an independent project, not affiliated with DWIN Technology.
