//! Configuration model and loader for the v0.0.1 contract.
//!
//! This module defines the shape of a configuration file and reads it from
//! disk. Semantic validation lives in [`validation`] and runs after loading,
//! before the listener binds.
//!
//! Scalars stay in their transport form (`String`) here. Converting them into
//! `SocketAddr`, `Url`, and `Method` is the validator's job, so a bad value is
//! reported against its field path rather than as an opaque parse failure.
//!
//! The normative contract is
//! `versions/connduit-v0.0.1-engineering-pack/docs/CONFIGURATION.md`.

use std::path::{Path, PathBuf};

use serde::Deserialize;

pub mod validation;

pub use validation::{
    ValidatedConfig, ValidatedRoute, ValidatedUpstream, ValidationError, validate_config,
};

/// Root of a Connduit configuration file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    /// Schema version of the file. The only supported value is `1`.
    pub version: u32,
    /// Listener settings. Required.
    pub server: ServerConfig,
    /// Upstream origins. At least one is required after validation.
    pub upstreams: Vec<UpstreamConfig>,
    /// Routes. At least one is required after validation.
    pub routes: Vec<RouteConfig>,
}

/// The `[server]` table.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    /// Listener address, for example `127.0.0.1:8080`. There is no default.
    pub address: String,
}

/// One `[[upstreams]]` entry.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpstreamConfig {
    /// Stable identifier referenced by routes. Must be non-empty and unique.
    pub id: String,
    /// Absolute origin-only `http://` URL, for example `http://127.0.0.1:3001`.
    pub url: String,
}

/// One `[[routes]]` entry.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteConfig {
    /// Stable identifier. Must be non-empty and unique.
    pub id: String,
    /// Absolute route prefix beginning with `/`, without a trailing slash
    /// unless it is exactly `/`.
    pub path: String,
    /// Accepted HTTP method tokens. Must be non-empty and valid.
    pub methods: Vec<String>,
    /// Identifier of the upstream that serves this route.
    pub upstream: String,
}

/// A failure while loading a configuration file.
///
/// Every variant carries the path so the diagnostic can be acted on without
/// guessing which file was meant.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// The requested file does not exist.
    #[error("configuration file `{path}` does not exist")]
    NotFound {
        /// Path that was requested.
        path: PathBuf,
    },
    /// The file exists but could not be read.
    #[error("cannot read configuration file `{path}`: {source}")]
    Read {
        /// Path that was requested.
        path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: std::io::Error,
    },
    /// The contents are not valid TOML.
    #[error("configuration file `{path}` is not valid TOML: {message}")]
    Syntax {
        /// Path that was requested.
        path: PathBuf,
        /// Parser diagnostic.
        message: String,
    },
    /// The document defines a field the schema does not allow.
    #[error("configuration file `{path}` has an unknown field: {message}")]
    UnknownField {
        /// Path that was requested.
        path: PathBuf,
        /// Deserializer diagnostic naming the field.
        message: String,
    },
    /// The document is valid TOML but does not match the schema.
    #[error("configuration file `{path}` does not match the schema: {message}")]
    Invalid {
        /// Path that was requested.
        path: PathBuf,
        /// Deserializer diagnostic naming the field.
        message: String,
    },
}

/// Reads and deserializes a configuration file.
///
/// This performs blocking file I/O and is intended for startup only, never from
/// a request path. It does not validate: call
/// [`validate_config`] on the result.
///
/// # Errors
///
/// Returns [`ConfigError`] when the file is missing, cannot be read, is not
/// valid TOML, defines an unknown field, or does not match the schema.
pub fn load_config(path: &Path) -> Result<AppConfig, ConfigError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            return Err(ConfigError::NotFound {
                path: path.to_path_buf(),
            });
        }
        Err(source) => {
            return Err(ConfigError::Read {
                path: path.to_path_buf(),
                source,
            });
        }
    };

    // Parse once for syntax and once for schema shape, so a malformed file and a
    // schema violation remain distinguishable typed errors. `toml` reports both
    // as the same error type, so the split happens here rather than by matching
    // an error message in the caller. Startup only, so the second parse is free.
    toml::from_str::<toml::Value>(&text).map_err(|error| ConfigError::Syntax {
        path: path.to_path_buf(),
        message: error.message().to_owned(),
    })?;

    toml::from_str::<AppConfig>(&text).map_err(|error| {
        let message = error.message().to_owned();
        let path = path.to_path_buf();

        if message.starts_with("unknown field") {
            ConfigError::UnknownField { path, message }
        } else {
            ConfigError::Invalid { path, message }
        }
    })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{AppConfig, ConfigError, load_config};

    const EXAMPLE: &str = r#"
version = 1

[server]
address = "127.0.0.1:8080"

[[upstreams]]
id = "users"
url = "http://127.0.0.1:3001"

[[upstreams]]
id = "catalog"
url = "http://127.0.0.1:3002"

[[routes]]
id = "users-api"
path = "/api/users"
methods = ["GET", "POST"]
upstream = "users"

[[routes]]
id = "catalog-api"
path = "/api/catalog"
methods = ["GET"]
upstream = "catalog"
"#;

    /// A temporary file that removes itself when the test ends, so a failed
    /// assertion cannot leave artifacts behind.
    struct TempConfig(PathBuf);

    impl TempConfig {
        fn new(name: &str, contents: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("connduit-test-{}-{name}.toml", std::process::id()));
            std::fs::write(&path, contents).expect("write temporary config");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempConfig {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn absent_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "connduit-absent-{}-{name}.toml",
            std::process::id()
        ))
    }

    #[test]
    fn parses_the_documented_example() {
        let config: AppConfig = toml::from_str(EXAMPLE).expect("example must deserialize");

        assert_eq!(config.version, 1);
        assert_eq!(config.server.address, "127.0.0.1:8080");
        assert_eq!(config.upstreams.len(), 2);
        assert_eq!(config.upstreams[0].id, "users");
        assert_eq!(config.upstreams[1].url, "http://127.0.0.1:3002");
        assert_eq!(config.routes.len(), 2);
        assert_eq!(config.routes[0].methods, ["GET", "POST"]);
        assert_eq!(config.routes[1].upstream, "catalog");
    }

    #[test]
    fn rejects_an_unknown_root_field() {
        let toml = format!("{EXAMPLE}\nunexpected = true\n");
        let error = toml::from_str::<AppConfig>(&toml).expect_err("unknown field must be rejected");

        assert!(
            error.to_string().contains("unexpected"),
            "error must name the offending field, got: {error}"
        );
    }

    #[test]
    fn rejects_an_unknown_nested_field() {
        let toml = EXAMPLE.replace(
            "address = \"127.0.0.1:8080\"",
            "address = \"127.0.0.1:8080\"\nbacklog = 128",
        );
        let error = toml::from_str::<AppConfig>(&toml).expect_err("unknown field must be rejected");

        assert!(
            error.to_string().contains("backlog"),
            "error must name the offending field, got: {error}"
        );
    }

    #[test]
    fn rejects_a_missing_required_field() {
        let toml = EXAMPLE.replace("upstream = \"catalog\"\n", "");
        let error = toml::from_str::<AppConfig>(&toml).expect_err("missing field must be rejected");

        assert!(
            error.to_string().contains("upstream"),
            "error must name the missing field, got: {error}"
        );
    }

    #[test]
    fn loads_a_valid_file() {
        let file = TempConfig::new("valid", EXAMPLE);
        let config = load_config(file.path()).expect("valid config must load");

        assert_eq!(config.server.address, "127.0.0.1:8080");
        assert_eq!(config.routes.len(), 2);
    }

    #[test]
    fn reports_a_missing_file_as_not_found() {
        let path = absent_path("missing");
        let error = load_config(&path).expect_err("missing file must fail");

        assert!(
            matches!(error, ConfigError::NotFound { .. }),
            "expected NotFound, got: {error}"
        );
    }

    #[test]
    fn reports_malformed_toml_as_a_syntax_error() {
        let file = TempConfig::new("malformed", "version = \n[server\naddress = 1");
        let error = load_config(file.path()).expect_err("malformed TOML must fail");

        assert!(
            matches!(error, ConfigError::Syntax { .. }),
            "expected Syntax, got: {error}"
        );
    }

    #[test]
    fn reports_an_unknown_field_as_its_own_error() {
        let file = TempConfig::new("unknown", &format!("{EXAMPLE}\nunexpected = true\n"));
        let error = load_config(file.path()).expect_err("unknown field must fail");

        assert!(
            matches!(error, ConfigError::UnknownField { .. }),
            "expected UnknownField, got: {error}"
        );
        assert!(
            error.to_string().contains("unexpected"),
            "error must name the offending field, got: {error}"
        );
    }

    #[test]
    fn reports_a_schema_violation_as_invalid() {
        let file = TempConfig::new("wrong-type", "version = \"one\"\n[server]\naddress = 1");
        let error = load_config(file.path()).expect_err("wrong types must fail");

        assert!(
            matches!(error, ConfigError::Invalid { .. }),
            "expected Invalid, got: {error}"
        );
    }

    #[test]
    fn every_error_names_the_file() {
        let file = TempConfig::new("named", "not toml");
        let error = load_config(file.path()).expect_err("malformed TOML must fail");
        let shown = file.path().display().to_string();

        assert!(
            error.to_string().contains(&shown),
            "error must name the file, got: {error}"
        );
    }
}
