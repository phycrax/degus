use std::assert_matches;

use degus_font::{Bitmap, Error, FONT0_LEN, Font0};

/// Where the spec puts the glyph of `code` at `width`.
fn offset(width: u8, code: u8) -> usize {
    let cell = |w: usize| 2 * w * w.div_ceil(8);
    (4..usize::from(width))
        .map(|w| 128 * cell(w))
        .sum::<usize>()
        + usize::from(code) * cell(usize::from(width))
}

/// A glyph that differs for every width and code: a diagonal starting at a
/// column picked by the code, and the code's bits along the top row.
fn pattern(width: u8, code: u8) -> Bitmap {
    let w = u16::from(width);
    Bitmap::from_fn(w, 2 * w, |x, y| {
        (x + u16::from(code)) % w == y % w || (y == 0 && x < 7 && code >> x & 1 == 1)
    })
}

fn patterned() -> Font0 {
    let mut font = Font0::new();
    for width in 4..=64 {
        for code in 0..=127 {
            font.set(width, code, pattern(width, code)).unwrap();
        }
    }
    font
}

#[test]
fn a_blank_font_0_is_all_zeros() {
    let bytes = Font0::new().to_bytes();
    assert_eq!(bytes.len(), FONT0_LEN);
    assert!(bytes.iter().all(|&b| b == 0));
}

#[test]
fn glyphs_sit_where_the_spec_puts_them() {
    let bytes = patterned().to_bytes();
    assert_eq!(bytes.len(), 3_082_752);
    for (width, code) in [
        (4, 0),
        (4, 127),
        (5, 0),
        (8, 65),
        (9, 1),
        (12, 103),
        (63, 64),
        (64, 127),
    ] {
        let glyph = pattern(width, code);
        let at = offset(width, code);
        assert_eq!(
            &bytes[at..at + glyph.as_bytes().len()],
            glyph.as_bytes(),
            "width {width}, code {code}"
        );
    }
    // The last glyph ends the file.
    assert_eq!(offset(64, 127) + 2 * 64 * 8, FONT0_LEN);
}

#[test]
fn rows_are_most_significant_bit_first_and_padded() {
    let mut glyph = Bitmap::new(12, 24);
    glyph.set(0, 0, true);
    glyph.set(11, 0, true);
    glyph.set(8, 1, true);
    assert_eq!(&glyph.as_bytes()[..4], &[0x80, 0x10, 0x00, 0x80]);
    assert_eq!(glyph.as_bytes().len(), 2 * 24);
}

#[test]
fn files_read_back_as_written() {
    let font = patterned();
    let bytes = font.to_bytes();
    let read = Font0::from_bytes(&bytes).unwrap();
    assert_eq!(read, font);
    assert_eq!(read.to_bytes(), bytes);
    assert_eq!(read.glyph(12, 103), Some(&pattern(12, 103)));
    assert_eq!(format!("{read:?}"), "Font0 { glyphs: 7808 drawn }");
}

#[test]
fn bits_past_a_glyph_s_width_are_ignored() {
    let mut bytes = Font0::new().to_bytes();
    // Width 4: one byte a row, of which the low 4 bits are past the width.
    bytes[0] = 0x9F;
    let font = Font0::from_bytes(&bytes).unwrap();
    let glyph = font.glyph(4, 0).unwrap();
    assert_eq!(glyph.as_bytes()[0], 0x90);
    assert!(glyph.get(0, 0) && !glyph.get(1, 0) && glyph.get(3, 0));
    assert!(!glyph.get(4, 0), "pixels outside the glyph are never set");
}

#[test]
fn glyphs_are_listed_by_width_then_code() {
    let font = Font0::new();
    let mut glyphs = font.glyphs();
    let (w, code, glyph) = glyphs.next().unwrap();
    assert_eq!((w, code, glyph.width(), glyph.height()), (4, 0, 4, 8));
    let (w, code, glyph) = glyphs.last().unwrap();
    assert_eq!((w, code, glyph.width(), glyph.height()), (64, 127, 64, 128));
    assert_eq!(font.glyphs().count(), 61 * 128);
}

#[test]
fn bad_files_widths_codes_and_sizes_are_errors() {
    assert_matches!(
        Font0::from_bytes(&vec![0; FONT0_LEN - 1]),
        Err(Error::Font0Length(len)) if len == FONT0_LEN - 1
    );
    let mut font = Font0::new();
    assert_matches!(font.set(3, 0, Bitmap::new(3, 6)), Err(Error::Font0Width(3)));
    assert_matches!(
        font.set(65, 0, Bitmap::new(65, 130)),
        Err(Error::Font0Width(65))
    );
    assert_matches!(
        font.set(8, 128, Bitmap::new(8, 16)),
        Err(Error::Font0Code(128))
    );
    assert_matches!(
        font.set(8, 65, Bitmap::new(8, 15)),
        Err(Error::GlyphSize {
            width: 8,
            got_width: 8,
            got_height: 15
        })
    );
    assert_eq!(font.glyph(3, 0), None);
    assert_eq!(font.glyph(8, 128), None);
    let error = font.set(8, 65, Bitmap::new(9, 16)).unwrap_err();
    assert_eq!(
        error.to_string(),
        "font 0's glyphs of width 8 are 8x16, not 9x16"
    );
}

#[test]
fn bitmaps_check_their_rows() {
    assert_matches!(
        Bitmap::from_rows(12, 2, &[0; 3]),
        Err(Error::Rows {
            width: 12,
            height: 2,
            expected: 4,
            len: 3
        })
    );
    let bitmap = Bitmap::from_rows(3, 2, &[0xA0, 0x40]).unwrap();
    assert_eq!(format!("{bitmap:#?}"), "Bitmap(3x2)\n#.#\n.#.");
    assert_eq!(format!("{bitmap:?}"), "Bitmap(3x2)");
    assert!(!bitmap.is_blank());
    assert!(Bitmap::new(0, 0).is_blank());
}

#[test]
#[should_panic(expected = "outside a 4x8 bitmap")]
fn setting_a_pixel_outside_panics() {
    Bitmap::new(4, 8).set(4, 0, true);
}
