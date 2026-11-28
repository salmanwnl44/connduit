//! Listener tests: bind failures and a real request over TCP.
//!
//! These run against a loopback port assigned by the OS, so they never collide
//! with a developer's services and never touch the public internet.

use std::path::PathBuf;

use connduit::config::{ValidatedConfig, ValidatedRoute, ValidatedUpstream};
use connduit::{AppState, StartupError, run, serve};
use http::Method;
use tokio::net::TcpListener;

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

/// A temporary configuration file that removes itself when the test ends.
struct TempConfig(PathBuf);

impl TempConfig {
    fn new(name: &str, contents: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "connduit-server-{}-{name}.toml",
            std::process::id()
        ));
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
        address: "127.0.0.1:0".parse().expect("the test address parses"),
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

#[tokio::test]
async fn an_occupied_port_is_a_startup_error() {
    // Hold a port for the duration of the test.
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").expect("bind a port");
    let address = occupied.local_addr().expect("read the local address");
    let file = TempConfig::new(
        "occupied",
        &VALID.replace("127.0.0.1:8080", &address.to_string()),
    );

    let error = run(&file.0)
        .await
        .expect_err("binding an occupied port must fail");

    match error {
        StartupError::Bind {
            address: reported, ..
        } => assert_eq!(reported, address),
        other => panic!("expected Bind, got: {other}"),
    }
}

#[tokio::test]
async fn a_request_reaches_the_handler() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind an ephemeral port");
    let address = listener.local_addr().expect("read the local address");

    let server = tokio::spawn(serve(listener, AppState::new(validated(), client())));

    let response = reqwest::get(format!("http://{address}/api/users"))
        .await
        .expect("the server must answer");

    // The placeholder handler answers 501 until M03-03 wires the router.
    assert_eq!(response.status(), reqwest::StatusCode::NOT_IMPLEMENTED);

    server.abort();
}

#[tokio::test]
async fn the_server_binds_the_configured_address() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind an ephemeral port");
    let address = listener.local_addr().expect("read the local address");

    let server = tokio::spawn(serve(listener, AppState::new(validated(), client())));

    let response = reqwest::get(format!("http://{address}/anything/else"))
        .await
        .expect("the server must answer");

    assert_eq!(response.status(), reqwest::StatusCode::NOT_IMPLEMENTED);

    server.abort();
}
