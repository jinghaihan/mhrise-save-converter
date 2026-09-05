use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use mhrise_save_converter::discover::discover_core_files;

#[derive(Debug, Parser)]
#[command(name = "mhrise-save", version, about = "Inspect and convert Monster Hunter Rise saves")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Inspect DSSS headers and integrity checks without changing files.
    Inspect {
        /// A save file or a win64_save directory.
        path: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Inspect { path } => inspect(&path),
    }
}

fn inspect(path: &std::path::Path) -> Result<()> {
    let files = discover_core_files(path)?;
    println!("Input: {}", path.display());
    println!("Core files: {}", files.len());

    for file in files {
        let name = file.path.file_name().and_then(|value| value.to_str()).unwrap_or("<unknown>");
        println!("- {name}: platform={}, checksum={}", file.platform, file.checksum);
    }

    Ok(())
}
