//! Turns `[[icl]]` and `[[wae]]` entries into builders. Nothing is encoded
//! here, so the check stays fast.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use degus_icl::{Chip, FlashSlot, IclBuilder, IclOptions, IconSource};
use degus_wae::{ClipSource, WaeBuilder, WaeOptions};

use crate::model::{IclSpec, Source, WaeSpec};

pub(crate) fn prepare_icl(root: &Path, spec: &IclSpec, chip: Chip) -> Result<IclBuilder, String> {
    if !(1..=100).contains(&spec.quality) {
        return Err(format!("quality {} is not in 1-100", spec.quality));
    }

    let mut builder = IclBuilder::new(IclOptions {
        quality: spec.quality,
        subsampling: spec.subsampling.into(),
        passthrough_jpeg: spec.passthrough_jpeg,
        chip,
        flash_slot: Some(FlashSlot::new(spec.id)),
        ..IclOptions::default()
    });
    for (id, path) in resolve(root, &spec.images, "images")? {
        builder.insert(id, IconSource::File(path));
    }
    Ok(builder)
}

pub(crate) fn prepare_wae(root: &Path, spec: &WaeSpec) -> Result<WaeBuilder, String> {
    let mut builder = WaeBuilder::new(WaeOptions {
        format: spec.format.into(),
        downmix: spec.downmix.into(),
    });
    for (id, path) in resolve(root, &spec.clips, "clips")? {
        builder.insert(id, ClipSource::File(path));
    }
    Ok(builder)
}

/// The ID and full path of each source. There must be sources, their
/// paths must exist and their IDs must not repeat.
fn resolve(root: &Path, sources: &[Source], key: &str) -> Result<Vec<(u16, PathBuf)>, String> {
    if sources.is_empty() {
        return Err(format!("needs `{key}`"));
    }
    let mut ids = BTreeSet::new();
    sources
        .iter()
        .map(|source| {
            if !ids.insert(source.id) {
                return Err(format!("{key}: ID {} is listed twice", source.id));
            }
            Ok((source.id, existing(root, &source.path)?))
        })
        .collect()
}

fn existing(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let full = root.join(path);
    if full.is_file() {
        Ok(full)
    } else {
        Err(format!("{} not found", path.display()))
    }
}
