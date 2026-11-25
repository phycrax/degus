# What the simulator assumes

The simulator follows DWIN's *T5L DGUSII Application Development Guide*
V2.921 where it says how the display behaves. Where it doesn't, the code
picks a behaviour and marks it `ASSUME:`, taking it from DWIN's editor,
whose preview draws pages too, where the preview shows one. This list
collects them, to check on a panel. Once a behaviour is confirmed, move it
to the guide-backed list and drop its marker.

## To check on a device

Touch:

- A return key code writes its value on press, as DWIN's editor previews
  it. With a hold time, it writes once the touch has been held that long
  instead. (`touch.rs`)
- A touch control switches page when the finger lifts, wherever it lifts.
  (`touch.rs`)
- Where touch areas overlap, the control listed first wins, as DWIN's
  editor previews it. (`touch.rs`)
- While a popup keyboard is up, only its keys respond. Touches elsewhere
  are ignored. (`touch.rs`)
- Keys of a popup keyboard switch no page. (`touch.rs`)
- Only touches on a touch control sound, not touches on bare screen.
  (`touch.rs`)

Data input:

- An input in progress survives a page switch, so keys on a page reached
  through the input's `next_page` still type into it. (`sim.rs`)
- `load_vp` gets `load_value` when input starts, and its old value back
  when input ends, by enter or cancel. (`input.rs`)
- Enter with nothing typed ends input like cancel. (`input.rs`)
- A number that fits the limits but not the data type is rejected.
  (`number.rs`)
- A float input writes the number itself, not the fixed-point integer the
  limits use. (`number.rs`)
- The sign key works only for signed types: not the high or low byte.
  (`input.rs`)
- The cursor is 2 pixels wide and doesn't blink. (`render.rs`)

Display controls:

- A variable icon shows icon `icon_min + (value - val_min)`, up to
  `icon_max`, as DWIN's editor previews it. The guide says only that the
  values map linearly onto the icons. (`render.rs`)
- In transparency mode, the icon's background colour is keyed out: the
  colour at its pixel (1, 1), within 30 in every channel, as DWIN's editor
  previews it. The filter strength is ignored. (`render.rs`)
- In overlay mode 2, the icon is mixed onto the page as
  `icon × icon_brightness / 256 + page × background_brightness / 256`.
  Overlay mode 1 draws like overlay mode 0. (`render.rs`)
- A data variable aligns its number and unit in a field of
  `integer_digits` + `decimal_digits` (+ 1 for the point) + the unit's
  length characters from `position` on, as DWIN's editor previews it.
  (`render.rs`)
- A data variable shows only the lowest `integer_digits` digits of the
  integer part. A float that is not finite shows `-`. (`number.rs`)
- Font 0 glyphs draw at their own size, *w* × 2*w*, whatever height the
  control gives. (`font.rs`)
- Text ends at `00 00` wherever it would end at `FF FF`. (`text.rs`)
- Text with fixed character widths adds no character spacing, as DWIN's
  editor previews it. With adjusted spacing, each character takes its
  inked columns plus the spacing, and one that draws nothing takes its
  whole cell. (`text.rs`)
- Right-to-left text draws like right-aligned text. (`text.rs`)

Sound:

- In music mode, a new clip cuts off the one playing: one channel.
  (`system.rs`)
- The display clears `0x00A0`'s count byte in both modes, once it has
  started the clip or the beep. (`system.rs`)
- A volume above `0x40` plays at 100 %. (`system.rs`)
- `0x00A1`'s low byte reads 2 from the start of a clip until its length has
  passed, then 0. It never reads 1 (paused).

System variables:

- `0x00DE`/`0x00DF` keep the background ICL switched while `0x00DE`'s high
  byte stays `5A`, and `0x00DF` holds the whole ICL number. (`system.rs`)
- `0x000E`–`0x000F` read `0000 6322`, the guide's example version.
- Writes to read-only system VPs are undone.

UART frames:

- A frame the display cannot use, such as one with a wrong CRC, a length
  too short or an unknown command, gets no answer. (`uart.rs`)
- Bytes after a read's length are ignored. (`uart.rs`, `degus-codec`)
- A read of more words than one frame holds, 125 or 124 with CRC, gets no
  answer. (`uart.rs`)

## Backed by the guide

- Values outside `val_min`–`val_max` show no icon (§7.2).
- Text ends at `0xFFFF` or `0x0000`, after its length, or at the end of
  the box, and `FF FF` may start mid-word. CR LF breaks a line. Alignment
  applies to a single line only. In the two-byte encodings, ASCII takes
  the ASCII font at half the width; in the one-byte encoding and Unicode,
  every character takes the other font. Bit 7 of the encoding byte fixes
  each character to its cell when set, and adjusts the spacing when clear
  (§7.13).
- A data input draws right-aligned, `position` being the top right of the
  cursor. A number outside the limits is dropped like a cancel (§6.2).
- `0x0084` `5A 01 <page>` switches page unless the page has no image, and
  the `5A` is cleared after (§5.1).
- `0x0016` reports touches as `5A` + state (1 press, 2 lift, 3 pressing),
  X and Y (§5.1).
- `0x00A0` in music mode: clip ID, `01` to play, volume in 1/64 steps
  starting at `0x40`, status, and `5A` + a WAE number to play from instead
  of the configuration's. In buzzer mode the count byte is the beep length
  in 8 ms units (§3.3.2.5 and §5.1).
- The buzzer's tone is 825.7536 MHz / (`div1` × `div2`) at `duty` / `div2`,
  and a touch beeps for `time` × 10 ms (CFG `0x28`–`0x2D`).
- A progress bar writes its percentage, 0–100, to its rate VP (§7.23).
- With CFG `0x05` bit 5 set, the VPs start as file 22 holds them, VP *n*
  at byte 2*n* (§3.3.2.4).
- Frames are `5A A5`, a length byte counting the command, data and CRC,
  the command and data, and a CRC-16/MODBUS trailer, low byte first, when
  CFG `0x05` bit 7 is set. `0x82` writes are answered `82 4F 4B`, `0x83`
  reads with `83`, the address, the word count and the words, and
  auto-uploads use the same `83` frame (§4.1, §4.2, and `0x000C`'s
  examples in §5.1).

## Not simulated

- Fonts other than font 0: their characters draw as boxes.
- The font 0 a display comes with: when the set has no font 0, font 0
  text is not drawn.
- Display and touch controls other than those `degus-controls` models.
  They are reported when their page is shown; an unknown touch control's
  page switch and press effect still work.
- Voice IDs: music-mode touch sounds play the control's voice ID, which
  `degus-controls` doesn't read yet, so they are silent.
- Pausing a clip, standby dimming, rotation, and description pointers
  (`sp`).
- The register (`0x80`, `0x81`), curve buffer (`0x84`) and double-word
  (`0x86`, `0x87`) commands: they are reported and get no answer.
- `0x000C`'s baud rate and CRC switches, and setting the RTC through
  `0x009C`: the writes are only stored.
