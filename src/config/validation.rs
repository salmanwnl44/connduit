//! Semantic validation for the configuration contract.
//!
//! Splitting this out of `config` follows the change-trigger in
//! `docs/implementation/MODULE_MAP.md` §8: the loader and the validator change
//! for different reasons and together exceeded ~400 non-test lines.
//!
//! Validation converts the raw strings of [`AppConfig`] into typed values
//! (`SocketAddr`, `Url`, `Method`) and enforces `CONFIGURATION.md` §4. It
//! performs no network I/O and no DNS resolution.

use std::net::SocketAddr;

use http::Method;
use url::Url;

use super::AppConfig;

/// The only configuration schema version this build accepts.
const SUPPORTED_VERSION: u32 = 1;

/// A configuration that passed every validation rule.
///
/// Every value is parsed, so request handling never re-parses the
/// configuration.
#[derive(Debug)]
pub struct ValidatedConfig {
    /// Listener address that parsed as a socket address.
    pub address: SocketAddr,
    /// Upstreams with parsed origin URLs, in file order.
    pub upstreams: Vec<ValidatedUpstream>,
    /// Routes with parsed methods, in file order.
    pub routes: Vec<ValidatedRoute>,
}

/// An upstream whose origin URL is parsed and origin-only.
#[derive(Debug)]
pub struct ValidatedUpstream {
    /// Identifier referenced by routes.
    pub id: String,
    /// Parsed origin URL. The path is always `/`.
    pub url: Url,
}

/// A route whose path, methods, and upstream reference are valid.
#[derive(Debug)]
pub struct ValidatedRoute {
    /// Stable identifier.
    pub id: String,
    /// Absolute route prefix, matched case-sensitively.
    pub path: String,
    /// Parsed methods.
    pub methods: Vec<Method>,
    /// Identifier of the upstream that serves this route.
    pub upstream: String,
}

/// A semantic problem with a configuration value.
///
/// The field path uses the same indexing as the file, for example
/// `routes[1].upstream`, so the operator can locate it without counting.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{field}: {message}")]
pub struct ValidationError {
    /// Dotted path to the offending field.
    pub field: String,
    /// What is wrong, and what is accepted where useful.
    pub message: String,
}

impl ValidationError {
    fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

/// Applies every semantic rule and returns a parsed, immutable configuration.
///
/// All rules are checked before returning, so the operator sees every problem in
/// one pass instead of fixing them one at a time.
///
/// # Errors
///
/// Returns every [`ValidationError`] found. The list is never empty on failure.
pub fn validate_config(config: AppConfig) -> Result<ValidatedConfig, Vec<ValidationError>> {
    let mut errors = Vec::new();

    if config.version != SUPPORTED_VERSION {
        errors.push(ValidationError::new(
            "version",
            format!(
                "unsupported version {}; expected {SUPPORTED_VERSION}",
                config.version
            ),
        ));
    }

    let address = match config.server.address.parse::<SocketAddr>() {
        Ok(address) => Some(address),
        Err(_) => {
            errors.push(ValidationError::new(
                "server.address",
                format!(
                    "invalid socket address \"{}\"; expected host:port",
                    config.server.address
                ),
            ));
            None
        }
    };

    if config.upstreams.is_empty() {
        errors.push(ValidationError::new(
            "upstreams",
            "at least one upstream is required",
        ));
    }
    if config.routes.is_empty() {
        errors.push(ValidationError::new(
            "routes",
            "at least one route is required",
        ));
    }

    let mut upstream_ids: Vec<&str> = Vec::new();
    let mut upstreams = Vec::with_capacity(config.upstreams.len());

    for (index, upstream) in config.upstreams.iter().enumerate() {
        if upstream.id.is_empty() {
            errors.push(ValidationError::new(
                format!("upstreams[{index}].id"),
                "must not be empty",
            ));
        } else if upstream_ids.contains(&upstream.id.as_str()) {
            errors.push(ValidationError::new(
                format!("upstreams[{index}].id"),
                format!("duplicate upstream id \"{}\"", upstream.id),
            ));
        } else {
            upstream_ids.push(upstream.id.as_str());
        }

        match parse_origin(&upstream.url) {
            Ok(url) => upstreams.push(ValidatedUpstream {
                id: upstream.id.clone(),
                url,
            }),
            Err(message) => errors.push(ValidationError::new(
                format!("upstreams[{index}].url"),
                message,
            )),
        }
    }

    let mut route_ids: Vec<&str> = Vec::new();
    let mut route_paths: Vec<&str> = Vec::new();
    let mut routes = Vec::with_capacity(config.routes.len());

    for (index, route) in config.routes.iter().enumerate() {
        if route.id.is_empty() {
            errors.push(ValidationError::new(
                format!("routes[{index}].id"),
                "must not be empty",
            ));
        } else if route_ids.contains(&route.id.as_str()) {
            errors.push(ValidationError::new(
                format!("routes[{index}].id"),
                format!("duplicate route id \"{}\"", route.id),
            ));
        } else {
            route_ids.push(route.id.as_str());
        }

        validate_path(index, &route.path, &mut route_paths, &mut errors);
        let methods = validate_methods(index, &route.methods, &mut errors);

        if !upstream_ids.contains(&route.upstream.as_str()) {
            errors.push(ValidationError::new(
                format!("routes[{index}].upstream"),
                format!("references unknown upstream \"{}\"", route.upstream),
            ));
        }

        routes.push(ValidatedRoute {
            id: route.id.clone(),
            path: route.path.clone(),
            methods,
            upstream: route.upstream.clone(),
        });
    }

    if !errors.is_empty() {
        return Err(errors);
    }

    // `address` is only `None` when an error was recorded above, so this branch
    // is unreachable after the check. It returns instead of panicking.
    let Some(address) = address else {
        return Err(vec![ValidationError::new(
            "server.address",
            "invalid socket address",
        )]);
    };

    Ok(ValidatedConfig {
        address,
        upstreams,
        routes,
    })
}

/// Enforces the route path rules and records duplicate prefixes.
fn validate_path<'a>(
    index: usize,
    path: &'a str,
    seen: &mut Vec<&'a str>,
    errors: &mut Vec<ValidationError>,
) {
    let field = || format!("routes[{index}].path");

    if !path.starts_with('/') {
        errors.push(ValidationError::new(field(), "must begin with '/'"));
        return;
    }

    if path.contains('?') || path.contains('#') {
        errors.push(ValidationError::new(
            field(),
            "must not contain a query or fragment",
        ));
    }

    if path != "/" && path.ends_with('/') {
        errors.push(ValidationError::new(
            field(),
            "must not end with '/'; only the root path may be exactly \"/\"",
        ));
    }

    if seen.contains(&path) {
        errors.push(ValidationError::new(
            field(),
            format!("duplicate route prefix \"{path}\""),
        ));
    } else {
        seen.push(path);
    }
}

/// Parses every method token, recording one error per invalid token.
fn validate_methods(
    index: usize,
    tokens: &[String],
    errors: &mut Vec<ValidationError>,
) -> Vec<Method> {
    if tokens.is_empty() {
        errors.push(ValidationError::new(
            format!("routes[{index}].methods"),
            "at least one method is required",
        ));
        return Vec::new();
    }

    let mut methods = Vec::with_capacity(tokens.len());
    for token in tokens {
        match Method::from_bytes(token.as_bytes()) {
            Ok(method) => methods.push(method),
            Err(_) => errors.push(ValidationError::new(
                format!("routes[{index}].methods"),
                format!("invalid HTTP method \"{token}\""),
            )),
        }
    }
    methods
}

/// Parses an upstream URL and enforces the origin-only rule.
fn parse_origin(value: &str) -> Result<Url, String> {
    let url = Url::parse(value).map_err(|error| format!("invalid URL: {error}"))?;

    if url.scheme() != "http" {
        return Err(format!(
            "scheme must be \"http\", found \"{}\"",
            url.scheme()
        ));
    }
    if url.host_str().is_none() {
        return Err("host is required".to_owned());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("must not contain user information".to_owned());
    }
    if url.path() != "/" {
        return Err(format!(
            "must be origin only; found path \"{}\"",
            url.path()
        ));
    }
    if url.query().is_some() {
        return Err("must not contain a query".to_owned());
    }
    if url.fragment().is_some() {
        return Err("must not contain a fragment".to_owned());
    }

    Ok(url)
}

#[cfg(test)]
mod tests {
    use http::Method;

    use super::{ValidationError, validate_config};
    use crate::config::AppConfig;

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

    fn valid_config() -> AppConfig {
        toml::from_str(EXAMPLE).expect("example must deserialize")
    }

    fn errors_of(config: AppConfig) -> Vec<ValidationError> {
        validate_config(config).expect_err("configuration must be rejected")
    }

    fn has_field(errors: &[ValidationError], field: &str) -> bool {
        errors.iter().any(|error| error.field == field)
    }

    #[test]
    fn accepts_the_documented_example() {
        let validated = validate_config(valid_config()).expect("example must validate");

        assert_eq!(validated.address.to_string(), "127.0.0.1:8080");
        assert_eq!(validated.upstreams.len(), 2);
        assert_eq!(
            validated.upstreams[0].url.as_str(),
            "http://127.0.0.1:3001/"
        );
        assert_eq!(validated.routes[0].methods, [Method::GET, Method::POST]);
        assert_eq!(validated.routes[1].upstream, "catalog");
    }

    #[test]
    fn root_path_is_allowed() {
        let mut config = valid_config();
        config.routes[0].path = "/".to_owned();
        config.routes[1].path = "/api".to_owned();

        assert!(validate_config(config).is_ok());
    }

    #[test]
    fn accepts_an_upstream_with_a_trailing_slash() {
        let mut config = valid_config();
        config.upstreams[0].url = "http://users.internal:8080/".to_owned();

        assert!(validate_config(config).is_ok());
    }

    #[test]
    fn reports_every_problem_at_once() {
        let mut config = valid_config();
        config.version = 2;
        config.server.address = "not-an-address".to_owned();
        config.routes[0].methods.clear();

        let errors = errors_of(config);

        assert!(has_field(&errors, "version"));
        assert!(has_field(&errors, "server.address"));
        assert!(has_field(&errors, "routes[0].methods"));
        assert!(errors.len() >= 3, "expected all problems, got {errors:?}");
    }

    #[test]
    fn rejects_an_unsupported_version() {
        let mut config = valid_config();
        config.version = 2;
        assert!(has_field(&errors_of(config), "version"));
    }

    #[test]
    fn rejects_an_invalid_listener_address() {
        let mut config = valid_config();
        config.server.address = "localhost".to_owned();
        assert!(has_field(&errors_of(config), "server.address"));
    }

    #[test]
    fn rejects_an_empty_upstream_list() {
        let mut config = valid_config();
        config.upstreams.clear();
        assert!(has_field(&errors_of(config), "upstreams"));
    }

    #[test]
    fn rejects_an_empty_route_list() {
        let mut config = valid_config();
        config.routes.clear();
        assert!(has_field(&errors_of(config), "routes"));
    }

    #[test]
    fn rejects_an_empty_upstream_id() {
        let mut config = valid_config();
        config.upstreams[0].id.clear();
        assert!(has_field(&errors_of(config), "upstreams[0].id"));
    }

    #[test]
    fn rejects_a_duplicate_upstream_id() {
        let mut config = valid_config();
        config.upstreams[1].id = "users".to_owned();
        assert!(has_field(&errors_of(config), "upstreams[1].id"));
    }

    #[test]
    fn rejects_an_empty_route_id() {
        let mut config = valid_config();
        config.routes[0].id.clear();
        assert!(has_field(&errors_of(config), "routes[0].id"));
    }

    #[test]
    fn rejects_a_duplicate_route_id() {
        let mut config = valid_config();
        config.routes[1].id = "users-api".to_owned();
        assert!(has_field(&errors_of(config), "routes[1].id"));
    }

    #[test]
    fn rejects_a_path_without_a_leading_slash() {
        let mut config = valid_config();
        config.routes[0].path = "api/users".to_owned();
        assert!(has_field(&errors_of(config), "routes[0].path"));
    }

    #[test]
    fn rejects_a_non_root_path_with_a_trailing_slash() {
        let mut config = valid_config();
        config.routes[0].path = "/api/users/".to_owned();
        assert!(has_field(&errors_of(config), "routes[0].path"));
    }

    #[test]
    fn rejects_a_path_with_a_query() {
        let mut config = valid_config();
        config.routes[0].path = "/api/users?active=1".to_owned();
        assert!(has_field(&errors_of(config), "routes[0].path"));
    }

    #[test]
    fn rejects_a_path_with_a_fragment() {
        let mut config = valid_config();
        config.routes[0].path = "/api/users#top".to_owned();
        assert!(has_field(&errors_of(config), "routes[0].path"));
    }

    #[test]
    fn rejects_empty_methods() {
        let mut config = valid_config();
        config.routes[0].methods.clear();
        assert!(has_field(&errors_of(config), "routes[0].methods"));
    }

    #[test]
    fn rejects_an_invalid_method_token() {
        let mut config = valid_config();
        config.routes[0].methods = vec!["GE T".to_owned()];
        assert!(has_field(&errors_of(config), "routes[0].methods"));
    }

    #[test]
    fn rejects_a_duplicate_route_prefix() {
        let mut config = valid_config();
        config.routes[1].path = config.routes[0].path.clone();
        assert!(has_field(&errors_of(config), "routes[1].path"));
    }

    #[test]
    fn rejects_an_unknown_upstream_reference() {
        let mut config = valid_config();
        config.routes[0].upstream = "missing".to_owned();
        assert!(has_field(&errors_of(config), "routes[0].upstream"));
    }

    #[test]
    fn rejects_a_non_http_upstream_scheme() {
        let mut config = valid_config();
        config.upstreams[0].url = "https://users.internal".to_owned();
        assert!(has_field(&errors_of(config), "upstreams[0].url"));
    }

    #[test]
    fn rejects_an_unparseable_upstream_url() {
        let mut config = valid_config();
        config.upstreams[0].url = "not a url".to_owned();
        assert!(has_field(&errors_of(config), "upstreams[0].url"));
    }

    #[test]
    fn rejects_upstream_user_information() {
        let mut config = valid_config();
        config.upstreams[0].url = "http://user:pass@users.internal".to_owned();
        assert!(has_field(&errors_of(config), "upstreams[0].url"));
    }

    #[test]
    fn rejects_an_upstream_with_a_base_path() {
        let mut config = valid_config();
        config.upstreams[0].url = "http://users.internal/base".to_owned();
        assert!(has_field(&errors_of(config), "upstreams[0].url"));
    }

    #[test]
    fn rejects_an_upstream_with_a_query() {
        let mut config = valid_config();
        config.upstreams[0].url = "http://users.internal?mode=fast".to_owned();
        assert!(has_field(&errors_of(config), "upstreams[0].url"));
    }

    #[test]
    fn rejects_an_upstream_with_a_fragment() {
        let mut config = valid_config();
        config.upstreams[0].url = "http://users.internal#top".to_owned();
        assert!(has_field(&errors_of(config), "upstreams[0].url"));
    }
}
