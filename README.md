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
# A page with a button that writes 1 to VP 0x1000, and an icon that shows
# its value.
[page.home]
id = 0
controls = ["fan", "fan_on"]

[control.fan_on]
type = "button"
area = { x = 300, y = 200, w = 120, h = 60 }
vp = 0x1000
value = 1

[control.fan]
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

For the simulator's window, add `--features gui`; it plays the display's
sounds too, which on Linux needs the ALSA development package
(`libasound2-dev`). `--features serial` also lets a host connect over a
serial port.

## Use

```sh
degus new my-hmi --screen 800x480   # creates my-hmi/project.toml
degus check my-hmi                  # reports problems
degus build my-hmi                  # writes my-hmi/target/DWIN_SET
degus sim my-hmi                    # runs it on a simulated display
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
files. To see a change, write a script of VP writes, touches and `shot`
lines and run `degus sim panel --script <file>`; it saves screenshots and
fails on unmet `expect` lines.
```

## Simulator

`degus sim` runs a project on a simulated display, straight from the TOML
and images, without building. A script runs headless: VP writes and reads
as the host, touches, screenshots and expectations, for tests and agents. A
window (`--features gui`) takes clicks as touches, shows the VPs and logs
what passes between host and display, and plays the clips and beeps the
display would. With `--tcp`, or `--serial` (`--features serial`), the
firmware drives it with DGUS frames, as it would the panel's UART2.

It is a preview: fonts are a stand-in, images skip the JPEG round trip, and
where DWIN's guide is silent the simulator guesses
([crates/sim/ASSUMPTIONS.md](crates/sim/ASSUMPTIONS.md)). Check the result
on the display.

The frames come from `degus-codec`, which firmware can use for the host's
side of the link.

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
| `degus-sim` | The display simulator. |
| `degus-sim-gui` | The simulator's window. |
| `degus-icl` | ICL image lib builder. |
| `degus-wae` | WAE audio lib builder. |
| `degus-controls` | Touch (13) and display (14) controls builder. |
| `degus-config` | `T5LCFG.CFG`. |
| `degus-codec` | UART frames, for either end of the link. `no_std`. |

degus is an independent project, not affiliated with DWIN Technology.
