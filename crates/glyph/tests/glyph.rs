#[path = "support/ttf.rs"]
mod ttf;

use degus_glyph::{Error, Face, Glyph, Hinting};
use ttf::{TestFont, glyph, rect};

/// 1000 units per em, so at 10 px one pixel is 100 units.
fn face() -> Face {
    let font = TestFont {
        units_per_em: 1000,
        ascent: 800,
        descent: -200,
        monospace: true,
        glyphs: vec![
            glyph('A', 600, vec![rect(100, 0, 500, 700)]),
            glyph('g', 600, vec![rect(100, -200, 500, 500)]),
            glyph(
                'H',
                600,
                vec![
                    rect(100, 0, 200, 700),
                    rect(400, 0, 500, 700),
                    rect(200, 300, 400, 400),
                ],
            ),
            glyph(' ', 600, vec![]),
        ],
    };
    Face::new(font.build()).unwrap()
}

fn rows(glyph: &Glyph) -> Vec<String> {
    (0..glyph.height as i32)
        .map(|y| {
            (0..glyph.width as i32)
                .map(|x| match glyph.at(x, y) {
                    0 => '.',
                    255 => '#',
                    _ => '+',
                })
                .collect()
        })
        .collect()
}

#[test]
fn whole_pixel_rectangles_are_fully_covered() {
    let face = face();
    let scaler = face.scaler(10.0, Hinting::None).unwrap();
    let a = scaler.render('A').unwrap().unwrap();
    assert_eq!((a.left, a.top, a.width, a.height), (1, 7, 4, 7));
    assert_eq!(a.advance, 6.0);
    assert!(a.coverage.iter().all(|&c| c == 255));

    // Descends 2 px below the baseline.
    let g = scaler.render('g').unwrap().unwrap();
    assert_eq!((g.left, g.top, g.width, g.height), (1, 5, 4, 7));

    let h = scaler.render('H').unwrap().unwrap();
    assert_eq!(
        rows(&h),
        ["#..#", "#..#", "#..#", "####", "#..#", "#..#", "#..#"]
    );
}

#[test]
fn partly_covered_pixels_get_partial_coverage() {
    // At 15 px, 'A' spans x 1.5 to 7.5 and y 0 to 10.5.
    let face = face();
    let a = face
        .scaler(15.0, Hinting::None)
        .unwrap()
        .render('A')
        .unwrap()
        .unwrap();
    assert_eq!((a.left, a.top, a.width, a.height), (1, 11, 7, 11));
    let half = |c: u8| (126..=129).contains(&c);
    // The left and right columns and the top row are half covered.
    assert!(half(a.at(0, 5)) && half(a.at(6, 5)) && half(a.at(3, 0)));
    assert!(
        a.at(0, 0) < 70,
        "a corner is a quarter covered: {}",
        a.at(0, 0)
    );
    assert_eq!(a.at(3, 5), 255);
    assert_eq!(a.at(-1, 5), 0);
    assert_eq!(a.at(7, 5), 0);
}

#[test]
fn a_space_advances_without_coverage() {
    let face = face();
    let space = face
        .scaler(10.0, Hinting::None)
        .unwrap()
        .render(' ')
        .unwrap()
        .unwrap();
    assert_eq!((space.width, space.height, space.advance), (0, 0, 6.0));
    assert!(space.coverage.is_empty());
}

#[test]
fn characters_the_font_lacks_have_no_glyph() {
    let face = face();
    assert!(
        face.scaler(10.0, Hinting::None)
            .unwrap()
            .render('Z')
            .unwrap()
            .is_none()
    );
    assert!(!face.has_char('Z'));
    assert!(face.has_char('A'));
    assert_eq!(face.advance('Z', 10.0), None);
}

#[test]
fn metrics_scale_with_the_size() {
    let face = face();
    assert_eq!(face.units_per_em(), 1000);
    assert!(face.is_monospace());
    // skrifa scales in fixed point, so allow for rounding.
    let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
    let m = face.metrics(10.0);
    assert!(
        near(m.ascent, 8.0) && near(m.descent, 2.0) && near(m.height(), 10.0),
        "{m:?}"
    );
    assert!(near(face.advance('A', 20.0).unwrap(), 12.0));
}

#[test]
fn hinted_glyphs_draw() {
    let face = face();
    for hinting in [Hinting::Mono, Hinting::Smooth] {
        let scaler = face.scaler(10.0, hinting).unwrap();
        let h = scaler.render('H').unwrap().unwrap();
        assert!(h.width > 0 && h.height > 0, "{hinting:?}");
        assert_eq!(h.advance, 6.0, "{hinting:?}");
    }
}

#[test]
fn bad_sizes_and_files_are_errors() {
    let face = face();
    assert!(matches!(
        face.scaler(0.0, Hinting::None),
        Err(Error::Size(_))
    ));
    assert!(matches!(
        face.scaler(f32::NAN, Hinting::Mono),
        Err(Error::Size(_))
    ));
    assert!(matches!(
        Face::new(b"not a font".to_vec()),
        Err(Error::NotFont(_))
    ));
}
