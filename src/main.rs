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
        #[command(flatten)]
        paths: Paths,
    },
    /// Build the site, then serve it.
    #[cfg(feature = "serve")]
    Serve {
        #[command(flatten)]
        paths: Paths,
        /// Host to bind.
        #[arg(short = 'H', long, default_value = rekyll::serve::DEFAULT_HOST)]
        host: String,
        /// Port to listen on.
        #[arg(short = 'P', long, default_value_t = rekyll::serve::DEFAULT_PORT)]
        port: u16,
        /// Serve an existing build instead of rebuilding first.
        #[arg(long)]
        skip_initial_build: bool,
        /// Do not rebuild when a source file changes.
        #[arg(long)]
        no_watch: bool,
        /// Do not inject the reload script into served HTML.
        #[arg(long)]
        no_livereload: bool,
    },
    /// Print the licenses of everything compiled into this binary.
    Licenses,
}

#[derive(clap::Args)]
struct Paths {
    /// Source directory.
    #[arg(short = 's', long = "source", default_value = ".")]
    source: PathBuf,
    /// Destination directory.
    #[arg(short = 'd', long = "destination")]
    destination: Option<PathBuf>,
}

impl Paths {
    fn destination(&self) -> PathBuf {
        self.destination.clone().unwrap_or_else(|| self.source.join("_site"))
    }
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Licenses => {
            // Ignore a closed pipe (`rekyll licenses | head`).
            use std::io::Write;
            let _ = std::io::stdout().write_all(include_bytes!("../THIRD_PARTY_LICENSES.md"));
            Ok(())
        }
        Command::Build { paths } => rekyll::build::build(&paths.source, &paths.destination()),

        #[cfg(feature = "serve")]
        Command::Serve { paths, host, port, skip_initial_build, no_watch, no_livereload } => {
            let dest = paths.destination();
            if !skip_initial_build {
                rekyll::build::build(&paths.source, &dest)?;
            }
            // The baseurl decides what prefix the served URLs carry.
            let site = rekyll::site::Site::new(&paths.source, &dest)?;
            let watch = (!no_watch).then_some(rekyll::serve::Watch {
                source: &paths.source,
                destination: &dest,
            });
            rekyll::serve::serve(rekyll::serve::Options {
                root: &dest,
                host: &host,
                port,
                baseurl: site.config.str("baseurl"),
                watch,
                livereload: !no_livereload,
            })
        }
    }
}
