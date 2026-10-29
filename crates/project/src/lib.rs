//! Builds a DWIN T5L `DWIN_SET` folder from a project described in TOML. A
//! project is a folder: its `project.toml` lists the screen, the hardware
//! configuration, the icon (ICL) and audio (WAE) libraries, the files to
//! copy and the controls file of each page, with paths relative to the
//! folder.
//!
//! The build writes a `DWIN_SET` folder in the output folder, `target`
//! unless set, replacing the one there. It holds `<id>.icl` and `<id>.wae`
//! for each library, `13TouchFile.bin` and `14ShowFile.bin` for the
//! controls, `T5LCFG.CFG` from `[config]`, and the copied files.
//! [`Project::check`] reports problems without building, [`Project::clean`]
//! deletes `DWIN_SET` and [`new`] creates a project folder. See
//! `docs/project-format.md` in the repository for the file format.
//!
//! ```no_run
//! use degus_project::Project;
//!
//! let project = Project::load("my-hmi")?;
//! for diagnostic in project.check() {
//!     eprintln!("{diagnostic}");
//! }
//! let report = project.build()?;
//! println!("wrote {} files to {}", report.files.len(), report.output.display());
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod build;
mod check;
mod create;
mod error;
mod id;
pub mod model;
mod project;
mod sources;

pub use build::{BuildReport, BuiltFile, DWIN_SET, SLOT_SIZE};
pub use check::{Diagnostic, Severity};
pub use create::new;
pub use error::{Error, Result};
pub use model::PROJECT_FILE_NAME;
pub use project::{ControlsEntry, Project};
