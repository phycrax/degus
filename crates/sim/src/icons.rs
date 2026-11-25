//! Icons, decoded from the set's ICLs on first use. They are the JPEGs the
//! display stores, so they show the same compression artifacts.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use degus_set::icl::Icl;
use degus_set::icl::image::RgbImage;

/// Decoded icons by ICL and icon ID; `None` for ones that failed.
type Cache = HashMap<(u8, u16), Option<Arc<RgbImage>>>;

#[derive(Debug, Default)]
pub(crate) struct Icons {
    cache: RefCell<Cache>,
    problems: RefCell<Vec<String>>,
}

impl Icons {
    /// Icon `id` of ICL `lib`, decoded.
    pub fn get(&self, icls: &BTreeMap<u8, Icl>, lib: u8, id: u16) -> Option<Arc<RgbImage>> {
        if let Some(cached) = self.cache.borrow().get(&(lib, id)) {
            return cached.clone();
        }
        let icon = icls.get(&lib)?.icons.get(&id)?;
        let decoded = match icon.decode() {
            Ok(image) => Some(Arc::new(image)),
            Err(error) => {
                self.problems
                    .borrow_mut()
                    .push(format!("{lib}.icl, icon {id}: {error}"));
                None
            }
        };
        self.cache.borrow_mut().insert((lib, id), decoded.clone());
        decoded
    }

    /// Problems met while decoding since the last call.
    pub fn take_problems(&self) -> Vec<String> {
        std::mem::take(&mut self.problems.borrow_mut())
    }
}
