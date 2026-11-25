//! `degus`: tools for DWIN T5L displays. For now, `degus sim` runs a
//! `DWIN_SET` folder on a simulated display, in a window.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use degus_set::Size;

mod link;
mod load;
mod speaker;
mod window;

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Runs a DWIN_SET folder on a simulated display, in a window. Clicks
    /// are touches.
    Sim(SimArgs),
}

#[derive(Args)]
pub(crate) struct SimArgs {
    /// The DWIN_SET folder, or the folder holding it.
    pub dir: PathBuf,
    /// Screen size in pixels, such as 800x480. Without it, the size comes
    /// from the configuration's LCD block, or from page 0's background.
    #[arg(long, value_name = "WxH", value_parser = parse_size)]
    pub size: Option<Size>,
    /// Accepts a host on this TCP address, such as 127.0.0.1:5000.
    #[arg(long, value_name = "ADDR")]
    pub tcp: Option<String>,
    /// Links a host on this serial port, such as COM5 or /dev/ttyUSB0.
    #[cfg(feature = "serial")]
    #[arg(long, value_name = "PORT")]
    pub serial: Option<String>,
    /// Baud rate for --serial, instead of the configuration's.
    #[cfg(feature = "serial")]
    #[arg(long, requires = "serial")]
    pub baud: Option<u32>,
}

fn parse_size(text: &str) -> Result<Size, String> {
    let (w, h) = text
        .split_once(['x', 'X'])
        .ok_or("expected WxH, such as 800x480")?;
    let number = |n: &str| n.trim().parse::<u16>().map_err(|e| format!("{n:?}: {e}"));
    let (w, h) = (number(w)?, number(h)?);
    if w == 0 || h == 0 {
        return Err("the size must not be 0".to_owned());
    }
    Ok(Size::new(w, h))
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Sim(args) => window::run(args),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sizes() {
        assert_eq!(parse_size("800x480"), Ok(Size::new(800, 480)));
        assert_eq!(parse_size("1920X1080"), Ok(Size::new(1920, 1080)));
        assert!(parse_size("800").is_err());
        assert!(parse_size("0x480").is_err());
        assert!(parse_size("800x-1").is_err());
    }
}
