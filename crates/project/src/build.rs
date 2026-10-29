use std::path::{Path, PathBuf};

use degus_controls::{DISPLAY_FILE_NAME, TOUCH_FILE_NAME};

use crate::check::{Diagnostic, Severity};
use crate::error::{Error, Result, io_error};
use crate::id::id_from_file_name;
use crate::project::Project;

/// Flash bytes per file number: the T5L stores file N at N × 256 KB.
pub const SLOT_SIZE: u64 = 256 * 1024;

/// The folder the build writes in the output folder: the one the display
/// downloads from an SD card.
pub const DWIN_SET: &str = "DWIN_SET";

/// What [`Project::build`] wrote.
#[derive(Debug, Clone)]
pub struct BuildReport {
    /// The `DWIN_SET` folder written.
    pub output: PathBuf,
    /// Files written, in the order they were built.
    pub files: Vec<BuiltFile>,
    /// Problems that did not stop the build.
    pub warnings: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltFile {
    pub name: String,
    pub len: usize,
    /// The leading number of the name, which places the file in flash.
    /// `None` for files the display does not store by number.
    pub number: Option<u16>,
}

impl BuiltFile {
    /// Flash file numbers the file occupies, from `number` on.
    pub fn slots(&self) -> usize {
        self.len.div_ceil(SLOT_SIZE as usize).max(1)
    }
}

impl Project {
    /// Checks the project and, if it has no errors, writes `DWIN_SET` in its
    /// output folder, replacing the one there.
    pub fn build(&self) -> Result<BuildReport> {
        self.build_to(self.output_dir())
    }

    /// Like [`Project::build`], into `output` instead of the configured folder.
    pub fn build_to(&self, output: impl AsRef<Path>) -> Result<BuildReport> {
        let analysis = self.analyze();
        let diagnostics = analysis.diagnostics;
        if diagnostics.iter().any(|d| d.severity == Severity::Error) {
            return Err(Error::Check(diagnostics));
        }

        let mut files = Vec::new();
        for (id, builder) in analysis.icls {
            let icl = builder
                .build()
                .map_err(|source| Error::Icl { id, source })?;
            files.push((format!("{id}.icl"), icl.bytes));
        }
        for (id, builder) in analysis.waes {
            let wae = builder
                .build()
                .map_err(|source| Error::Wae { id, source })?;
            files.push((format!("{id}.wae"), wae.bytes));
        }
        let controls = self.to_controls().build()?;
        files.push((TOUCH_FILE_NAME.to_owned(), controls.touch));
        files.push((DISPLAY_FILE_NAME.to_owned(), controls.display));
        if let Some(bytes) = analysis.config {
            files.push((degus_config::FILE_NAME.to_owned(), bytes));
        }
        for path in &self.config.copy {
            let source = self.resolve(path);
            let bytes = std::fs::read(&source).map_err(io_error(&source))?;
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            files.push((name, bytes));
        }
        check_flash(&files)?;

        // Every file is built each time, so nothing of an earlier build is
        // kept: a leftover would still reach the display.
        let set = output.as_ref().join(DWIN_SET);
        if set.exists() {
            std::fs::remove_dir_all(&set).map_err(io_error(&set))?;
        }
        std::fs::create_dir_all(&set).map_err(io_error(&set))?;
        for (name, bytes) in &files {
            let path = set.join(name);
            std::fs::write(&path, bytes).map_err(io_error(&path))?;
        }

        Ok(BuildReport {
            output: set,
            files: files
                .into_iter()
                .map(|(name, bytes)| BuiltFile {
                    number: id_from_file_name(&name),
                    name,
                    len: bytes.len(),
                })
                .collect(),
            warnings: diagnostics,
        })
    }

    /// Deletes the `DWIN_SET` folder the build writes, and the output
    /// folder if that leaves it empty. Returns whether there was one.
    pub fn clean(&self) -> Result<bool> {
        let output = self.output_dir();
        let set = output.join(DWIN_SET);
        if !set.exists() {
            return Ok(false);
        }
        std::fs::remove_dir_all(&set).map_err(io_error(&set))?;
        // Fails, keeping the folder, when anything else is in it.
        let _ = std::fs::remove_dir(&output);
        Ok(true)
    }
}

/// Fails when a numbered file runs into the flash of the next one.
fn check_flash(files: &[(String, Vec<u8>)]) -> Result<()> {
    let mut ranges: Vec<(u64, u64, &str)> = files
        .iter()
        .filter_map(|(name, bytes)| {
            let start = u64::from(id_from_file_name(name)?) * SLOT_SIZE;
            Some((start, start + (bytes.len() as u64).max(1), name.as_str()))
        })
        .collect();
    ranges.sort();
    for (&(_, end, first), &(start, _, second)) in ranges.iter().zip(ranges.iter().skip(1)) {
        if start < end {
            return Err(Error::FlashOverlap {
                first: first.to_owned(),
                second: second.to_owned(),
            });
        }
    }
    Ok(())
}
