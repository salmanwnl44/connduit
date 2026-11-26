//! Connduit binary entry point.
//!
//! This is the composition root: parse arguments, build dependencies, run, and
//! shut down. Gateway behavior lives in the library, not here.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

/// A small configurable HTTP reverse proxy.
#[derive(Debug, Parser)]
#[command(name = "connduit")]
struct Cli {
    /// Path to the TOML configuration file.
    ///
    /// There is no default path and no environment-variable interpolation.
    #[arg(long, value_name = "PATH")]
    config: PathBuf,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match connduit::run(&cli.config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("connduit: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use clap::error::ErrorKind;

    use super::Cli;

    #[test]
    fn parses_the_config_path() {
        let cli = Cli::try_parse_from(["connduit", "--config", "config/connduit.toml"])
            .expect("a valid invocation must parse");

        assert_eq!(cli.config.to_string_lossy(), "config/connduit.toml");
    }

    #[test]
    fn requires_the_config_argument() {
        let error = Cli::try_parse_from(["connduit"]).expect_err("--config is required");

        assert_eq!(error.kind(), ErrorKind::MissingRequiredArgument);
    }

    #[test]
    fn exposes_help() {
        let error = Cli::try_parse_from(["connduit", "--help"]).expect_err("--help exits early");

        assert_eq!(error.kind(), ErrorKind::DisplayHelp);
    }

    #[test]
    fn rejects_unknown_arguments() {
        let error = Cli::try_parse_from(["connduit", "--config", "a.toml", "--listen", "x"])
            .expect_err("unknown arguments must be rejected");

        assert_eq!(error.kind(), ErrorKind::UnknownArgument);
    }
}
