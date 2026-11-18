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

System variables:

- `0x00DE`/`0x00DF` keep the background ICL switched while `0x00DE`'s high
  byte stays `5A`, and `0x00DF` holds the whole ICL ID. (`system.rs`)
- The RTC (`0x0010`) reads the PC's clock, in UTC. (`system.rs`)
- `0x000E`–`0x000F` read `0000 6322`, the guide's example version.
- Writes to read-only system VPs are undone.

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
- A progress bar writes its percentage, 0–100, to its rate VP (§7.23).

## Not simulated

- Fonts: one stand-in bitmap font, scaled into each control's cell. Font
  IDs are ignored.
- JPEG artifacts, and anything else the ICL build or the panel's hardware
  changes, such as the JPEG header layout.
- Audio (`0x00A0`, touch sound), standby dimming, rotation, file 22, and
  description pointers (`sp`).
- DGUS frames on the UART (`5A A5 len cmd data [crc]`). Links speak a
  text protocol until a frame codec exists, planned as a device side in
  the `dguscard` crate.
