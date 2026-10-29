//! `degus`: builds a DWIN T5L `DWIN_SET` folder from a TOML project.
//! `degus guide` prints the project format, `docs/project-format.md` in the
//! repository.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use degus_project::model::Size;
use degus_project::{BuiltFile, DWIN_SET, Diagnostic, Error, Project, Result, Severity};

/// The project format reference that `degus guide` prints.
const GUIDE: &str = include_str!("../../../docs/project-format.md");

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Checks the project and writes its DWIN_SET folder, replacing the one
    /// there.
    Build {
        #[command(flatten)]
        project: ProjectArg,
        /// Folder to write DWIN_SET in, instead of the project's `output`.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Reports problems without building.
    Check {
        #[command(flatten)]
        project: ProjectArg,
    },
    /// Deletes the project's DWIN_SET folder.
    Clean {
        #[command(flatten)]
        project: ProjectArg,
    },
    /// Creates a project folder.
    New {
        /// The folder to create; it must not exist.
        path: PathBuf,
        /// Screen size in pixels.
        #[arg(long, default_value = "1920x1080")]
        screen: Size,
    },
    /// Prints the project format: how to write project.toml and the
    /// controls files.
    Guide,
}

#[derive(Args)]
struct ProjectArg {
    /// The project folder, which holds project.toml.
    #[arg(default_value = ".")]
    project: PathBuf,
}

fn main() -> ExitCode {
    let result = match Cli::parse().command {
        Command::Build { project, output } => build(project.project, output),
        Command::Check { project } => check(project.project),
        Command::Clean { project } => clean(project.project),
        Command::New { path, screen } => new(path, screen),
        Command::Guide => guide(),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if let Error::Check(diagnostics) = &error {
                print_diagnostics(diagnostics);
            }
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn build(path: PathBuf, output: Option<PathBuf>) -> Result<()> {
    let project = Project::load(path)?;
    let report = match output {
        Some(output) => project.build_to(output)?,
        None => project.build()?,
    };
    print_diagnostics(&report.warnings);

    let name_width = report.files.iter().map(|f| f.name.len()).max();
    let len_width = report.files.iter().map(|f| f.len.to_string().len()).max();
    for file in &report.files {
        println!(
            "  {:name_width$}  {:>len_width$} B  {}",
            file.name,
            file.len,
            flash_files(file),
            name_width = name_width.unwrap_or(0),
            len_width = len_width.unwrap_or(0),
        );
    }
    println!(
        "wrote {} file(s) to {}{}",
        report.files.len(),
        report.output.display(),
        warning_count(report.warnings.len()),
    );
    Ok(())
}

fn check(path: PathBuf) -> Result<()> {
    let project = Project::load(path)?;
    let diagnostics = project.check();
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return Err(Error::Check(diagnostics));
    }
    print_diagnostics(&diagnostics);
    println!(
        "checked {}{}",
        project.file().display(),
        warning_count(diagnostics.len()),
    );
    Ok(())
}

fn clean(path: PathBuf) -> Result<()> {
    let project = Project::load(path)?;
    let set = project.output_dir().join(DWIN_SET);
    if project.clean()? {
        println!("removed {}", set.display());
    } else {
        println!("nothing to clean: {} does not exist", set.display());
    }
    Ok(())
}

fn new(path: PathBuf, screen: Size) -> Result<()> {
    degus_project::new(&path, screen)?;
    println!("created {}", path.display());
    Ok(())
}

fn guide() -> Result<()> {
    // A closed pipe, as in `degus guide | head`, is not an error.
    let _ = std::io::stdout().write_all(GUIDE.as_bytes());
    Ok(())
}

fn print_diagnostics(diagnostics: &[Diagnostic]) {
    for diagnostic in diagnostics {
        eprintln!("{diagnostic}");
    }
}

/// The flash file numbers `file` takes, empty for files not stored by number.
fn flash_files(file: &BuiltFile) -> String {
    match (file.number, file.slots()) {
        (None, _) => String::new(),
        (Some(first), 1) => format!("file {first}"),
        (Some(first), slots) => format!("files {first}-{}", usize::from(first) + slots - 1),
    }
}

fn warning_count(warnings: usize) -> String {
    match warnings {
        0 => String::new(),
        n => format!(" ({n} warning(s))"),
    }
}
