# degus project format

A degus project describes a DWIN T5L display in TOML: its pages, their
controls, and the images and sounds they use. `degus build` turns it into
the `DWIN_SET` folder the display downloads from an SD card.

## Workflow

```sh
degus new my-hmi --screen 800x480   # creates my-hmi/project.toml
degus check my-hmi                  # reports problems, builds nothing
degus build my-hmi                  # writes my-hmi/target/DWIN_SET
degus clean my-hmi                  # deletes it
degus guide                         # prints this document
```

The project folder defaults to the current one, and `degus build -o <dir>`
writes `<dir>/DWIN_SET` instead. Each build replaces `DWIN_SET` whole.

To download it, copy `DWIN_SET` to the root of an SD card formatted FAT32
with 4 KB clusters, insert the card while the display is off, and power it
on. When the blue screen says the download has ended, power off, remove the
card and power on again.

## How the display works

- **Pages.** A page is a full-screen background image and the controls on
  it. Page N shows image N of the background ICL (`background_icl` in
  `[config]`, 32 unless set). The display starts on page 0.
- **VPs.** A VP (variable pointer) is the address of a 16-bit word in the
  display's memory. VPs are the only interface between panel and firmware:
  display controls draw VP values, touch controls write them, and the
  firmware reads and writes them over the UART. Use 0x1000–0xFFFF;
  0x0000–0x0FFF are the display's system variables.
- **Controls.** Touch controls (`button`, `key`, `input`) react to touches
  in an area. Display controls (`icon`, `data`, `text`, `progress`) draw VP
  values.
- **Libraries.** An ICL holds images, a WAE holds audio clips. Both are numbered
  files, and file N is stored at N × 256 KB in flash. As an example, a 600 KB 
  `32.icl` also takes numbers 33 and 34. The build writes the controls to files 13
  and 14.
- **Fonts.** Font 0, ASCII in sizes 4×8 to 64×128, is built into the
  display. degus does not build fonts yet; add other font files to `copy`,
  named by their font ID (`24_symbols.HZK` is font 24).

## A small project

```
my-hmi/
  project.toml
  controls/home.toml
  images/home.png  images/fan-off.png  images/fan-on.png
```

`project.toml` lists every file with its ID, so file and folder names are up
to you.

```toml
# project.toml
screen = { w = 800, h = 480 }
controls = [{ id = 0, path = "controls/home.toml" }]

[config]
background_icl = 32

[[icl]]
id = 32
images = [{ id = 0, path = "images/home.png" }]

[[icl]]
id = 40
images = [
    { id = 0, path = "images/fan-off.png" },
    { id = 1, path = "images/fan-on.png" },
]
```

```toml
# controls/home.toml: page 0

# Fan state at 0x1000: 0 off, 1 on.
[[control]]
type = "icon"
vp = 0x1000
position = { x = 100, y = 200 }
icon_lib = 40
val_max = 1
icon_min = 0
icon_max = 1

[[control]]
type = "button"
name = "fan on"
area = { x = 300, y = 200, w = 120, h = 60 }
vp = 0x1000
value = 1

[[control]]
type = "button"
name = "fan off"
area = { x = 440, y = 200, w = 120, h = 60 }
vp = 0x1000
value = 0

# Temperature in 0.1 °C, written by the firmware: 235 shows "23.5 C".
[[control]]
type = "data"
vp = 0x1001
position = { x = 100, y = 100 }
font_size = 24
integer_digits = 3
decimal_digits = 1
unit = " C"
```

The buttons write VP 0x1000 themselves, so the icon follows a touch without
the firmware, and the display reports each write over the UART.

## Conventions

- Numbers may be written in hex: `vp = 0x1000`.
- Areas are `{ x, y, w, h }` and positions `{ x, y }`, in pixels from the
  top left corner.
- Colours are `"#RRGGBB"`. The display stores RGB565, so the low bits of
  each channel are dropped.
- Paths are relative to the project folder, wherever degus runs.
- Unknown keys are errors, so a typo never falls back to a default.
- TOML puts every key after a table header such as `[config]` or `[[icl]]`
  into that table, so plain keys like `controls` and `copy` go first.

## `project.toml`

| Key | Default | Meaning |
|-----|---------|---------|
| `screen` | required | `{ w, h }`. Controls must fit inside it. |
| `chip` | `"t5l2"` | `"t5l1"` or `"t5l2"`. Sets the largest image: about 248 KB of JPEG on a T5L1, 760 KB on a T5L2. |
| `output` | `"target"` | Folder the build writes `DWIN_SET` in. |
| `controls` | `[]` | The controls file of each page, `{ id, path }`; `id` is the page ID, below 4092. Several pages may share a file. A page without controls needs no entry. |
| `copy` | `[]` | Files copied into `DWIN_SET` unchanged, such as fonts. |
| `[config]` | none | The hardware configuration, written as `T5LCFG.CFG`. |
| `[[icl]]` | none | An image library. |
| `[[wae]]` | none | An audio library. |

### `[[icl]]`: images

| Key | Default | Meaning |
|-----|---------|---------|
| `id` | required | File number; the ICL is written as `<id>.icl`. Icon controls name it in `icon_lib`. |
| `images` | required | `{ id, path }` each; `id` is the image ID. PNG, BMP, JPEG or GIF. |
| `quality` | `90` | JPEG quality, 1–100. |
| `subsampling` | `"4:2:0"` | `"4:4:4"` keeps colour edges sharper, such as on text, and makes larger files. |
| `passthrough_jpeg` | `false` | Store JPEG sources as they are when the display can decode them. |

Images are stored as JPEG, without transparency: transparent pixels are
blended onto white.

### `[[wae]]`: audio

| Key | Default | Meaning |
|-----|---------|---------|
| `id` | required | File number; the WAE is written as `<id>.wae`. |
| `clips` | required | `{ id, path }` each; `id` is the clip ID. WAV, MP3, FLAC or Ogg Vorbis. |
| `format` | `"pcm16"` | `"pcm16"` or `"companded8"`, DWIN's 8-bit "12:8" format at half the size. |
| `downmix` | `"left"` | How stereo becomes mono: `"left"` keeps the left channel, `"average"` mixes them. |

Clips are converted to 32 kHz mono, the only rate the display plays.

### `[config]`: hardware configuration

Without `[config]`, the build writes no `T5LCFG.CFG`, and the display keeps
the configuration it has. With it, these settings are always written, with
the defaults shown for keys left out:

```toml
[config]
background_icl = 32
baud = 115200
standby = true
standby_brightness = 20
```

| Key | Default | Meaning |
|-----|---------|---------|
| `background_icl` | `32` | ICL holding the page backgrounds, 16–63. The check looks at backgrounds only when it is set. |
| `music_wae` | `0` | WAE that sounds play from, 0–63. |
| `audio` | `"buzzer"` | What plays touch sounds: `"buzzer"` or `"music"` (the WAE). |
| `touch_sound` | `true` | Sound on touch. |
| `auto_upload` | `true` | Report VPs written by touch controls over the UART. |
| `crc` | `false` | UART frames carry a CRC-16. |
| `baud` | `115200` | UART speed. The display runs at 3225600 / n, and the nearest such rate must be within 2 %. |
| `brightness` | `100` | Backlight at power-on, 0–100 %. |
| `standby` | `false` | Dim the backlight to `standby_brightness` after `standby_delay` without a touch. |
| `standby_brightness` | `0` | 0–100 %. |
| `standby_delay` | `1000` | In 10 ms units. |
| `rotation` | `0` | `0`, `90`, `180` or `270`. At 90 and 270, `screen` is the panel turned on its side. |
| `load_22` | `false` | Load initial VP values from file 22 (or `init_file`) at power-on. |
| `touch_report_div` | `40` | Touch report rate is 400 Hz / value. |
| `pwm_backlight` | `false` | The PWM output drives the backlight. |
| `nand_flash` | none | An added SPI NAND flash: `"1gbit"` or `"4gbit"`. |
| `format_nand_flash` | `false` | Format the NAND flash each time the file is loaded. |
| `erase_internal_flash` | `false` | Erase the internal flash each time the file is loaded. |
| `adc_16bit` | `false` | 16-bit AD readings instead of 12-bit. |
| `encrypt_8051` | `false` | The OS CPU's 8051 program is downloaded encrypted. |
| `run_os_during_download` | `false` | The OS CPU keeps running during an SD download. |

The following are optional blocks. A block left out is written disabled, and
the display keeps its own setting, so the factory LCD and touch panel
settings stay as they are.

| Key | Meaning |
|-----|---------|
| `init_file` | File that `load_22` loads instead of 22. |
| `sysclk_adjust` | System clock adjustment, −2 to 2 in steps of 1/224. |
| `picture_replace_icl` | ICL whose images replace the backgrounds with the same ID. |
| `sd_folder` | Renames the SD download folder: up to 8 letters, digits, `_` and `-`. The display keeps the name for good; the build still writes `DWIN_SET`. |
| `backlight_delay` | Delay before the backlight comes on at power-on, in 10 ms units. |
| `[config.lcd]` | LCD timing, named as in DWIN's guide: `pclk_phs`, `pclk_div`, `h_w`, `h_s`, `h_d` (width), `h_e`, `v_w`, `v_s`, `v_d` (height), `v_e`, all required, and `tcon_sel` (`0`). Copy them from the display's own CFG file. |
| `[config.touch_panel]` | `mode` and `sense`, required, and `freq` (`0`). |
| `[config.clock_out]` | Clock output on P3.0 at 825.7536 MHz / `div` (required); `enabled` (`true`). |
| `[config.buzzer]` | `div1` (`0x6E`), `div2` (`0x0BB8`), `duty` (`0x00F0`) and `time` (`10`, in 10 ms units). The defaults are the factory 2.5 kHz beep. |

## Controls files

A controls file is a list of `[[control]]` tables, each picked by its
`type`. They are in drawing order: later display controls draw over earlier
ones. Every control takes an optional `name`, used in check messages.

Touch controls (`button`, `key`, `input`) also take:

| Key | Default | Meaning |
|-----|---------|---------|
| `area` | required | `{ x, y, w, h }`: where touches count. |
| `next_page` | none | Page to switch to on release. |
| `pressed_page` | none | Page whose background shows inside `area` while it is pressed. Draw the pressed look of a page's buttons on one more background image, and point here. |

Display controls (`icon`, `data`, `text`, `progress`) also take `sp`, a
description pointer: the display keeps the control's settings at that VP,
where the firmware can change them at runtime. `icon`, `data` and `text`
also take `popup_menu` (`false`).

### `button`

Writes `value` to `vp` when pressed (DGUS: "Return key code").

| Key | Default | Meaning |
|-----|---------|---------|
| `vp` | required | |
| `value` | required | |
| `target` | `"word"` | Part of the VP written: `"word"`, `"high_byte"`, `"low_byte"` or `{ bit = 0..15 }`. |
| `hold_time` | `0` | In 0.1 s units. |
| `upload` | `true` | Report the write over the UART. |

### `input`

Pressing `area` starts typing a number, edited with `key` controls; enter
writes it to `vp` (DGUS: "Data input").

```toml
[[control]]
type = "input"
area = { x = 100, y = 100, w = 200, h = 40 }
vp = 0x1030
position = { x = 290, y = 108 }
integer_digits = 3
decimal_digits = 1
limits = { min = 0, max = 1000 }
keyboard = { page = 20, area = { x = 0, y = 0, w = 300, h = 200 }, position = { x = 400, y = 200 } }
```

| Key | Default | Meaning |
|-----|---------|---------|
| `vp` | required | VP the number is written to. |
| `position` | required | `{ x, y }` of the cursor. The number is drawn leftward from it, so put it at the right of the field. |
| `data_type` | `"i16"` | `"i16"`, `"i32"`, `"i64"`, `"float"`, `"high_byte"` or `"low_byte"`. |
| `integer_digits` | `8` | |
| `decimal_digits` | `0` | |
| `limits` | none | `{ min, max }`; numbers outside are rejected. The bounds ignore the decimal point: with 1 decimal digit, `1000` means 100.0. |
| `keyboard` | none | `{ page, area, position }`: while typing, `area` of page `page` is drawn at `position`, and that page's `key` controls inside `area` work. Without it, the keys are the `key` controls of this page. |
| `color` | `"#000000"` | Colour of the typed number. |
| `font` | `0` | Font ID. |
| `font_size` | `16` | Character width in pixels. |
| `cursor_color` | `"black"` | `"black"` or `"white"`. |
| `masked` | `false` | Show `*` instead of the typed digits. |
| `load_vp` | none | A VP set to `load_value` while typing. |
| `load_value` | `0` | |
| `keyboard_transparency` | `0` | 0–255. |
| `upload` | `true` | Report the write over the UART. |

### `key`

A keyboard key: sends `code` to the `input` being edited (DGUS: "Basic
touch").

| Key | Default | Meaning |
|-----|---------|---------|
| `code` | required | `0x30`–`0x39` digits, `0x2E` decimal point, `0x2D` sign, `0xF0` cancel, `0xF1` enter, `0xF2` backspace. |

### `icon`

Shows image `icon_min + (value − val_min)` of ICL `icon_lib`, while the VP
value is between `val_min` and `val_max` (DGUS: "Variable icon").

| Key | Default | Meaning |
|-----|---------|---------|
| `vp` | required | |
| `position` | required | `{ x, y }` of the image's top left corner. |
| `icon_lib` | `0` | ICL ID. `0` is the active background ICL. |
| `val_min` | `0` | Value that shows `icon_min`. |
| `val_max` | required | Value that shows `icon_max`. |
| `icon_min`, `icon_max` | required | Image IDs. |
| `show_background` | `false` | Draw the image's background instead of keying it out. |
| `layer_mode` | `"overlay"` | `"overlay"`, `"overlay_mode1"` or `"overlay_mode2"`. |
| `icon_brightness`, `background_brightness` | `0` | 0–255. |
| `filter` | `0x3F` | How strongly the background is keyed out. |

With `icon_lib = 0`, images come from the background ICL in use, which
system variable 0xDE can switch. Keep images with text under the same IDs
in each language's background ICL, and they switch with the pages.

### `data`

Shows the VP as a number (DGUS: "Data variable").

| Key | Default | Meaning |
|-----|---------|---------|
| `vp` | required | |
| `position` | required | `{ x, y }` |
| `data_type` | `"i16"` | `"i16"`, `"u16"`, `"i32"`, `"u32"`, `"i64"`, `"f32"`, `"f64"`, `"high_byte"` or `"low_byte"`. |
| `integer_digits` | `2` | |
| `decimal_digits` | `0` | Places the decimal point: with 1, the value 235 shows 23.5. |
| `align` | `"left"` | `"left"`, `"right"` or `"center"`, around `position`. |
| `leading_zeros` | `false` | Draw leading zeros. |
| `unit` | `""` | Drawn after the number; ASCII, at most 11 bytes. |
| `color` | `"#000000"` | |
| `font` | `0` | Font ID. |
| `font_size` | `8` | Character width in pixels. |
| `auto_spacing` | `false` | Adjust character spacing automatically. |

### `text`

Shows text stored from `vp` on, inside `area` (DGUS: "Text display").

| Key | Default | Meaning |
|-----|---------|---------|
| `vp` | required | First VP of the text. |
| `area` | required | `{ x, y, w, h }` |
| `length` | `2` | Most bytes shown. |
| `encoding` | `"ascii"` | `"ascii"`, `"gb2312"`, `"gbk"`, `"big5"`, `"shift_jis"` or `"unicode"` (UTF-16 big-endian). |
| `font` | `0` | Font ID. |
| `ascii_font` | `0` | Font for ASCII characters in the multi-byte encodings. |
| `font_width`, `font_height` | `8` | Character size in pixels. |
| `color` | `"#000000"` | |
| `align` | `"left"` | `"left"`, `"right"`, `"center"` or `"right_to_left"`. |
| `vertical_align` | `"top"` | `"top"` or `"center"`. |
| `auto_spacing` | `false` | Adjust character spacing automatically. |
| `char_spacing`, `line_spacing` | `0` | In pixels. |

### `progress`

Fills `area` in proportion to the VP value between `val_min` and `val_max`
(DGUS: "Progress bar").

| Key | Default | Meaning |
|-----|---------|---------|
| `vp` | required | |
| `area` | required | `{ x, y, w, h }` |
| `val_min` | `0` | Value of an empty bar, signed 16-bit. |
| `val_max` | required | Value of a full bar. |
| `data_type` | `"i16"` | `"i16"`, `"high_byte"` or `"low_byte"`. |
| `direction` | `"right"` | Direction the bar grows: `"right"`, `"left"`, `"up"` or `"down"`. |
| `bar_color` | `"#000000"` | The filled part. |
| `background_color` | `"#000000"` | The empty part, drawn when `fill_background` is set. |
| `border_color` | `"#000000"` | Drawn when `show_border` is set. |
| `fill_background`, `show_border` | `false` | |
| `rate_vp` | none | DGUS: "rate addr"; setting it turns on "show rate". |

## Planning VPs

Each VP is one 16-bit word, and many values take more than one:

| Value | VPs |
|-------|-----|
| `i16`, `u16`, `high_byte`, `low_byte` | 1 |
| `i32`, `u32`, `f32`, input `float` | 2 |
| `i64`, `f64` | 4 |
| `text` | `length` / 2, rounded up |
| `sp` | 10 for `icon`, 13 for `data`, `text` and `progress` |

The check does not catch overlapping VPs, so give each value its own range.
DWIN suggests VPs in 0x1000–0x7FFF and description pointers in
0x8000–0xFFFF. Controls may share a VP on purpose, as the button and the
icon do in the small project above.

## What `degus check` reports

`degus build` runs the same check first. Errors stop the build; warnings
do not.

Errors:

- TOML that does not parse, unknown keys, missing required keys, and files
  that are not there;
- IDs listed twice: images in an ICL, clips in a WAE, pages in `controls`,
  and file numbers (13 and 14 are taken);
- a `background_icl` that is not a declared `[[icl]]`;
- `[config]` values out of range, and a `baud` the UART cannot reach;
- copied files named like a generated file, or a second `T5LCFG*.CFG`;
- page IDs of 4092 and up, and more than 255 display controls on a page;
- controls outside the screen, and empty areas;
- `icon` images missing from their ICL (with `icon_lib = 0`, checked only
  when `background_icl` is set);
- `min` above `max` in `val_min`/`val_max`, `icon_min`/`icon_max` and
  `limits`;
- key codes starting with `FE` or `FD`, bits above 15, and units that are
  not ASCII or too long;
- at build time, images too large for the chip, and files that run into the
  next file number's flash.

Warnings:

- pages without a background image, when `background_icl` is set;
- `next_page` and `pressed_page` that point nowhere;
- inputs whose keyboard page has no `key` controls;
- `icon` value and image ranges of different lengths;
- a `music_wae` that is not a declared `[[wae]]`, and `load_22` without the
  file it loads in `copy`;
- a `[config.lcd]` whose size, turned by `rotation`, is not `screen`;
- `sd_folder`, which renames the download folder for good.

Not checked: overlapping VPs, background image sizes, font files, and
whether numbers and text fit where they are drawn.
