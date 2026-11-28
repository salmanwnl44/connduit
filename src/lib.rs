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
pub mod routing;

use std::path::Path;
use std::sync::Arc;

/// Process-wide state shared by every request.
///
/// Built once at startup from a validated configuration and one HTTP client.
/// Cloning is cheap: the configuration is behind an [`Arc`], the route table
/// holds no request state, and [`reqwest::Client`] is a handle.
#[derive(Debug, Clone)]
pub struct AppState {
    config: Arc<config::ValidatedConfig>,
    routes: routing::RouteTable,
    client: reqwest::Client,
}

impl AppState {
    /// Builds the state from a validated configuration and a shared client.
    ///
    /// The route table is derived here, once, so request handling never walks
    /// the configuration.
    #[must_use]
    pub fn new(config: config::ValidatedConfig, client: reqwest::Client) -> Self {
        let routes = config
            .routes
            .iter()
            .map(|route| routing::Route {
                id: route.id.clone(),
                path: route.path.clone(),
                upstream: route.upstream.clone(),
                methods: route.methods.clone(),
            })
            .collect();

        Self {
            config: Arc::new(config),
            routes: routing::RouteTable::new(routes),
            client,
        }
    }

    /// Returns the immutable validated configuration.
    #[must_use]
    pub fn config(&self) -> &config::ValidatedConfig {
        &self.config
    }

    /// Returns the immutable route table.
    #[must_use]
    pub fn routes(&self) -> &routing::RouteTable {
        &self.routes
    }

    /// Returns the shared upstream client.
    #[must_use]
    pub fn client(&self) -> &reqwest::Client {
        &self.client
    }
}

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
    /// The upstream HTTP client could not be built.
    #[error("failed to build the upstream HTTP client: {0}")]
    Client(#[from] reqwest::Error),
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
/// [`StartupError::Invalid`] when it fails a semantic rule,
/// [`StartupError::Client`] when the HTTP client cannot be built, and
/// [`StartupError::ListenerNotImplemented`] until M03 lands. Nothing is served
/// on any path; a zero exit is never returned yet.
pub fn run(config_path: &Path) -> Result<(), StartupError> {
    let config = config::load_config(config_path)?;
    let validated = config::validate_config(config).map_err(StartupError::Invalid)?;

    // One client for the process lifetime. Its builder options are refined by
    // M04-01; building it here already proves it can be constructed before any
    // request arrives.
    let client = reqwest::Client::builder().build()?;

    // Constructed once, then dropped because there is no listener yet. M03-02
    // takes ownership of this value instead of discarding it.
    let _state = AppState::new(validated, client);

    Err(StartupError::ListenerNotImplemented)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use http::Method;

    use super::{AppState, StartupError, run};
    use crate::config::{ValidatedConfig, ValidatedRoute, ValidatedUpstream};

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

    fn validated() -> ValidatedConfig {
        ValidatedConfig {
            address: "127.0.0.1:8080".parse().expect("the test address parses"),
            upstreams: vec![ValidatedUpstream {
                id: "users".to_owned(),
                url: url::Url::parse("http://127.0.0.1:3001").expect("the test URL parses"),
            }],
            routes: vec![ValidatedRoute {
                id: "users-api".to_owned(),
                path: "/api/users".to_owned(),
                methods: vec![Method::GET],
                upstream: "users".to_owned(),
            }],
        }
    }

    fn client() -> reqwest::Client {
        reqwest::Client::builder()
            .build()
            .expect("the test client builds")
    }

    #[test]
    fn state_is_built_from_a_validated_configuration() {
        let state = AppState::new(validated(), client());

        assert_eq!(state.routes().len(), 1);
        assert!(state.routes().route_for("/api/users/7").is_some());
        assert!(state.routes().route_for("/other").is_none());
        assert_eq!(state.config().address.to_string(), "127.0.0.1:8080");
        assert_eq!(state.config().upstreams[0].id, "users");
    }

    #[test]
    fn state_resolves_methods_through_the_route_table() {
        let state = AppState::new(validated(), client());

        assert!(matches!(
            state.routes().resolve("/api/users", &Method::GET),
            crate::routing::RouteMatch::Matched(_)
        ));
        assert!(matches!(
            state.routes().resolve("/api/users", &Method::POST),
            crate::routing::RouteMatch::MethodNotAllowed { .. }
        ));
        assert!(matches!(
            state.routes().resolve("/elsewhere", &Method::GET),
            crate::routing::RouteMatch::NotFound
        ));
    }

    #[test]
    fn cloning_state_shares_the_client_and_configuration() {
        let state = AppState::new(validated(), client());
        let clone = state.clone();

        // Pointer equality proves the configuration was not duplicated: both
        // handles dereference to the same `Arc` target.
        assert!(std::ptr::eq(state.config(), clone.config()));
        assert_eq!(clone.routes().len(), state.routes().len());
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
                    errors.iter().any(|error| error.field == "version"),
                    "error must name the field, got {errors:?}"
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
}
