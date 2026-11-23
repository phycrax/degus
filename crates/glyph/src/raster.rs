use ab_glyph_rasterizer::{Point, Rasterizer, point};
use skrifa::outline::OutlinePen;

/// A drawn glyph: coverage over the pixels its outline touches.
#[derive(Debug, Clone, PartialEq)]
pub struct Glyph {
    /// Columns from the pen position to the coverage's left edge.
    pub left: i32,
    /// Rows from the baseline up to the coverage's top edge.
    pub top: i32,
    /// Width of the coverage, 0 for a glyph without outline, such as a
    /// space.
    pub width: u32,
    pub height: u32,
    /// Pixels from this glyph's pen position to the next one's.
    pub advance: f32,
    /// Row by row from the top: 0 is uncovered, 255 fully covered.
    pub coverage: Vec<u8>,
}

impl Glyph {
    /// The coverage of pixel `(x, y)` of the coverage box, 0 outside it.
    pub fn at(&self, x: i32, y: i32) -> u8 {
        let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else {
            return 0;
        };
        if x >= self.width || y >= self.height {
            return 0;
        }
        self.coverage[(y * self.width + x) as usize]
    }
}

#[derive(Debug, Clone, Copy)]
enum Segment {
    Line([(f32, f32); 2]),
    Quad([(f32, f32); 3]),
    Cubic([(f32, f32); 4]),
}

impl Segment {
    fn points(&self) -> &[(f32, f32)] {
        match self {
            Segment::Line(p) => p,
            Segment::Quad(p) => p,
            Segment::Cubic(p) => p,
        }
    }
}

/// A glyph outline in pixels, y up, as skrifa draws it.
#[derive(Debug, Default)]
pub(crate) struct Path {
    segments: Vec<Segment>,
    start: (f32, f32),
    current: (f32, f32),
}

impl OutlinePen for Path {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close();
        self.start = (x, y);
        self.current = (x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.segments.push(Segment::Line([self.current, (x, y)]));
        self.current = (x, y);
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.segments
            .push(Segment::Quad([self.current, (cx0, cy0), (x, y)]));
        self.current = (x, y);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.segments.push(Segment::Cubic([
            self.current,
            (cx0, cy0),
            (cx1, cy1),
            (x, y),
        ]));
        self.current = (x, y);
    }

    fn close(&mut self) {
        if self.current != self.start {
            self.segments
                .push(Segment::Line([self.current, self.start]));
        }
        self.current = self.start;
    }
}

impl Path {
    /// Coverage of the pixels the outline touches, which lie within the
    /// bounds of its points.
    pub(crate) fn rasterize(mut self, advance: f32) -> Glyph {
        self.close();
        let points = self.segments.iter().flat_map(|s| s.points().iter());
        let (mut x_min, mut y_min) = (f32::INFINITY, f32::INFINITY);
        let (mut x_max, mut y_max) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
        for &(x, y) in points {
            x_min = x_min.min(x);
            x_max = x_max.max(x);
            y_min = y_min.min(y);
            y_max = y_max.max(y);
        }
        let empty = Glyph {
            left: 0,
            top: 0,
            width: 0,
            height: 0,
            advance,
            coverage: Vec::new(),
        };
        if self.segments.is_empty() || !(x_min < x_max && y_min < y_max) {
            return empty;
        }

        let left = x_min.floor() as i32;
        let top = y_max.ceil() as i32;
        let width = (x_max.ceil() as i32 - left) as u32;
        let height = (top - y_min.floor() as i32) as u32;
        // Pixel space for the rasterizer: from the box's top left, y down.
        let map = |(x, y): (f32, f32)| -> Point { point(x - left as f32, top as f32 - y) };
        let mut rasterizer = Rasterizer::new(width as usize, height as usize);
        for segment in &self.segments {
            match *segment {
                Segment::Line([a, b]) => rasterizer.draw_line(map(a), map(b)),
                Segment::Quad([a, b, c]) => rasterizer.draw_quad(map(a), map(b), map(c)),
                Segment::Cubic([a, b, c, d]) => {
                    rasterizer.draw_cubic(map(a), map(b), map(c), map(d))
                }
            }
        }
        let mut coverage = vec![0; (width * height) as usize];
        rasterizer.for_each_pixel(|index, alpha| {
            coverage[index] = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
        });
        Glyph {
            left,
            top,
            width,
            height,
            advance,
            coverage,
        }
    }
}
