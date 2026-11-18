//! Text displays: the text in VPs, decoded and laid out in the text box.

use degus_controls::{Encoding, TextAlign, TextVar, VerticalAlign};
use image::RgbImage;

use crate::draw::{Rect, rgb565};
use crate::font::draw_char;
use crate::vp::VpSpace;

/// The text's bytes: at most `length`, ending before the first `0x0000` or
/// `0xFFFF` word.
pub fn text_bytes(vp: &VpSpace, text: &TextVar) -> Vec<u8> {
    let mut bytes = vp.bytes(text.vp, text.length.into());
    // ASSUME: the end marks are whole, word-aligned words.
    if let Some(end) = bytes
        .chunks(2)
        .position(|pair| pair == [0x00, 0x00] || pair == [0xFF, 0xFF])
    {
        bytes.truncate(end * 2);
    }
    bytes
}

/// Decodes `bytes`; NUL characters are dropped.
pub fn decode(bytes: &[u8], encoding: Encoding) -> String {
    let codec = match encoding {
        // "8bit encoding": one byte per character, taken as Latin-1.
        Encoding::Ascii => {
            return bytes
                .iter()
                .filter(|&&b| b != 0)
                .map(|&b| char::from(b))
                .collect();
        }
        Encoding::Gb2312 | Encoding::Gbk => encoding_rs::GBK,
        Encoding::Big5 => encoding_rs::BIG5,
        Encoding::ShiftJis => encoding_rs::SHIFT_JIS,
        Encoding::Unicode => encoding_rs::UTF_16BE,
    };
    let (text, _) = codec.decode_without_bom_handling(bytes);
    text.chars().filter(|&c| c != '\0').collect()
}

/// Width of `c`'s cell: in the multi-byte encodings ASCII takes half the
/// font width.
// ASSUME: UTF-16 text halves ASCII like the other multi-byte encodings.
fn char_width(c: char, text: &TextVar) -> u32 {
    let w = u32::from(text.font_width);
    if text.encoding != Encoding::Ascii && c.is_ascii() {
        w / 2
    } else {
        w
    }
}

/// Draws a text display from the VPs.
pub fn draw_text(fb: &mut RgbImage, vp: &VpSpace, text: &TextVar) {
    let string = decode(&text_bytes(vp, text), text.encoding);
    let area = Rect::from_area(text.area);
    let spacing = i32::from(text.char_spacing);
    let line_h = i32::from(text.font_height);
    let color = rgb565(text.color);

    // Break into lines at newlines and at the box's right edge.
    let mut lines: Vec<Vec<char>> = vec![Vec::new()];
    let mut width = 0;
    for c in string.chars() {
        match c {
            '\r' => continue,
            '\n' => {
                lines.push(Vec::new());
                width = 0;
                continue;
            }
            _ => {}
        }
        let w = char_width(c, text) as i32;
        let line = lines.last_mut().unwrap();
        if !line.is_empty() && width + w > area.w {
            lines.push(Vec::new());
            width = 0;
        }
        lines.last_mut().unwrap().push(c);
        width += w + spacing;
    }
    let line_width = |line: &[char]| -> i32 {
        let chars: i32 = line.iter().map(|&c| char_width(c, text) as i32).sum();
        chars + spacing * (line.len() as i32 - 1).max(0)
    };

    // Alignment applies to a single line only (the guide).
    let single = lines.len() == 1;
    let block_h =
        lines.len() as i32 * line_h + (lines.len() as i32 - 1) * i32::from(text.line_spacing);
    let mut y = area.y;
    if single && text.vertical_align == VerticalAlign::Center {
        y += (area.h - block_h).max(0) / 2;
    }
    for line in &lines {
        if y + line_h > area.y + area.h {
            break;
        }
        let free = (area.w - line_width(line)).max(0);
        let mut x = area.x
            + match text.align {
                _ if !single => 0,
                TextAlign::Left => 0,
                TextAlign::Right | TextAlign::RightToLeft => free,
                TextAlign::Center => free / 2,
            };
        for &c in line {
            let w = char_width(c, text);
            draw_char(fb, x, y, w, line_h as u32, c, color);
            x += w as i32 + spacing;
        }
        y += line_h + i32::from(text.line_spacing);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stops_at_the_end_marks_or_the_length() {
        let mut vp = VpSpace::new();
        vp.write_bytes(0x5000, b"North 12AB").unwrap();
        let mut text = TextVar {
            vp: 0x5000,
            length: 32,
            ..TextVar::default()
        };
        assert_eq!(text_bytes(&vp, &text), b"North 12AB");
        text.length = 5;
        assert_eq!(text_bytes(&vp, &text), b"North");
        vp.write(0x5001, &[0xFFFF]).unwrap();
        text.length = 32;
        assert_eq!(text_bytes(&vp, &text), b"No");
    }

    #[test]
    fn decodes_each_encoding() {
        assert_eq!(decode(b"AB\0", Encoding::Ascii), "AB");
        assert_eq!(decode(&[0x00, 0x41, 0x4E, 0x2D], Encoding::Unicode), "A中");
        assert_eq!(decode(&[0xD6, 0xD0, 0x41], Encoding::Gbk), "中A");
    }
}
