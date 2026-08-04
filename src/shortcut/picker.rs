//! File picker for adding shortcuts (PLAT-06).
//!
//! Must not be called on the UI/render thread without yielding the event loop —
//! use `App::begin_pick_executable` which runs this on a COM STA worker thread.

use std::path::PathBuf;

use tracing::info;

use crate::error::AppError;

/// Open a native file dialog for .exe / .lnk. Returns None if user cancels.
///
/// **Windows**: call from a COM STA thread (`CoInitializeEx` apartment-threaded).
/// Blocking the winit UI thread freezes the pet and makes the dialog feel laggy.
pub fn pick_executable() -> Result<Option<PathBuf>, AppError> {
    let mut dialog = rfd::FileDialog::new()
        .set_title("添加快捷方式")
        .add_filter("程序 / 快捷方式", &["exe", "lnk"])
        .add_filter("所有文件", &["*"]);

    // Prefer a useful starting folder (Desktop → user profile).
    if let Some(dir) = default_pick_dir() {
        dialog = dialog.set_directory(dir);
    }

    let file = dialog.pick_file();

    if let Some(ref p) = file {
        info!(path = %p.display(), "picked shortcut path");
    }
    Ok(file)
}

fn default_pick_dir() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("USERPROFILE") {
        let desktop = PathBuf::from(&home).join("Desktop");
        if desktop.is_dir() {
            return Some(desktop);
        }
        let home = PathBuf::from(home);
        if home.is_dir() {
            return Some(home);
        }
    }
    None
}
