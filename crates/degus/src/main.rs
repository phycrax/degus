//! `degus`: builds a DWIN T5L `DWIN_SET` folder from a TOML project.
//! `degus guide` prints the project format, `docs/project-format.md` in the
//! repository, and `degus sim` runs the project on a simulated display.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use degus_project::model::Size;
use degus_project::{BuiltFile, DWIN_SET, Diagnostic, Error, Project, Result, Severity};
use degus_sim::{LoadError, Runner, Sim, World};

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
    /// Runs the project on a simulated display, in a window or from a
    /// script.
    Sim(SimArgs),
}

#[derive(Args)]
struct SimArgs {
    #[command(flatten)]
    project: ProjectArg,
    /// Runs this script without a window, printing what passes between host
    /// and display; `shot` paths are relative to the script.
    #[arg(long, value_name = "FILE")]
    script: Option<PathBuf>,
    /// Accepts a host on this TCP address, such as 127.0.0.1:5000.
    #[arg(long, value_name = "ADDR", conflicts_with = "script")]
    tcp: Option<String>,
    /// Accepts a host on this serial port at the configured baud rate.
    #[cfg(feature = "serial")]
    #[arg(long, value_name = "PORT", conflicts_with = "script")]
    serial: Option<String>,
    /// What the host speaks over `--tcp` or `--serial`.
    #[arg(long, value_enum, default_value_t = Protocol::Frames)]
    protocol: Protocol,
}

/// What a link to the host speaks.
#[derive(Clone, Copy, ValueEnum)]
enum Protocol {
    /// DGUS frames, `5A A5 len cmd data [crc]`, as the panel speaks them on
    /// UART2, with CRC as the project's `crc` sets it.
    Frames,
    /// Script lines, `write 0x1000 1` and `read 0x1000 2`, answered `ack`
    /// and `data 0x1000 0x0001`: for typing by hand, such as with `nc`.
    Lines,
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
        Command::Sim(args) => return sim(args),
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

fn sim(args: SimArgs) -> ExitCode {
    let world = match World::load(&args.project.project) {
        Ok(world) => world,
        Err(error) => {
            if let LoadError::Check(diagnostics) = &error {
                print_diagnostics(diagnostics);
            }
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    print_diagnostics(&world.warnings);
    let mut sim = Sim::new(world);

    let Some(script) = &args.script else {
        return window(sim, &args);
    };
    let text = match std::fs::read_to_string(script) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("error: {}: {error}", script.display());
            return ExitCode::FAILURE;
        }
    };
    let base = script.parent().unwrap_or(Path::new("."));
    match Runner::new(&mut sim, base, |line| println!("{line}")).run(&text) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {}: {error}", script.display());
            ExitCode::FAILURE
        }
    }
}

#[cfg(feature = "gui")]
fn window(sim: Sim, args: &SimArgs) -> ExitCode {
    use degus_sim::{Codec, FrameCodec, LineCodec, Link};

    let crc = sim.world().config.crc;
    let codec = || -> Box<dyn Codec> {
        match args.protocol {
            Protocol::Frames => Box::new(FrameCodec::new(crc)),
            Protocol::Lines => Box::new(LineCodec::new()),
        }
    };
    let gui = degus_sim_gui::Gui::new();
    let mut links = Vec::new();
    if let Some(addr) = &args.tcp {
        match Link::tcp(addr, codec(), gui.waker()) {
            Ok(link) => links.push(link),
            Err(error) => {
                eprintln!("error: cannot listen on {addr}: {error}");
                return ExitCode::FAILURE;
            }
        }
    }
    #[cfg(feature = "serial")]
    if let Some(port) = &args.serial {
        let baud = sim.world().config.baud();
        match Link::serial(port, baud, codec(), gui.waker()) {
            Ok(link) => links.push(link),
            Err(error) => {
                eprintln!("error: cannot open {port}: {error}");
                return ExitCode::FAILURE;
            }
        }
    }
    let title = std::fs::canonicalize(&args.project.project)
        .ok()
        .and_then(|path| path.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "project".to_owned());
    match gui.run(sim, links, &title) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(feature = "gui"))]
fn window(_sim: Sim, _args: &SimArgs) -> ExitCode {
    eprintln!(
        "error: this degus has no window; pass --script, or install it with `--features gui`"
    );
    ExitCode::FAILURE
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
