use std::fmt;

use skrifa::charmap::Charmap;
use skrifa::instance::{LocationRef, Size};
use skrifa::metrics::GlyphMetrics;
use skrifa::outline::{
    DrawSettings, Engine, HintingInstance, HintingOptions, OutlineGlyphCollection, Target,
};
use skrifa::{FontRef, GlyphId, MetadataProvider};

use crate::error::{Error, Result};
use crate::raster::{Glyph, Path};

/// A TrueType or OpenType font, from a file's bytes.
///
/// Variable fonts are drawn at their default instance.
#[derive(Clone)]
pub struct Face {
    data: Vec<u8>,
    index: u32,
}

/// How outlines are fitted to the pixel grid before they are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hinting {
    /// The outlines as designed, only scaled.
    None,
    /// Strong hinting for 1-bit pixels, FreeType's mono target. The font's
    /// own instructions are used, or the automatic hinter for fonts without
    /// them.
    Mono,
    /// Lighter hinting for gray pixels, FreeType's normal target.
    Smooth,
}

/// A font's vertical metrics at a pixel size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineMetrics {
    /// Pixels from the baseline up to the top of the line.
    pub ascent: f32,
    /// Pixels from the baseline down to the bottom of the line.
    pub descent: f32,
    /// Extra pixels the font asks for between lines.
    pub leading: f32,
}

impl LineMetrics {
    /// The line's height without leading: ascent and descent.
    pub fn height(&self) -> f32 {
        self.ascent + self.descent
    }
}

impl fmt::Debug for Face {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Face")
            .field("len", &self.data.len())
            .field("index", &self.index)
            .finish()
    }
}

impl Face {
    /// The font in `data`, a TrueType or OpenType file. For a collection
    /// (`.ttc`), the first font.
    pub fn new(data: impl Into<Vec<u8>>) -> Result<Self> {
        Self::from_collection(data, 0)
    }

    /// Font `index` of a font collection (`.ttc`). Index 0 also reads a
    /// single font.
    pub fn from_collection(data: impl Into<Vec<u8>>, index: u32) -> Result<Self> {
        let face = Self {
            data: data.into(),
            index,
        };
        let font = FontRef::from_index(&face.data, index)
            .map_err(|error| Error::NotFont(error.to_string()))?;
        if font.outline_glyphs().format().is_none() {
            return Err(Error::NoOutlines);
        }
        Ok(face)
    }

    fn font(&self) -> FontRef<'_> {
        FontRef::from_index(&self.data, self.index).expect("Face::new read the font")
    }

    pub fn units_per_em(&self) -> u16 {
        self.font()
            .metrics(Size::unscaled(), LocationRef::default())
            .units_per_em
    }

    /// Whether the font says all its glyphs have the same advance.
    pub fn is_monospace(&self) -> bool {
        self.font()
            .metrics(Size::unscaled(), LocationRef::default())
            .is_monospace
    }

    /// Whether the font has a glyph for `ch`.
    pub fn has_char(&self, ch: char) -> bool {
        glyph_id(&self.font().charmap(), ch).is_some()
    }

    /// The vertical metrics at `px` pixels per em, unhinted.
    pub fn metrics(&self, px: f32) -> LineMetrics {
        let metrics = self.font().metrics(Size::new(px), LocationRef::default());
        LineMetrics {
            ascent: metrics.ascent,
            descent: -metrics.descent,
            leading: metrics.leading,
        }
    }

    /// The advance of `ch` at `px` pixels per em, unhinted. `None` when the
    /// font has no glyph for it.
    pub fn advance(&self, ch: char, px: f32) -> Option<f32> {
        let font = self.font();
        let id = glyph_id(&font.charmap(), ch)?;
        font.glyph_metrics(Size::new(px), LocationRef::default())
            .advance_width(id)
    }

    /// Draws glyphs at `px` pixels per em.
    pub fn scaler(&self, px: f32, hinting: Hinting) -> Result<Scaler<'_>> {
        if !(px.is_finite() && px > 0.0) {
            return Err(Error::Size(px));
        }
        let font = self.font();
        let outlines = font.outline_glyphs();
        let size = Size::new(px);
        let target = match hinting {
            Hinting::None => None,
            Hinting::Mono => Some(Target::Mono),
            Hinting::Smooth => Some(Target::default()),
        };
        let hinting = target
            .map(|target| {
                let options = HintingOptions {
                    engine: Engine::AutoFallback,
                    target,
                };
                HintingInstance::new(&outlines, size, LocationRef::default(), options)
                    .map_err(|error| Error::Hinting(error.to_string()))
            })
            .transpose()?;
        Ok(Scaler {
            charmap: font.charmap(),
            metrics: font.glyph_metrics(size, LocationRef::default()),
            outlines,
            size,
            hinting,
        })
    }
}

/// Draws a [`Face`]'s glyphs at one pixel size; see [`Face::scaler`].
pub struct Scaler<'a> {
    charmap: Charmap<'a>,
    metrics: GlyphMetrics<'a>,
    outlines: OutlineGlyphCollection<'a>,
    size: Size,
    hinting: Option<HintingInstance>,
}

impl fmt::Debug for Scaler<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Scaler")
            .field("px", &self.size.ppem())
            .field("hinted", &self.hinting.is_some())
            .finish()
    }
}

impl Scaler<'_> {
    /// Draws `ch`. `None` when the font has no glyph for it.
    pub fn render(&self, ch: char) -> Result<Option<Glyph>> {
        let Some(id) = glyph_id(&self.charmap, ch) else {
            return Ok(None);
        };
        let Some(outline) = self.outlines.get(id) else {
            return Ok(None);
        };
        let settings = match &self.hinting {
            Some(instance) => DrawSettings::hinted(instance, false),
            None => DrawSettings::unhinted(self.size, LocationRef::default()),
        };
        let mut path = Path::default();
        let adjusted = outline
            .draw(settings, &mut path)
            .map_err(|error| Error::Draw {
                ch,
                message: error.to_string(),
            })?;
        let advance = adjusted
            .advance_width
            .or_else(|| self.metrics.advance_width(id))
            .unwrap_or(0.0);
        Ok(Some(path.rasterize(advance)))
    }
}

/// The glyph for `ch`, leaving out `.notdef`.
fn glyph_id(charmap: &Charmap<'_>, ch: char) -> Option<GlyphId> {
    charmap.map(ch).filter(|&id| id != GlyphId::NOTDEF)
}
