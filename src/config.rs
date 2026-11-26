//! Configuration model for the v0.0.1 TOML contract.
//!
//! This module defines only the *shape* of a configuration file: the Serde
//! types that accept the documented fields and reject everything else. It
//! performs no file I/O and no semantic validation.
//!
//! Scalars stay in their transport form (`String`) here. Turning them into
//! `SocketAddr`, `Url`, and `Method` is the validator's job in M01-04, so an
//! invalid address is a reported validation error rather than a deserialization
//! failure with no field context.
//!
//! The normative contract is
//! `versions/connduit-v0.0.1-engineering-pack/docs/CONFIGURATION.md`.

use serde::Deserialize;

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

#[cfg(test)]
mod tests {
    use super::AppConfig;

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
}
