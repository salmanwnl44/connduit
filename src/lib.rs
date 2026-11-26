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

use std::process::ExitCode;

/// Runs the gateway process.
///
/// # Current behavior
///
/// The gateway is not implemented yet, so this writes a diagnostic to standard
/// error and returns [`ExitCode::FAILURE`]. A zero exit code would claim a
/// working gateway where none exists.
///
/// This becomes the composition root in M03, once configuration loading and the
/// listener exist.
pub fn run() -> ExitCode {
    eprintln!("connduit: the gateway is not implemented yet; no listener was started");
    ExitCode::FAILURE
}
