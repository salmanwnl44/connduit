//! Connduit — a small configurable HTTP reverse proxy.
//!
//! This crate is the library surface for configuration, routing, forwarding, and
//! error handling. Normative behavior is defined by the frozen v0.0.1
//! engineering pack under `versions/connduit-v0.0.1-engineering-pack/`.
//!
//! Each module is declared when its behavior lands, so that the public API
//! never advertises an empty module:
//!
//! | Module | Introduced by |
//! |---|---|
//! | `config` | M01 |
//! | `routing` | M02 |
//! | `forwarding` | M04 |
//! | `error` | M05 |

pub mod config;

use std::path::Path;

/// A failure while starting the gateway.
///
/// Startup errors are reported once, in the binary, and always exit non-zero.
#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    /// The configuration file could not be read or parsed.
    #[error(transparent)]
    Config(#[from] config::ConfigError),
    /// The configuration was read but failed validation.
    ///
    /// Every problem is listed, so the operator does not fix them one restart at
    /// a time.
    #[error("configuration is invalid:\n{}", format_validation_errors(.0))]
    Invalid(Vec<config::ValidationError>),
    /// Loading and validation succeeded, but the listener does not exist yet.
    #[error("the HTTP listener is not implemented yet")]
    ListenerNotImplemented,
}

/// Renders validation errors one per line, indented for readability.
fn format_validation_errors(errors: &[config::ValidationError]) -> String {
    errors
        .iter()
        .map(|error| format!("  {error}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Loads and validates the configuration, then starts the gateway.
///
/// # Errors
///
/// Returns [`StartupError::Config`] when the file is missing or malformed,
/// [`StartupError::Invalid`] when it fails a semantic rule, and
/// [`StartupError::ListenerNotImplemented`] until M01 completes and M03 lands.
/// Nothing is served on any path; a zero exit is never returned yet.
pub fn run(config_path: &Path) -> Result<(), StartupError> {
    let config = config::load_config(config_path)?;
    config::validate_config(config).map_err(StartupError::Invalid)?;

    // The listener is introduced by M03. Returning success here would claim a
    // working gateway that does not exist.
    Err(StartupError::ListenerNotImplemented)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{StartupError, run};

    const VALID: &str = r#"
version = 1

[server]
address = "127.0.0.1:8080"

[[upstreams]]
id = "users"
url = "http://127.0.0.1:3001"

[[routes]]
id = "users-api"
path = "/api/users"
methods = ["GET"]
upstream = "users"
"#;

    /// A temporary file that removes itself when the test ends.
    struct TempConfig(PathBuf);

    impl TempConfig {
        fn new(name: &str, contents: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("connduit-run-{}-{name}.toml", std::process::id()));
            std::fs::write(&path, contents).expect("write temporary config");
            Self(path)
        }
    }

    impl Drop for TempConfig {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn reports_a_missing_configuration_file() {
        let path =
            std::env::temp_dir().join(format!("connduit-run-absent-{}.toml", std::process::id()));

        let error = run(&path).expect_err("a missing file must fail");

        assert!(
            matches!(
                error,
                StartupError::Config(super::config::ConfigError::NotFound { .. })
            ),
            "expected NotFound, got: {error}"
        );
    }

    #[test]
    fn reports_invalid_configuration() {
        let file = TempConfig::new("invalid", &VALID.replace("version = 1", "version = 2"));

        let error = run(&file.0).expect_err("version 2 must be rejected");

        match error {
            StartupError::Invalid(errors) => {
                assert!(!errors.is_empty(), "validation must report a field");
                assert!(
                    error_body(&errors).contains("version"),
                    "error must name the field"
                );
            }
            other => panic!("expected Invalid, got: {other}"),
        }
    }

    #[test]
    fn a_valid_configuration_reaches_the_listener_gap() {
        let file = TempConfig::new("valid", VALID);

        let error = run(&file.0).expect_err("the listener is not implemented");

        assert!(
            matches!(error, StartupError::ListenerNotImplemented),
            "expected ListenerNotImplemented, got: {error}"
        );
    }

    fn error_body(errors: &[super::config::ValidationError]) -> String {
        errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    }
}
