//! PawDesk entry point.

#![allow(dead_code)] // M0 keeps forward-looking stubs for M1+ modules.
#![cfg_attr(
    all(windows, not(debug_assertions), not(test)),
    windows_subsystem = "windows"
)]

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
        #[cfg(not(debug_assertions))]
        platform::show_fatal_error("PawDesk", &err.user_message());
        std::process::exit(1);
    }
}
