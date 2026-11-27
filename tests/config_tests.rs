//! Fixture-driven tests for the configuration contract.
//!
//! Each fixture is a complete TOML document. Valid fixtures must load and
//! validate; each invalid fixture must be rejected with an error that names the
//! offending field path.
//!
//! Fixtures live in `tests/fixtures/` and are read from disk, so they exercise
//! the same loader path the binary uses.

use std::path::{Path, PathBuf};

use connduit::config::{load_config, validate_config};

/// Valid fixtures that must load and validate.
const VALID: &[&str] = &["valid-one-route.toml", "valid-two-routes.toml"];

/// Invalid fixtures paired with the field path the error must name.
const INVALID: &[(&str, &str)] = &[
    ("invalid-version.toml", "version"),
    ("invalid-address.toml", "server.address"),
    ("no-upstreams.toml", "upstreams"),
    ("no-routes.toml", "routes"),
    ("empty-upstream-id.toml", "upstreams[0].id"),
    ("duplicate-upstream-id.toml", "upstreams[1].id"),
    ("empty-route-id.toml", "routes[0].id"),
    ("duplicate-route-id.toml", "routes[1].id"),
    ("path-no-slash.toml", "routes[0].path"),
    ("path-trailing-slash.toml", "routes[0].path"),
    ("path-query.toml", "routes[0].path"),
    ("duplicate-prefix.toml", "routes[1].path"),
    ("empty-methods.toml", "routes[0].methods"),
    ("invalid-method.toml", "routes[1].methods"),
    ("unknown-upstream-ref.toml", "routes[0].upstream"),
    ("upstream-https.toml", "upstreams[0].url"),
    ("upstream-credentials.toml", "upstreams[0].url"),
    ("upstream-path.toml", "upstreams[0].url"),
    ("upstream-query.toml", "upstreams[0].url"),
];

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

#[test]
fn valid_fixtures_load_and_validate() {
    let mut problems = Vec::new();

    for name in VALID {
        let path = fixtures().join(name);

        let config = match load_config(&path) {
            Ok(config) => config,
            Err(error) => {
                problems.push(format!("{name}: load failed: {error}"));
                continue;
            }
        };

        if let Err(errors) = validate_config(config) {
            problems.push(format!(
                "{name}: validation failed: {:?}",
                errors.iter().map(|e| e.field.as_str()).collect::<Vec<_>>()
            ));
        }
    }

    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn invalid_fixtures_name_their_field() {
    let mut problems = Vec::new();

    for (name, expected) in INVALID {
        let path = fixtures().join(name);

        let config = match load_config(&path) {
            Ok(config) => config,
            Err(error) => {
                problems.push(format!("{name}: load failed before validation: {error}"));
                continue;
            }
        };

        match validate_config(config) {
            Ok(_) => problems.push(format!("{name}: expected rejection, but it validated")),
            Err(errors) => {
                if !errors.iter().any(|error| error.field == *expected) {
                    problems.push(format!(
                        "{name}: expected field `{expected}`, got {:?}",
                        errors.iter().map(|e| e.field.as_str()).collect::<Vec<_>>()
                    ));
                }
            }
        }
    }

    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn every_fixture_is_listed() {
    let mut problems = Vec::new();

    let mut on_disk: Vec<String> = std::fs::read_dir(fixtures())
        .expect("fixtures directory must exist")
        .map(|entry| {
            entry
                .expect("readable entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.ends_with(".toml"))
        .collect();
    on_disk.sort();

    let mut listed: Vec<String> = VALID
        .iter()
        .map(|name| (*name).to_owned())
        .chain(INVALID.iter().map(|(name, _)| (*name).to_owned()))
        .collect();
    listed.sort();

    for name in &on_disk {
        if !listed.contains(name) {
            problems.push(format!("{name} exists on disk but is not listed"));
        }
    }
    for name in &listed {
        if !on_disk.contains(name) {
            problems.push(format!("{name} is listed but missing on disk"));
        }
    }

    assert!(problems.is_empty(), "{problems:#?}");
}
