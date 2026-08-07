//! Windows-specific platform helpers (tech §7.1, PLAT-01/02).

use tracing::{debug, info, warn};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, GetMonitorInfoW,
    MonitorFromPoint, MonitorFromWindow, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, MONITORINFO, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, GetWindowLongW, SetClassLongPtrW, SetWindowLongW, SetWindowPos,
    UpdateLayeredWindow, GCLP_HBRBACKGROUND, GWL_EXSTYLE, HWND_TOPMOST, SM_CXSCREEN, SM_CYSCREEN,
    SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW,
    ULW_ALPHA, WS_EX_LAYERED, WS_EX_TRANSPARENT,
};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::{MonitorInfo, Rect};
use crate::error::AppError;

/// Query the primary monitor work area via Win32.
pub fn primary_work_area() -> Result<Rect, AppError> {
    unsafe {
        let pt = POINT { x: 0, y: 0 };
        let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(hmon, &mut info).as_bool() {
            return Err(AppError::Platform("GetMonitorInfoW failed".into()));
        }

        let wa = info.rcWork;
        Ok(rect_from_win(wa))
    }
}

/// Enumerate a practical monitor list for M0 (primary + cursor monitor).
/// Full multi-monitor enum can be expanded in M5.
pub fn list_monitors_approx() -> Result<Vec<MonitorInfo>, AppError> {
    let primary = primary_work_area()?;
    let mut monitors = vec![MonitorInfo {
        name: "Primary".into(),
        bounds: Rect {
            x: 0,
            y: 0,
            width: unsafe { GetSystemMetrics(SM_CXSCREEN) },
            height: unsafe { GetSystemMetrics(SM_CYSCREEN) },
        },
        work_area: primary,
        is_primary: true,
    }];

    if let Ok(cursor) = cursor_pos() {
        if let Ok(near) = work_area_from_point(cursor.0, cursor.1) {
            if near.work_area != primary {
                monitors.push(near);
            }
        }
    }

    Ok(monitors)
}

pub fn cursor_pos() -> Result<(i32, i32), AppError> {
    unsafe {
        let mut pt = POINT::default();
        GetCursorPos(&mut pt)
            .map_err(|e| AppError::Platform(format!("GetCursorPos failed: {e}")))?;
        Ok((pt.x, pt.y))
    }
}

pub fn work_area_from_point(x: i32, y: i32) -> Result<MonitorInfo, AppError> {
    unsafe {
        let pt = POINT { x, y };
        let hmon = MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(hmon, &mut info).as_bool() {
            return Err(AppError::Platform("GetMonitorInfoW failed".into()));
        }
        Ok(MonitorInfo {
            name: "Nearest".into(),
            bounds: rect_from_win(info.rcMonitor),
            work_area: rect_from_win(info.rcWork),
            is_primary: false,
        })
    }
}

/// Prepare a layered window for per-pixel alpha via [`update_layered_rgba`].
///
/// DXGI swapchain / color-key are unreliable (solid white/magenta square).
/// We present with `UpdateLayeredWindow` + premultiplied BGRA only.
///
/// Important: if winit or anyone called `SetLayeredWindowAttributes` first,
/// `UpdateLayeredWindow` fails until `WS_EX_LAYERED` is cleared and re-set.
pub fn enable_transparent_window(window: &impl HasWindowHandle) -> Result<(), AppError> {
    let hwnd = hwnd_from_window(window)?;
    unsafe {
        let mut ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
        // Toggle layered bit so any prior SetLayeredWindowAttributes is reset.
        ex &= !(WS_EX_LAYERED.0 as i32);
        ex &= !(WS_EX_TRANSPARENT.0 as i32);
        SetWindowLongW(hwnd, GWL_EXSTYLE, ex);
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );

        ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
        ex |= WS_EX_LAYERED.0 as i32;
        ex &= !(WS_EX_TRANSPARENT.0 as i32);
        SetWindowLongW(hwnd, GWL_EXSTYLE, ex);
        let _ = SetClassLongPtrW(hwnd, GCLP_HBRBACKGROUND, 0);
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
    info!("layered window ready for UpdateLayeredWindow present (no color-key)");
    Ok(())
}

/// Present a top-down RGBA8 buffer with true per-pixel alpha (silhouette only).
///
/// `rgba` is tightly packed top-to-bottom, 4 bytes/pixel, length >= w*h*4.
/// Transparent pixels (a≈0) show the desktop; only the pet silhouette is visible.
///
/// When `screen_pos` is `Some((x,y))`, size **and** position are applied in the
/// same `UpdateLayeredWindow` call (avoids empty-frame flash on menu open).
pub fn update_layered_rgba(
    window: &impl HasWindowHandle,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<(), AppError> {
    update_layered_rgba_ex(window, width, height, rgba, None)
}

/// Like [`update_layered_rgba`], optionally setting the layered window's
/// top-left screen position atomically with the bitmap.
pub fn update_layered_rgba_ex(
    window: &impl HasWindowHandle,
    width: u32,
    height: u32,
    rgba: &[u8],
    screen_pos: Option<(i32, i32)>,
) -> Result<(), AppError> {
    if width == 0 || height == 0 {
        return Ok(());
    }
    let need = (width as usize) * (height as usize) * 4;
    if rgba.len() < need {
        return Err(AppError::Platform(format!(
            "layered buffer too small: {} < {need}",
            rgba.len()
        )));
    }

    let hwnd = hwnd_from_window(window)?;
    unsafe {
        // Screen DC (NULL hwnd) — required by UpdateLayeredWindow docs.
        let hdc_screen = GetDC(None);
        if hdc_screen.0.is_null() {
            return Err(AppError::Platform("GetDC(NULL) failed".into()));
        }
        let hdc_mem = CreateCompatibleDC(Some(hdc_screen));
        if hdc_mem.0.is_null() {
            ReleaseDC(None, hdc_screen);
            return Err(AppError::Platform("CreateCompatibleDC failed".into()));
        }

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                // negative = top-down DIB
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0 as u32,
                biSizeImage: need as u32,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [Default::default(); 1],
        };

        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let hbmp = match CreateDIBSection(
            Some(hdc_mem),
            &bmi,
            DIB_RGB_COLORS,
            &mut bits,
            None,
            0,
        ) {
            Ok(h) => h,
            Err(e) => {
                let _ = DeleteDC(hdc_mem);
                ReleaseDC(None, hdc_screen);
                return Err(AppError::Platform(format!("CreateDIBSection failed: {e}")));
            }
        };

        if bits.is_null() {
            let _ = DeleteObject(hbmp.into());
            let _ = DeleteDC(hdc_mem);
            ReleaseDC(None, hdc_screen);
            return Err(AppError::Platform("CreateDIBSection null bits".into()));
        }

        // RGBA → premultiplied BGRA in the DIB (required for AC_SRC_ALPHA).
        let dst = std::slice::from_raw_parts_mut(bits as *mut u8, need);
        for i in 0..(width as usize * height as usize) {
            let o = i * 4;
            let r = rgba[o] as u32;
            let g = rgba[o + 1] as u32;
            let b = rgba[o + 2] as u32;
            let a = rgba[o + 3] as u32;
            let pr = (r * a + 127) / 255;
            let pg = (g * a + 127) / 255;
            let pb = (b * a + 127) / 255;
            dst[o] = pb as u8;
            dst[o + 1] = pg as u8;
            dst[o + 2] = pr as u8;
            dst[o + 3] = a as u8;
        }

        let old = SelectObject(hdc_mem, hbmp.into());
        let mut size = windows::Win32::Foundation::SIZE {
            cx: width as i32,
            cy: height as i32,
        };
        let mut pt_src = POINT { x: 0, y: 0 };
        let mut pt_dst = screen_pos
            .map(|(x, y)| POINT { x, y })
            .unwrap_or(POINT { x: 0, y: 0 });
        let mut blend = windows::Win32::Graphics::Gdi::BLENDFUNCTION {
            BlendOp: 0, // AC_SRC_OVER
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: 1, // AC_SRC_ALPHA
        };

        // Ensure layered bit is still set (some paths clear it).
        let mut ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
        if ex & (WS_EX_LAYERED.0 as i32) == 0 {
            ex |= WS_EX_LAYERED.0 as i32;
            SetWindowLongW(hwnd, GWL_EXSTYLE, ex);
        }

        // pptDst: None keeps current screen position; Some moves atomically with size.
        let ok = UpdateLayeredWindow(
            hwnd,
            Some(hdc_screen),
            if screen_pos.is_some() {
                Some(&mut pt_dst)
            } else {
                None
            },
            Some(&mut size),
            Some(hdc_mem),
            Some(&mut pt_src),
            windows::Win32::Foundation::COLORREF(0),
            Some(&mut blend),
            ULW_ALPHA,
        );

        let _ = SelectObject(hdc_mem, old);
        let _ = DeleteObject(hbmp.into());
        let _ = DeleteDC(hdc_mem);
        ReleaseDC(None, hdc_screen);

        if let Err(e) = ok {
            return Err(AppError::Platform(format!("UpdateLayeredWindow failed: {e}")));
        }
    }
    Ok(())
}

/// Re-assert always-on-top without activating the window.
pub fn ensure_topmost(window: &impl HasWindowHandle) -> Result<(), AppError> {
    let hwnd = hwnd_from_window(window)?;
    unsafe {
        SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )
        .map_err(|e| AppError::Platform(format!("SetWindowPos TOPMOST failed: {e}")))?;
    }
    debug!("window topmost flag applied");
    Ok(())
}

/// Best-effort work area for the monitor that currently hosts the window.
pub fn work_area_for_window(window: &impl HasWindowHandle) -> Result<Rect, AppError> {
    let hwnd = hwnd_from_window(window)?;
    unsafe {
        let hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(hmon, &mut info).as_bool() {
            warn!("GetMonitorInfoW for window failed; falling back to primary");
            return primary_work_area();
        }
        Ok(rect_from_win(info.rcWork))
    }
}

fn hwnd_from_window(window: &impl HasWindowHandle) -> Result<HWND, AppError> {
    let handle = window
        .window_handle()
        .map_err(|e| AppError::Platform(format!("raw window handle unavailable: {e}")))?;
    match handle.as_raw() {
        RawWindowHandle::Win32(h) => Ok(HWND(h.hwnd.get() as *mut _)),
        other => Err(AppError::Platform(format!(
            "expected Win32 window handle, got {other:?}"
        ))),
    }
}

fn rect_from_win(r: RECT) -> Rect {
    Rect {
        x: r.left,
        y: r.top,
        width: r.right - r.left,
        height: r.bottom - r.top,
    }
}

/// Keep window top-left so the window stays mostly inside the work area (PLAT-05).
pub fn clamp_top_left_to_work_area(
    x: i32,
    y: i32,
    win_w: i32,
    win_h: i32,
    work: Rect,
) -> (i32, i32) {
    let margin = 8i32;
    // Keep at least `peek` pixels visible on each axis.
    let peek = 40i32.min(win_w).min(win_h);
    let min_x = work.x - win_w + peek;
    let max_x = work.x + work.width - peek;
    let min_y = work.y - win_h + peek;
    let max_y = work.y + work.height - peek;
    let nx = x.clamp(min_x.min(max_x), max_x.max(min_x));
    let ny = y.clamp(min_y.min(max_y), max_y.max(min_y));
    // Prefer fully inside when possible
    let prefer_x = x.clamp(work.x + margin, (work.x + work.width - win_w - margin).max(work.x));
    let prefer_y = y.clamp(work.y + margin, (work.y + work.height - win_h - margin).max(work.y));
    // If fully inside is valid, use it; else use peek-based clamp.
    let fully_ok = prefer_x >= work.x - margin
        && prefer_y >= work.y - margin
        && prefer_x + win_w <= work.x + work.width + margin
        && prefer_y + win_h <= work.y + work.height + margin;
    if fully_ok {
        (prefer_x, prefer_y)
    } else {
        (nx, ny)
    }
}

/// Design baseline pet window in logical pixels (actual size = baseline × pet.scale).
pub const PET_WINDOW_LOGICAL_SIZE: u32 = 128;

/// Approximate solid-body hit radius in physical pixels (fallback circular hit test).
pub const PET_HIT_RADIUS_PX: f64 = 48.0;

/// Alpha threshold for sprite hit-testing (0–255).
pub const PET_HIT_ALPHA_THRESHOLD: u8 = 24;

/// Toggle mouse click-through (WS_EX_TRANSPARENT). When enabled, clicks pass to desktop.
pub fn set_click_through(window: &impl HasWindowHandle, enabled: bool) -> Result<(), AppError> {
    let hwnd = hwnd_from_window(window)?;
    unsafe {
        let mut style = GetWindowLongW(hwnd, GWL_EXSTYLE);
        // Ensure layered for composition with transparent surfaces.
        style |= WS_EX_LAYERED.0 as i32;
        if enabled {
            style |= WS_EX_TRANSPARENT.0 as i32;
        } else {
            style &= !(WS_EX_TRANSPARENT.0 as i32);
        }
        SetWindowLongW(hwnd, GWL_EXSTYLE, style);
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
    Ok(())
}

/// Circular hit test in window client coordinates (fallback).
pub fn point_in_pet_hit(local_x: f64, local_y: f64, width: f64, height: f64) -> bool {
    let cx = width * 0.5;
    let cy = height * 0.5;
    let dx = local_x - cx;
    let dy = local_y - cy;
    dx * dx + dy * dy <= PET_HIT_RADIUS_PX * PET_HIT_RADIUS_PX
}

/// Alpha-based hit test: map client pixel to sprite UV and sample alpha.
/// `rgba` is tightly packed RGBA for `sprite_w * sprite_h`.
/// Sprite is assumed centered and scaled uniformly to fit the window (letterboxed).
pub fn point_in_sprite_alpha(
    local_x: f64,
    local_y: f64,
    win_w: f64,
    win_h: f64,
    sprite_w: u32,
    sprite_h: u32,
    rgba: &[u8],
) -> bool {
    if win_w <= 0.0 || win_h <= 0.0 || sprite_w == 0 || sprite_h == 0 {
        return false;
    }
    let sw = sprite_w as f64;
    let sh = sprite_h as f64;
    // Uniform scale to fill window (matches scale_rgba_centered present path).
    let scale = (win_w / sw).min(win_h / sh);
    let mut dw = sw * scale;
    let mut dh = sh * scale;
    // Same ~2px safe margin as present path.
    let margin = 2.0_f64.min(dw / 16.0).min(dh / 16.0);
    dw = (dw - margin * 2.0).max(1.0);
    dh = (dh - margin * 2.0).max(1.0);
    let ox = (win_w - dw) * 0.5;
    let oy = (win_h - dh) * 0.5;
    if local_x < ox || local_y < oy || local_x >= ox + dw || local_y >= oy + dh {
        return false;
    }
    let u = ((local_x - ox) / dw * sw).floor() as i32;
    let v = ((local_y - oy) / dh * sh).floor() as i32;
    if u < 0 || v < 0 || u >= sprite_w as i32 || v >= sprite_h as i32 {
        return false;
    }
    let i = ((v as u32 * sprite_w + u as u32) * 4) as usize;
    if i + 3 >= rgba.len() {
        return false;
    }
    rgba[i + 3] >= PET_HIT_ALPHA_THRESHOLD
}

/// Clamp a window top-left so the window stays mostly inside the work area.
pub fn clamp_position_to_work_area(
    x: i32,
    y: i32,
    win_w: i32,
    win_h: i32,
    work: super::Rect,
) -> (i32, i32) {
    let max_x = work.x + work.width - win_w.max(1);
    let max_y = work.y + work.height - win_h.max(1);
    let nx = x.clamp(work.x, max_x.max(work.x));
    let ny = y.clamp(work.y, max_y.max(work.y));
    (nx, ny)
}
