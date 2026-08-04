//! PawDesk entry point.

#![allow(dead_code)] // M0 keeps forward-looking stubs for M1+ modules.

mod app;
mod config;
mod error;
mod event;
mod pet;
mod platform;
mod reminder;
mod render;
mod shortcut;
mod ui;

use tracing::error;

fn main() {
    if let Err(err) = app::App::run() {
        // Logging may have failed before init; still print.
        eprintln!("PawDesk failed: {err}");
        error!("fatal: {err}");
        std::process::exit(1);
    }
}
