#![cfg(feature = "render")]

#[path = "../../glyph/tests/support/ttf.rs"]
mod ttf;

use degus_font::glyph::Face;
use degus_font::{Bitmap, Font0, Font0Options};
use ttf::{TestFont, glyph, rect};

/// A monospaced font whose em is as tall as its line and twice its
/// advance, so that it fits font 0's glyphs exactly at 2 px per pixel of
/// width.
fn face() -> Face {
    let font = TestFont {
        units_per_em: 1000,
        ascent: 800,
        descent: -200,
        monospace: true,
        glyphs: vec![
            glyph('0', 500, vec![rect(100, 0, 400, 700)]),
            glyph('g', 500, vec![rect(100, -200, 400, 500)]),
            glyph('|', 500, vec![rect(150, -200, 350, 800)]),
            glyph(' ', 500, vec![]),
            glyph('\u{1}', 500, vec![rect(0, 0, 500, 500)]),
        ],
    };
    Face::new(font.build()).unwrap()
}

fn unhinted() -> Font0Options {
    Font0Options {
        hinting: false,
        ..Font0Options::default()
    }
}

/// Columns `x0..x1` of rows `y0..y1` set, in a `width` × 2 `width` glyph.
fn block(width: u16, (x0, x1): (u16, u16), (y0, y1): (u16, u16)) -> Bitmap {
    Bitmap::from_fn(width, 2 * width, |x, y| {
        (x0..x1).contains(&x) && (y0..y1).contains(&y)
    })
}

#[test]
fn glyphs_are_scaled_to_fit_and_share_a_baseline() {
    let font = Font0::render(&face(), &unhinted()).unwrap();
    // Width 10: 20 px per em, so the line fills the 20 rows exactly and the
    // baseline is under row 15. A unit is 1/50 px.
    let zero = font.glyph(10, b'0').unwrap();
    assert_eq!(zero, &block(10, (2, 8), (2, 16)), "{zero:#?}");
    let g = font.glyph(10, b'g').unwrap();
    assert_eq!(g, &block(10, (2, 8), (6, 20)), "{g:#?}");
    let bar = font.glyph(10, b'|').unwrap();
    assert_eq!(bar, &block(10, (3, 7), (0, 20)), "{bar:#?}");
    // Width 4: 8 px per em, so '0' spans x 0.8 to 3.2 and y 0 to 5.6. Only
    // columns 1 and 2 are more than half covered, and its top row is 60 %.
    let zero = font.glyph(4, b'0').unwrap();
    assert_eq!(zero, &block(4, (1, 3), (0, 6)), "{zero:#?}");
}

#[test]
fn spaces_control_codes_and_missing_characters_stay_blank() {
    let font = Font0::render(&face(), &unhinted()).unwrap();
    for width in [4, 10, 64] {
        assert!(font.glyph(width, b' ').unwrap().is_blank());
        assert!(
            font.glyph(width, 1).unwrap().is_blank(),
            "control code 1 is drawn"
        );
        assert!(font.glyph(width, b'A').unwrap().is_blank());
        assert!(font.glyph(width, 127).unwrap().is_blank());
    }
}

#[test]
fn a_set_scale_overrides_the_fit() {
    let options = Font0Options {
        scale: Some(1.0),
        ..unhinted()
    };
    let font = Font0::render(&face(), &options).unwrap();
    // Width 10 at 10 px per em: the 10-row line sits in rows 5 to 14 and
    // the advance of 5 px is centred.
    let zero = font.glyph(10, b'0').unwrap();
    assert_eq!(zero, &block(10, (4, 7), (6, 13)), "{zero:#?}");
}

#[test]
fn the_threshold_decides_partly_covered_pixels() {
    // At 10 px per em, '|' spans x 1.5 to 3.5 of the 5 px advance, centred
    // in 10 columns: columns 4 and 6 are half covered.
    let render = |threshold| {
        let options = Font0Options {
            scale: Some(1.0),
            threshold,
            ..unhinted()
        };
        Font0::render(&face(), &options).unwrap()
    };
    let columns = |font: &Font0| {
        let bar = font.glyph(10, b'|').unwrap();
        (0..10).filter(|&x| bar.get(x, 10)).collect::<Vec<_>>()
    };
    assert_eq!(columns(&render(1)), [4, 5, 6]);
    assert_eq!(columns(&render(255)), [5]);
    assert_eq!(columns(&render(0)), [4, 5, 6], "0 counts as 1");
}

#[test]
fn hinted_fonts_draw_at_every_width() {
    let font = Font0::render(&face(), &Font0Options::default()).unwrap();
    for width in 4..=64 {
        let zero = font.glyph(width, b'0').unwrap();
        assert!(!zero.is_blank(), "width {width}");
        assert!(font.glyph(width, b' ').unwrap().is_blank());
    }
}
