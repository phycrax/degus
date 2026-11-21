use std::collections::BTreeMap;

use image::DynamicImage;

use crate::encode::encode_source;
use crate::error::{Error, Result};
use crate::icl::{Icl, Icon};
use crate::options::IclOptions;

/// Where an icon's pixels come from.
#[derive(Debug, Clone)]
pub enum IconSource {
    /// Encoded image bytes (PNG, BMP, GIF or JPEG).
    Encoded(Vec<u8>),
    /// Decoded pixels.
    Image(DynamicImage),
    /// A baseline JPEG to store unchanged, regardless of
    /// [`IclOptions::passthrough_jpeg`]. Fails if the T5L cannot decode it.
    Jpeg(Vec<u8>),
}

/// Collects icons by ID and encodes them into an [`Icl`].
#[derive(Debug, Clone, Default)]
pub struct IclBuilder {
    pub options: IclOptions,
    icons: BTreeMap<u16, IconSource>,
}

impl IclBuilder {
    pub fn new(options: IclOptions) -> Self {
        Self {
            options,
            icons: BTreeMap::new(),
        }
    }

    /// Sets the icon for `id`, returning the source it replaced.
    pub fn insert(&mut self, id: u16, source: IconSource) -> Option<IconSource> {
        self.icons.insert(id, source)
    }

    pub fn remove(&mut self, id: u16) -> Option<IconSource> {
        self.icons.remove(&id)
    }

    pub fn contains(&self, id: u16) -> bool {
        self.icons.contains_key(&id)
    }

    pub fn len(&self) -> usize {
        self.icons.len()
    }

    pub fn is_empty(&self) -> bool {
        self.icons.is_empty()
    }

    /// Icon IDs in ascending order.
    pub fn ids(&self) -> impl Iterator<Item = u16> + '_ {
        self.icons.keys().copied()
    }

    /// Encodes every icon: the slow part of making an ICL.
    pub fn build(&self) -> Result<Icl> {
        let mut icl = Icl::new();
        for (&id, source) in &self.icons {
            let encoded = encode_source(source, &self.options).map_err(|e| Error::Icon {
                id,
                source: Box::new(e),
            })?;
            icl.icons.insert(id, Icon::from_prepared(encoded));
        }
        Ok(icl)
    }
}
