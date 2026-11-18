# What the simulator assumes

The simulator follows DWIN's *T5L DGUSII Application Development Guide*
V2.921 where it says how the display behaves. Where it doesn't, the code
picks a behaviour and marks it `ASSUME:`. This list collects them, to check
on a panel; once a behaviour is confirmed, move it to the guide-backed list
and drop its marker.

## To check on a device

Touch:

- A return key code (`button`) writes its value on press. With a
  `hold_time`, it writes once the touch has been held that long instead.
  (`touch.rs`)
- A touch control switches page when the finger lifts, wherever it lifts.
  (`touch.rs`)
- Where touch areas overlap, the control listed last, which is drawn on
  top, wins. (`touch.rs`)
- While a popup keyboard is up, only its keys respond. Touches elsewhere
  are ignored. (`touch.rs`)
- Keys of a popup keyboard switch no page. (`touch.rs`)

Data input:

- An input in progress survives a page switch, so keys on a page reached
  through the input's `next_page` still type into it. (`sim.rs`)
- `load_vp` gets `load_value` when input starts, and its old value back
  when input ends, by enter or cancel. (`input.rs`)
- Enter with nothing typed ends input like cancel.
- A number that fits the limits but not the data type is rejected.
  (`format.rs`)
- A `float` input writes the number itself, not the fixed-point integer
  the limits use. (`format.rs`)
- The sign key works only for signed types: not `high_byte` or `low_byte`.
- The cursor is 2 pixels wide.

Display controls:

- A variable icon maps values linearly onto its icons:
  `icon_min + (value - val_min) × (icon_max - icon_min) / (val_max - val_min)`.
  That is `icon_min + (value - val_min)` when both ranges are the same
  size. (`render.rs`)
- In transparency mode, a pixel is keyed out when every channel is below
  half the `filter` value (0x3F gives 31). Icons are flattened onto white
  like the build does, so only black backgrounds key out. (`render.rs`)
- In overlay mode 2, the icon is mixed onto the page as
  `icon × icon_brightness / 256 + page × background_brightness / 256`.
  Overlay mode 1 draws like overlay mode 0. (`render.rs`)
- A data variable aligns its number in a field of `integer_digits` +
  `decimal_digits` (+ 1 for the point) characters starting at `position`.
  The unit follows the number. (`render.rs`)
- A data variable shows only the lowest `integer_digits` digits of the
  integer part. A float that is not finite shows `-`. (`format.rs`)
- Text stops at a word-aligned `0x0000` or `0xFFFF`. (`text.rs`)
- UTF-16 text draws ASCII at half the font width, like the other
  multi-byte encodings. (`text.rs`)
- Text breaks lines at `\n` as well as at the box's edge.

Sound:

- In music mode, a new clip cuts off the one playing: one channel.
- The display clears `0x00A0`'s count byte in both modes, once it has
  started the clip or the beep.
- A volume above `0x40` plays at 100 %.
- `0x00A1`'s low byte reads 2 from the start of a clip until its length has
  passed, then 0. It never reads 1 (paused).
- Only touches on a touch control sound, not touches on bare screen.
- The buzzer sounds like a square wave at its frequency and duty, without
  DC. A narrower pulse sounds quieter.
- 8-bit (12:8) clips expand each code to the middle of its range. The spec
  documents only the encoder. (`degus-wae`)

System variables:

- `0x00DE`/`0x00DF` keep the background ICL switched while `0x00DE`'s high
  byte stays `5A`, and `0x00DF` holds the whole ICL ID. (`system.rs`)
- The RTC (`0x0010`) reads the PC's clock, in UTC. (`system.rs`)
- `0x000E`–`0x000F` read `0000 6322`, the guide's example version.
- Writes to read-only system VPs are undone.

UART frames:

- A frame the display cannot use, such as one with a wrong CRC, a length
  too short or an unknown command, gets no answer. (`frames.rs`)
- Bytes after a read's length are ignored. (`frames.rs`, `dguscard`)
- A read of more words than one frame holds, 125 or 124 with CRC, gets no
  answer. (`frames.rs`)

## Backed by the guide

- Values outside `val_min`–`val_max` show no icon (§7.2).
- Text stops at `0x0000`, `0xFFFF` or the end of the box. In the
  multi-byte encodings, ASCII takes half the font width (§7.13).
- A data input draws right-aligned, `position` being the top right of the
  cursor. A number outside the limits is dropped like a cancel (§6.2).
- `0x0084` `5A 01 <page>` switches page unless the page has no image, and
  the `5A` is cleared after (§5.1).
- `0x0016` reports touches as `5A` + state (1 press, 2 lift, 3 pressing),
  X and Y (§5.1).
- `0x00A0` in music mode: clip ID, `01` to play, volume in 1/64 steps
  starting at `0x40`, status, and `5A` + a WAE number to play from instead
  of the CFG's. In buzzer mode the count byte is the beep length in 8 ms
  units (§3.3.2.5 and §5.1).
- The buzzer's tone is 825.7536 MHz / (`div1` × `div2`) at `duty` / `div2`,
  and a touch beeps for `time` × 10 ms (CFG `0x28`–`0x2D`).
- A progress bar writes its percentage, 0–100, to its rate VP (§7.23).
- Frames are `5A A5`, a length byte counting the command, data and CRC,
  the command and data, and a CRC-16/MODBUS trailer, low byte first, when
  CFG `0x05` bit 7 is set. `0x82` writes are answered `82 4F 4B`, `0x83`
  reads with `83`, the address, the word count and the words, and
  auto-uploads use the same `83` frame (§4.1, §4.2, and `0x000C`'s
  examples in §5.1).

## Not simulated

- Fonts: one stand-in bitmap font, scaled into each control's cell. Font
  IDs are ignored.
- JPEG artifacts, and anything else the ICL build or the panel's hardware
  changes, such as the JPEG header layout.
- Voice IDs: music-mode touch sounds play the control's voice ID, which
  degus does not build, so they are silent.
- Pausing a clip, standby dimming, rotation, file 22, and description
  pointers (`sp`).
- The register (`0x80`, `0x81`), curve buffer (`0x84`) and double-word
  (`0x86`, `0x87`) commands: they are logged and get no answer.
- `0x000C`'s baud rate and CRC switches: links keep the configured ones.
