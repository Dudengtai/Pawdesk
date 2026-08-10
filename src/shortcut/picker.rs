//! File picker for adding shortcuts (PLAT-06).
//!
//! On Windows this uses the native `IFileOpenDialog` instead of rfd so shortcut
//! files stay visible on the Desktop and are returned as `.lnk` paths rather
//! than being resolved/hidden by the common dialog.
//!
//! Important: open the **Shell virtual Desktop** (user + public merge), not the
//! filesystem path `C:\Users\...\Desktop`, otherwise Public Desktop shortcuts
//! are missing.

use std::path::PathBuf;

use tracing::info;
use winit::window::Window;

use crate::error::AppError;

/// Picker settings captured on the UI thread, then used on a COM STA worker.
#[derive(Debug, Clone)]
pub struct PickContext {
    parent_hwnd: Option<isize>,
}

/// Build picker settings. Passing the launcher window as the owner keeps the
/// dialog above the launcher without toggling its topmost window level.
pub fn build_pick_context(parent: Option<&Window>) -> PickContext {
    PickContext {
        parent_hwnd: parent.and_then(window_hwnd),
    }
}

fn window_hwnd(window: &Window) -> Option<isize> {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = window.window_handle().ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get() as isize),
        _ => None,
    }
}

/// Open the prepared native file dialog. Returns None if user cancels.
///
/// **Windows**: call from a COM STA thread (`CoInitializeEx` apartment-threaded).
/// Blocking the winit UI thread freezes the pet and makes the dialog feel laggy.
pub fn pick_executable(context: PickContext) -> Result<Option<PathBuf>, AppError> {
    #[cfg(windows)]
    let file = windows_pick_executable(context)?;
    #[cfg(not(windows))]
    let file = fallback_pick_executable(context)?;

    if let Some(ref p) = file {
        info!(path = %p.display(), "picked shortcut path");
    }
    Ok(file)
}

#[cfg(windows)]
fn windows_pick_executable(context: PickContext) -> Result<Option<PathBuf>, AppError> {
    use std::ffi::c_void;

    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::{
        Common::COMDLG_FILTERSPEC, FileOpenDialog, FOLDERID_Desktop, FOS_FILEMUSTEXIST,
        FOS_NODEREFERENCELINKS, FOS_PATHMUSTEXIST, IFileOpenDialog, IShellItem, KF_FLAG_DEFAULT,
        SHGetKnownFolderItem, SIGDN_FILESYSPATH,
    };
    use windows::core::PCWSTR;

    unsafe {
        let dialog: IFileOpenDialog =
            CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| AppError::Platform(format!("create file dialog: {e}")))?;

        // Clear any remembered dialog state (last filter / last folder) so we
        // always land on the virtual Desktop with the intended filter.
        let _ = dialog.ClearClientData();

        // Do **not** set FOS_FORCEFILESYSTEM: that forces a plain filesystem
        // folder view of `%USERPROFILE%\Desktop` and hides the Public Desktop
        // shortcuts that Explorer merges into "桌面".
        dialog
            .SetOptions(FOS_FILEMUSTEXIST | FOS_PATHMUSTEXIST | FOS_NODEREFERENCELINKS)
            .map_err(|e| AppError::Platform(format!("configure file dialog: {e}")))?;

        let title = wide("添加快捷方式");
        dialog
            .SetTitle(PCWSTR(title.as_ptr()))
            .map_err(|e| AppError::Platform(format!("set dialog title: {e}")))?;

        // Prefer app/shortcut filter; keep "所有文件" as a fallback.
        let app_name = wide("程序 / 快捷方式");
        let app_spec = wide("*.lnk;*.url;*.exe");
        let all_name = wide("所有文件");
        let all_spec = wide("*.*");
        let filters = [
            COMDLG_FILTERSPEC {
                pszName: PCWSTR(app_name.as_ptr()),
                pszSpec: PCWSTR(app_spec.as_ptr()),
            },
            COMDLG_FILTERSPEC {
                pszName: PCWSTR(all_name.as_ptr()),
                pszSpec: PCWSTR(all_spec.as_ptr()),
            },
        ];
        dialog
            .SetFileTypes(&filters)
            .map_err(|e| AppError::Platform(format!("set dialog filters: {e}")))?;
        dialog
            .SetFileTypeIndex(1)
            .map_err(|e| AppError::Platform(format!("set default filter: {e}")))?;

        // Open the Shell virtual Desktop (user + public merge), same as Explorer.
        let desktop: IShellItem =
            SHGetKnownFolderItem(&FOLDERID_Desktop, KF_FLAG_DEFAULT, None).map_err(|e| {
                AppError::Platform(format!("resolve shell Desktop folder: {e}"))
            })?;
        dialog
            .SetFolder(&desktop)
            .map_err(|e| AppError::Platform(format!("set Desktop folder: {e}")))?;

        let owner = context.parent_hwnd.map(|h| HWND(h as *mut c_void));
        if let Err(e) = dialog.Show(owner) {
            // HRESULT_FROM_WIN32(ERROR_CANCELLED) = 0x800704C7
            if e.code() == windows::core::HRESULT(0x800704C7u32 as i32) {
                return Ok(None);
            }
            return Err(AppError::Platform(format!("show file dialog: {e}")));
        }

        let item = dialog
            .GetResult()
            .map_err(|e| AppError::Platform(format!("read file dialog result: {e}")))?;
        let name = item
            .GetDisplayName(SIGDN_FILESYSPATH)
            .map_err(|e| AppError::Platform(format!("read selected path: {e}")))?;
        let text = match name.to_string() {
            Ok(text) => text,
            Err(e) => {
                CoTaskMemFree(Some(name.0 as *const c_void));
                return Err(AppError::Platform(format!("decode selected path: {e}")));
            }
        };
        CoTaskMemFree(Some(name.0 as *const c_void));

        Ok(Some(PathBuf::from(text)))
    }
}

#[cfg(not(windows))]
fn fallback_pick_executable(_context: PickContext) -> Result<Option<PathBuf>, AppError> {
    let mut dialog = rfd::FileDialog::new()
        .set_title("添加快捷方式")
        .add_filter("程序 / 快捷方式", &["lnk", "url", "exe"])
        .add_filter("所有文件", &["*"]);

    if let Ok(home) = std::env::var("USERPROFILE") {
        let desktop = PathBuf::from(&home).join("Desktop");
        if desktop.is_dir() {
            dialog = dialog.set_directory(desktop);
        }
    }
    Ok(dialog.pick_file())
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
