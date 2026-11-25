//! Reading the set `degus sim` runs, at start and on reload.

use std::path::{Path, PathBuf};

use degus_set::config::Config;
use degus_set::{Device, DwinSet, FOLDER, Size};

use crate::SimArgs;

/// Where the set comes from, and the screen size the command line gives.
pub(crate) struct Source {
    /// The folder holding `DWIN_SET`.
    pub root: PathBuf,
    size: Option<Size>,
}

impl Source {
    pub fn new(args: &SimArgs) -> Self {
        let dir = &args.dir;
        let named_set = dir
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(FOLDER));
        let root = match dir.parent() {
            Some(parent) if named_set => parent.to_path_buf(),
            _ => dir.clone(),
        };
        Self {
            root,
            size: args.size,
        }
    }

    /// The `DWIN_SET` folder.
    pub fn folder(&self) -> PathBuf {
        self.root.join(FOLDER)
    }

    /// Reads the set, at the screen size the command line gives or the set
    /// suggests.
    pub fn read(&self) -> Result<DwinSet, String> {
        // The size isn't known before the files are read.
        let mut set =
            DwinSet::read(&self.root, Device::new(Size::new(1, 1))).map_err(|e| e.to_string())?;
        set.device.screen = match self.size.or_else(|| screen(&set)) {
            Some(size) => size,
            None => {
                return Err(format!(
                    "{}: no LCD block in the configuration and no page 0 background to take \
                     the screen size from; pass --size",
                    display(&self.folder())
                ));
            }
        };
        Ok(set)
    }
}

/// The size pages are drawn at: the configuration's LCD resolution,
/// swapped when the display is rotated by 90° or 270°, or the size of page
/// 0's background.
fn screen(set: &DwinSet) -> Option<Size> {
    let default = Config::default();
    let config = set.config.as_ref().unwrap_or(&default);
    if let Some(lcd) = config.lcd
        && lcd.h_d > 0
        && lcd.v_d > 0
    {
        return Some(if config.rotation.swaps_axes() {
            Size::new(lcd.v_d, lcd.h_d)
        } else {
            Size::new(lcd.h_d, lcd.v_d)
        });
    }
    let background = set.icls.get(&config.background_icl)?.icons.get(&0)?;
    Some(Size::new(background.width(), background.height()))
}

fn display(path: &Path) -> String {
    path.display().to_string()
}
