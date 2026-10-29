# degus

Build DWIN T5L display projects from TOML.

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

degus replaces the clicking with text. A project is TOML files beside your
images and sounds, and a command-line build writes the `DWIN_SET` folder the
display downloads. The agent writes the panel next to the firmware,
`degus check` catches mistakes before the display does, and every change
shows up in a diff.

```toml
# A button that writes 1 to VP 0x1000, and an icon that shows its value.
[[control]]
type = "button"
area = { x = 300, y = 200, w = 120, h = 60 }
vp = 0x1000
value = 1

[[control]]
type = "icon"
vp = 0x1000
position = { x = 100, y = 200 }
icon_lib = 40
val_max = 1
icon_min = 0
icon_max = 1
```

## Install

```sh
cargo install --git https://github.com/phycrax/degus
```

## Use

```sh
degus new my-hmi --screen 800x480   # creates my-hmi/project.toml
degus check my-hmi                  # reports problems
degus build my-hmi                  # writes my-hmi/target/DWIN_SET
degus guide                         # prints the project format
```

Then download `DWIN_SET` to the display from an SD card. The project format,
with a complete example, is in [docs/project-format.md](docs/project-format.md),
which `degus guide` also prints.

## With coding agents

degus is made so agents can build the panel along with the firmware. To point
an agent at your panel, add something like this to your firmware repository's
`AGENTS.md` or `CLAUDE.md`:

```markdown
## Display panel

`panel/` is a degus project for the DWIN T5L display. Run `degus guide` before
editing it and `degus check panel` after each change; `degus build panel`
writes `panel/target/DWIN_SET`. The panel and the firmware share data only
through VPs, so keep the firmware's VP addresses in sync with the controls
files.
```

degus has no previewing capability yet, check the result on the display.

## Building capabilities

- ICL image libraries, from PNG, BMP, JPEG and GIF;
- WAE audio libraries, from WAV, MP3, FLAC and Ogg Vorbis;
- Touch (13) and Display (14) control files, with these controls for now: return
  key code, basic touch, data input, variable icon, data variable, text
  display and progress bar;
- panel configuration, `T5LCFG.CFG`.

Other files, such as fonts, are copied into `DWIN_SET` as they are. Other
DGUS controls and building fonts are TODO, tokens are welcome.

## Crates

| Crate | |
|-------|---|
| `degus` | The command-line tool. |
| `degus-project` | Loads, checks and builds projects. |
| `degus-icl` | ICL image lib builder. |
| `degus-wae` | WAE audio lib builder. |
| `degus-controls` | Touch (13) and display (14) controls builder. |
| `degus-config` | `T5LCFG.CFG`. |

degus is an independent project, not affiliated with DWIN Technology.
