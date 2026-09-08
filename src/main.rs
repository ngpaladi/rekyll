use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "rekyll", version, about = "Jekyll, but make it Rust")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Build the site.
    Build {
        /// Source directory.
        #[arg(short = 's', long = "source", default_value = ".")]
        source: PathBuf,
        /// Destination directory.
        #[arg(short = 'd', long = "destination")]
        destination: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Build { source, destination } => {
            let dest = destination.unwrap_or_else(|| source.join("_site"));
            rekyll::build::build(&source, &dest)
        }
    }
}
