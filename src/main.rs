//! Connduit binary entry point.
//!
//! This is the composition root: process setup, runtime wiring, and shutdown.
//! Gateway behavior lives in the library, not here. Keep this file thin.

use std::process::ExitCode;

fn main() -> ExitCode {
    connduit::run()
}
