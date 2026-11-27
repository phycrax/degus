//! Builds small TrueType fonts for tests, whose glyphs are polygons in font
//! units, so that tests know exactly which pixels a glyph covers.
//!
//! The font has the tables skrifa needs to map, measure and draw glyphs:
//! `cmap`, `glyf`, `head`, `hhea`, `hmtx`, `loca`, `maxp` and `post`. It has
//! no hinting instructions, so mono hinting uses the automatic hinter.

#![allow(dead_code)]

/// A glyph: the character it maps, its advance and its contours, each a
/// polygon of on-curve points.
pub struct TestGlyph {
    pub ch: char,
    pub advance: u16,
    pub contours: Vec<Vec<(i16, i16)>>,
}

pub struct TestFont {
    pub units_per_em: u16,
    pub ascent: i16,
    /// Negative, below the baseline, as in `hhea`.
    pub descent: i16,
    pub monospace: bool,
    pub glyphs: Vec<TestGlyph>,
}

/// A rectangle from `(x0, y0)` to `(x1, y1)`, clockwise as TrueType draws
/// outer contours.
pub fn rect(x0: i16, y0: i16, x1: i16, y1: i16) -> Vec<(i16, i16)> {
    vec![(x0, y0), (x0, y1), (x1, y1), (x1, y0)]
}

pub fn glyph(ch: char, advance: u16, contours: Vec<Vec<(i16, i16)>>) -> TestGlyph {
    TestGlyph {
        ch,
        advance,
        contours,
    }
}

struct Bytes(Vec<u8>);

impl Bytes {
    fn u16(&mut self, v: u16) -> &mut Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }
    fn i16(&mut self, v: i16) -> &mut Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }
    fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }
    fn zeros(&mut self, n: usize) -> &mut Self {
        self.0.resize(self.0.len() + n, 0);
        self
    }
}

impl TestFont {
    pub fn build(&self) -> Vec<u8> {
        // Glyph 0 is .notdef, empty; the others follow in order.
        let count = self.glyphs.len() as u16 + 1;
        let bounds = |g: &TestGlyph| {
            let points = g.contours.iter().flatten();
            let xs = points.clone().map(|p| p.0);
            let ys = points.map(|p| p.1);
            (
                xs.clone().min().unwrap_or(0),
                ys.clone().min().unwrap_or(0),
                xs.max().unwrap_or(0),
                ys.max().unwrap_or(0),
            )
        };

        let mut glyf = Bytes(Vec::new());
        // Where each glyph starts, and where the last one ends. .notdef is
        // empty: it starts and ends at 0.
        let mut loca = vec![0u32, 0];
        for g in &self.glyphs {
            if !g.contours.is_empty() {
                let (x_min, y_min, x_max, y_max) = bounds(g);
                glyf.i16(g.contours.len() as i16)
                    .i16(x_min)
                    .i16(y_min)
                    .i16(x_max)
                    .i16(y_max);
                let mut end = 0u16;
                for contour in &g.contours {
                    end += contour.len() as u16;
                    glyf.u16(end - 1);
                }
                glyf.u16(0); // no instructions
                let points: Vec<_> = g.contours.iter().flatten().copied().collect();
                for _ in &points {
                    glyf.0.push(0x01); // on curve, 16-bit deltas
                }
                let mut previous = 0i16;
                for p in &points {
                    glyf.i16(p.0 - previous);
                    previous = p.0;
                }
                previous = 0;
                for p in &points {
                    glyf.i16(p.1 - previous);
                    previous = p.1;
                }
                while !glyf.0.len().is_multiple_of(4) {
                    glyf.0.push(0);
                }
            }
            loca.push(glyf.0.len() as u32);
        }
        let mut loca_table = Bytes(Vec::new());
        for offset in &loca {
            loca_table.u32(*offset);
        }

        let all = self.glyphs.iter().map(bounds);
        let x_min = all.clone().map(|b| b.0).min().unwrap_or(0);
        let y_min = all.clone().map(|b| b.1).min().unwrap_or(0);
        let x_max = all.clone().map(|b| b.2).max().unwrap_or(0);
        let y_max = all.map(|b| b.3).max().unwrap_or(0);
        let advance_max = self.glyphs.iter().map(|g| g.advance).max().unwrap_or(0);
        let max_points = self
            .glyphs
            .iter()
            .map(|g| g.contours.iter().map(Vec::len).sum::<usize>())
            .max()
            .unwrap_or(0) as u16;
        let max_contours = self
            .glyphs
            .iter()
            .map(|g| g.contours.len())
            .max()
            .unwrap_or(0) as u16;

        let mut head = Bytes(Vec::new());
        head.u16(1)
            .u16(0)
            .u32(0x0001_0000)
            .u32(0)
            .u32(0x5F0F_3CF5)
            .u16(0x0003)
            .u16(self.units_per_em)
            .zeros(16)
            .i16(x_min)
            .i16(y_min)
            .i16(x_max)
            .i16(y_max)
            .u16(0)
            .u16(8)
            .i16(2)
            .i16(1) // long loca offsets
            .i16(0);

        let mut hhea = Bytes(Vec::new());
        hhea.u32(0x0001_0000)
            .i16(self.ascent)
            .i16(self.descent)
            .i16(0)
            .u16(advance_max)
            .i16(x_min)
            .i16(0)
            .i16(x_max)
            .i16(1)
            .i16(0)
            .i16(0)
            .zeros(8)
            .i16(0)
            .u16(count);

        let mut hmtx = Bytes(Vec::new());
        hmtx.u16(0).i16(0);
        for g in &self.glyphs {
            hmtx.u16(g.advance).i16(bounds(g).0);
        }

        let mut maxp = Bytes(Vec::new());
        maxp.u32(0x0001_0000)
            .u16(count)
            .u16(max_points)
            .u16(max_contours)
            .u16(0)
            .u16(0)
            .u16(2)
            .zeros(16);

        let mut groups: Vec<(u32, u32)> = self
            .glyphs
            .iter()
            .enumerate()
            .map(|(i, g)| (g.ch as u32, i as u32 + 1))
            .collect();
        groups.sort();
        let mut cmap = Bytes(Vec::new());
        cmap.u16(0).u16(1).u16(3).u16(10).u32(12);
        cmap.u16(12)
            .u16(0)
            .u32(16 + 12 * groups.len() as u32)
            .u32(0)
            .u32(groups.len() as u32);
        for (ch, id) in groups {
            cmap.u32(ch).u32(ch).u32(id);
        }

        let mut post = Bytes(Vec::new());
        post.u32(0x0003_0000)
            .u32(0)
            .i16(-100)
            .i16(50)
            .u32(u32::from(self.monospace))
            .zeros(16);

        let tables: [(&[u8; 4], Vec<u8>); 8] = [
            (b"cmap", cmap.0),
            (b"glyf", glyf.0),
            (b"head", head.0),
            (b"hhea", hhea.0),
            (b"hmtx", hmtx.0),
            (b"loca", loca_table.0),
            (b"maxp", maxp.0),
            (b"post", post.0),
        ];
        let mut font = Bytes(Vec::new());
        font.u32(0x0001_0000).u16(8).u16(128).u16(3).u16(0);
        let mut offset = 12 + 16 * tables.len() as u32;
        for (tag, data) in &tables {
            let mut padded = data.clone();
            padded.resize(data.len().next_multiple_of(4), 0);
            let (words, _) = padded.as_chunks::<4>();
            let checksum = words
                .iter()
                .copied()
                .map(u32::from_be_bytes)
                .fold(0u32, u32::wrapping_add);
            font.0.extend_from_slice(*tag);
            font.u32(checksum).u32(offset).u32(data.len() as u32);
            offset += padded.len() as u32;
        }
        for (_, data) in &tables {
            font.0.extend_from_slice(data);
            while !font.0.len().is_multiple_of(4) {
                font.0.push(0);
            }
        }
        font.0
    }
}
