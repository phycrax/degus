//! Text displays: the text in VPs, laid out in the text box as DWIN's
//! guide describes (§7.13).

use degus_set::controls::{Encoding, TextAlign, TextVar, VerticalAlign};
use degus_set::font::Font0;
use degus_set::icl::image::RgbImage;

use crate::draw::{Rect, rgb565};
use crate::font::Mark;
use crate::vp::VpSpace;

/// A character code as the text stores it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Code {
    /// A one-byte code: ASCII, or a single-byte code of an encoding.
    One(u8),
    /// A two-byte code of GB2312, GBK, BIG5 or SJIS, or a UTF-16 unit.
    Two(u16),
}

impl Code {
    fn value(self) -> u16 {
        match self {
            Code::One(b) => b.into(),
            Code::Two(c) => c,
        }
    }
}

/// The text's bytes: at most `length`, ending before the end mark.
///
/// The guide's examples end text with `FF FF` after an odd number of bytes
/// as well as after an even one, so the marks are found byte by byte. In
/// UTF-16 they are whole units.
// ASSUME: `00 00` ends text anywhere `FF FF` does.
pub(crate) fn text_bytes(vp: &VpSpace, text: &TextVar) -> Vec<u8> {
    let mut bytes = vp.bytes(text.vp, text.length.into());
    let is_mark = |pair: &[u8]| pair == [0x00, 0x00] || pair == [0xFF, 0xFF];
    let end = if text.encoding == Encoding::Unicode {
        bytes.chunks(2).position(is_mark).map(|i| 2 * i)
    } else {
        bytes.windows(2).position(is_mark)
    };
    if let Some(end) = end {
        bytes.truncate(end);
    }
    bytes
}

/// The codes of `bytes` in `encoding`. A two-byte code cut short by the
/// end is dropped.
fn codes(bytes: &[u8], encoding: Encoding) -> Vec<Code> {
    let mut codes = Vec::with_capacity(bytes.len());
    match encoding {
        Encoding::Ascii => codes.extend(bytes.iter().map(|&b| Code::One(b))),
        Encoding::Unicode => {
            let (units, _) = bytes.as_chunks::<2>();
            codes.extend(
                units
                    .iter()
                    .map(|&unit| Code::Two(u16::from_be_bytes(unit))),
            );
        }
        Encoding::Gb2312 | Encoding::Gbk | Encoding::Big5 | Encoding::ShiftJis => {
            let mut rest = bytes;
            while let Some((&lead, tail)) = rest.split_first() {
                // SJIS's half-width katakana take one byte.
                let single = lead < 0x80
                    || (encoding == Encoding::ShiftJis && (0xA1..=0xDF).contains(&lead));
                if single {
                    codes.push(Code::One(lead));
                    rest = tail;
                } else if let Some((&trail, tail)) = tail.split_first() {
                    codes.push(Code::Two(u16::from_be_bytes([lead, trail])));
                    rest = tail;
                } else {
                    break;
                }
            }
        }
    }
    codes
}

/// What `code` draws as. In the two-byte encodings, one-byte codes take
/// `ascii_font` at half the width (§7.13.1); everything else takes `font`
/// at `font_width`.
fn mark<'a>(code: Code, text: &TextVar, font0: Option<&'a Font0>) -> Mark<'a> {
    let h = u16::from(text.font_height);
    match (text.encoding, code) {
        (Encoding::Ascii | Encoding::Unicode, _) => {
            Mark::new(font0, text.font, code.value().into(), text.font_width, h)
        }
        (_, Code::One(b)) => Mark::new(font0, text.ascii_font, b.into(), text.font_width / 2, h),
        // Never a font 0 code: two-byte codes start at 0x81.
        (_, Code::Two(c)) => Mark::new(font0, text.font, c.into(), text.font_width, h),
    }
}

/// A character placed on a line.
struct Placed<'a> {
    mark: Mark<'a>,
    /// The first column drawn.
    from: u16,
    /// Pixels the character itself takes.
    width: i32,
    /// Pixels to the next character.
    advance: i32,
}

/// Draws a text display from the VPs.
pub(crate) fn draw_text(fb: &mut RgbImage, vp: &VpSpace, text: &TextVar, font0: Option<&Font0>) {
    let area = Rect::from_area(text.area);
    let color = rgb565(text.color);
    let line_h = i32::from(text.font_height);
    let fixed = text.fixed_width;

    // Break into lines at line feeds and at the box's right edge.
    let mut lines: Vec<Vec<Placed>> = vec![Vec::new()];
    let mut x = 0;
    for code in codes(&text_bytes(vp, text), text.encoding) {
        match code.value() {
            0x0D => continue,
            0x0A => {
                lines.push(Vec::new());
                x = 0;
                continue;
            }
            _ => {}
        }
        let mark = mark(code, text, font0);
        let cell = i32::from(mark.width());
        // ASSUME: with fixed widths the spacing is not added, as DWIN's
        // editor previews it; with adjusted spacing a character takes its
        // inked columns, or its whole cell when it draws nothing.
        let placed = match (fixed, mark.ink()) {
            (true, _) => Placed {
                mark,
                from: 0,
                width: cell,
                advance: cell,
            },
            (false, Some((start, end))) => Placed {
                mark,
                from: start,
                width: i32::from(end - start),
                advance: i32::from(end - start) + i32::from(text.char_spacing),
            },
            (false, None) => Placed {
                mark,
                from: 0,
                width: cell,
                advance: cell + i32::from(text.char_spacing),
            },
        };
        let line = lines.last_mut().expect("there is always a line");
        if !line.is_empty() && x + placed.width > area.w {
            lines.push(Vec::new());
            x = 0;
        }
        x += placed.advance;
        lines
            .last_mut()
            .expect("there is always a line")
            .push(placed);
    }
    let line_width = |line: &[Placed]| -> i32 {
        match line.split_last() {
            Some((last, rest)) => rest.iter().map(|p| p.advance).sum::<i32>() + last.width,
            None => 0,
        }
    };

    // Alignment applies to a single line only (§7.13.1).
    let single = lines.len() == 1;
    let mut y = area.y;
    if single && text.vertical_align == VerticalAlign::Center {
        y += (area.h - line_h).max(0) / 2;
    }
    for line in &lines {
        if y + line_h > area.bottom() {
            break;
        }
        let free = (area.w - line_width(line)).max(0);
        // ASSUME: right to left draws like right alignment.
        let mut x = area.x
            + match text.align {
                _ if !single => 0,
                TextAlign::Left => 0,
                TextAlign::Right | TextAlign::RightToLeft => free,
                TextAlign::Center => free / 2,
            };
        for placed in line {
            placed.mark.draw(fb, x, y, placed.from, area, color);
            x += placed.advance;
        }
        y += line_h + i32::from(text.line_spacing);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(encoding: Encoding, length: u16) -> TextVar {
        TextVar {
            vp: 0x5000,
            length,
            encoding,
            ..TextVar::default()
        }
    }

    #[test]
    fn stops_at_the_end_marks_or_the_length() {
        let mut vp = VpSpace::new();
        vp.write_bytes(0x5000, b"North 12AB").unwrap();
        let mut ascii = text(Encoding::Ascii, 32);
        assert_eq!(text_bytes(&vp, &ascii), b"North 12AB");
        ascii.length = 5;
        assert_eq!(text_bytes(&vp, &ascii), b"North");
        // The guide's `45FF FFFF` after "ABCD": the mark starts mid-word.
        vp.write_bytes(0x5000, &[0x41, 0x42, 0x43, 0x44, 0x45, 0xFF, 0xFF, 0xFF])
            .unwrap();
        ascii.length = 32;
        assert_eq!(text_bytes(&vp, &ascii), b"ABCDE");
        // UTF-16 units end at a whole unit only.
        vp.write(0x5000, &[0x4100, 0x0042, 0x0000]).unwrap();
        assert_eq!(
            text_bytes(&vp, &text(Encoding::Unicode, 32)),
            [0x41, 0, 0, 0x42]
        );
    }

    #[test]
    fn splits_codes_by_encoding() {
        let bytes = [0x41, 0xD6, 0xD0, 0x42, 0xB1];
        assert_eq!(
            codes(&bytes, Encoding::Gbk),
            [Code::One(0x41), Code::Two(0xD6D0), Code::One(0x42)]
        );
        assert_eq!(
            codes(&[0xB1, 0x41, 0x83, 0x41], Encoding::ShiftJis),
            [Code::One(0xB1), Code::One(0x41), Code::Two(0x8341)]
        );
        assert_eq!(
            codes(&[0x00, 0x41, 0x4E, 0x2D], Encoding::Unicode),
            [Code::Two(0x41), Code::Two(0x4E2D)]
        );
        assert_eq!(codes(&[0xD6, 0xD0], Encoding::Ascii).len(), 2);
    }
}
