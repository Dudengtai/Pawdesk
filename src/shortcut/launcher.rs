//! Process launch without shell concatenation (PLAT-07, SC-04).

use std::path::Path;

use tracing::{info, warn};

use super::model::ShortcutItem;
use crate::error::AppError;

/// Launch a shortcut. Uses ShellExecute on Windows for .lnk / .exe reliability.
pub fn launch(item: &ShortcutItem) -> Result<(), AppError> {
    if !item.enabled {
        return Err(AppError::Shortcut("快捷方式已禁用".into()));
    }
    if !item.target_path.exists() {
        return Err(AppError::Shortcut(format!(
            "路径不存在：{}",
            item.target_path.display()
        )));
    }

    #[cfg(windows)]
    {
        launch_windows(item)
    }
    #[cfg(not(windows))]
    {
        launch_command(item)
    }
}

#[cfg(windows)]
fn launch_windows(item: &ShortcutItem) -> Result<(), AppError> {
    use std::os::windows::ffi::OsStrExt;

    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let path_wide: Vec<u16> = item
        .target_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let args = if item.arguments.is_empty() {
        None
    } else {
        // Join args with spaces; do not pass through cmd.exe.
        use std::ffi::OsStr;
        let joined = item.arguments.join(" ");
        let w: Vec<u16> = OsStr::new(&joined)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        Some(w)
    };

    let dir = item.working_directory.as_ref().map(|d| {
        d.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>()
    });

    let args_pc = args
        .as_ref()
        .map(|a| PCWSTR(a.as_ptr()))
        .unwrap_or(PCWSTR::null());
    let dir_pc = dir
        .as_ref()
        .map(|d| PCWSTR(d.as_ptr()))
        .unwrap_or(PCWSTR::null());

    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR::null(), // default verb "open"
            PCWSTR(path_wide.as_ptr()),
            args_pc,
            dir_pc,
            SW_SHOWNORMAL,
        )
    };

    // ShellExecute returns > 32 on success (as HINSTANCE value).
    let code = result.0 as isize;
    if code <= 32 {
        warn!(code, path = %item.target_path.display(), "ShellExecuteW failed");
        return Err(AppError::Shortcut(match code {
            0 => "系统资源不足，无法启动".into(),
            2 => format!("文件不存在：{}", item.target_path.display()),
            3 => format!("路径不存在：{}", item.target_path.display()),
            5 => "没有权限启动该程序".into(),
            8 => "内存不足".into(),
            26..=31 => "文件关联或分享错误".into(),
            _ => format!("启动失败（代码 {code}）"),
        }));
    }

    info!(name = %item.name, path = %item.target_path.display(), "launched shortcut");
    Ok(())
}

#[cfg(not(windows))]
fn launch_command(item: &ShortcutItem) -> Result<(), AppError> {
    use std::process::Command;
    let mut cmd = Command::new(&item.target_path);
    cmd.args(&item.arguments);
    if let Some(dir) = &item.working_directory {
        cmd.current_dir(dir);
    }
    cmd.spawn()
        .map_err(|e| AppError::Shortcut(format!("启动失败：{e}")))?;
    Ok(())
}

pub fn path_exists(path: &Path) -> bool {
    path.exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn missing_path_errors() {
        let item = ShortcutItem::new("x", PathBuf::from("Z:\\no_such_pawdesk_file.exe"), 0);
        let err = launch(&item).unwrap_err();
        assert!(matches!(err, AppError::Shortcut(_)));
    }

    #[test]
    fn disabled_errors() {
        let mut item =
            ShortcutItem::new("x", PathBuf::from("C:\\Windows\\System32\\notepad.exe"), 0);
        item.enabled = false;
        assert!(launch(&item).is_err());
    }
}
