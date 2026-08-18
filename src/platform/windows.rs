//! Windows-specific platform helpers (tech §7.1, PLAT-01/02).

use std::cell::RefCell;

use tracing::{debug, info, warn};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_TRANSITIONS_FORCEDISABLED};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, GetMonitorInfoW,
    MonitorFromPoint, MonitorFromWindow, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetCursorPos, GetSystemMetrics, GetWindowLongW, SetClassLongPtrW,
    SetWindowsHookExW, SetWindowLongW, SetWindowPos, SystemParametersInfoW, UnhookWindowsHookEx,
    UpdateLayeredWindow, GCLP_HBRBACKGROUND, GWL_EXSTYLE, HWND_TOPMOST, SM_CXSCREEN, SM_CYSCREEN,
    SPI_GETCLIENTAREAANIMATION, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOREDRAW,
    SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, ULW_ALPHA,
    WH_MOUSE_LL,
    WM_LBUTTONDOWN, WS_EX_LAYERED, WS_EX_TRANSPARENT,
};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::{MonitorInfo, Rect};
use crate::error::AppError;

/// Windows stand-in for `prefers-reduced-motion`: client-area animation off.
pub fn client_area_animation_enabled() -> bool {
    unsafe {
        let mut on: u32 = 1;
        let ok = SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some((&mut on as *mut u32).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        ok.is_ok() && on != 0
    }
}

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
        // DWM's resize transition otherwise fades/flashes the old layered
        // surface when the HWND box changes (reminder / first dock grow).
        let disable: i32 = 1;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_TRANSITIONS_FORCEDISABLED,
            &disable as *const i32 as *const _,
            std::mem::size_of::<i32>() as u32,
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
    LAYERED_DIB.with(|cell| unsafe {
        let hdc_screen = GetDC(None);
        if hdc_screen.0.is_null() {
            return Err(AppError::Platform("GetDC(NULL) failed".into()));
        }

        let mut slot = cell.borrow_mut();
        let reuse = slot
            .as_ref()
            .map(|s| s.width == width && s.height == height)
            .unwrap_or(false);
        if !reuse {
            *slot = None;
            match LayeredDib::create(hdc_screen, width, height, need) {
                Ok(dib) => *slot = Some(dib),
                Err(e) => {
                    ReleaseDC(None, hdc_screen);
                    return Err(e);
                }
            }
        }
        let dib = slot.as_mut().expect("layered DIB just created");

        // RGBA → premultiplied BGRA in the DIB (required for AC_SRC_ALPHA).
        let dst = std::slice::from_raw_parts_mut(dib.bits, need);
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

        let ok = UpdateLayeredWindow(
            hwnd,
            Some(hdc_screen),
            if screen_pos.is_some() {
                Some(&mut pt_dst)
            } else {
                None
            },
            Some(&mut size),
            Some(dib.hdc_mem),
            Some(&mut pt_src),
            windows::Win32::Foundation::COLORREF(0),
            Some(&mut blend),
            ULW_ALPHA,
        );

        ReleaseDC(None, hdc_screen);

        if let Err(e) = ok {
            return Err(AppError::Platform(format!("UpdateLayeredWindow failed: {e}")));
        }
        Ok(())
    })
}

/// Move and resize a layered HWND in one **synchronous** `SetWindowPos`.
///
/// winit's `request_inner_size` / `set_outer_position` use `SWP_ASYNCWINDOWPOS`
/// plus `InvalidateRgn`. The real size change lands *after* our
/// `UpdateLayeredWindow`, drops the layered bitmap, and the cat flashes.
///
/// Do not pass `SWP_NOCOPYBITS` (that discards the bitmap on purpose) and do
/// not pass `SWP_ASYNCWINDOWPOS`. Caller must `UpdateLayeredWindow` immediately
/// after this returns.
pub fn sync_layered_hwnd(
    window: &impl HasWindowHandle,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<(), AppError> {
    let hwnd = hwnd_from_window(window)?;
    let w = width.max(1) as i32;
    let h = height.max(1) as i32;
    unsafe {
        SetWindowPos(
            hwnd,
            None,
            x,
            y,
            w,
            h,
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOREDRAW,
        )
        .map_err(|e| AppError::Platform(format!("sync_layered_hwnd SetWindowPos: {e}")))?;
    }
    Ok(())
}

thread_local! {
    static LAYERED_DIB: RefCell<Option<LayeredDib>> = const { RefCell::new(None) };
}

/// Reused memory DC + DIB so open/close frames don't Create/Delete every tick.
struct LayeredDib {
    hdc_mem: HDC,
    hbmp: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    width: u32,
    height: u32,
}

impl LayeredDib {
    unsafe fn create(hdc_screen: HDC, width: u32, height: u32, need: usize) -> Result<Self, AppError> {
        let hdc_mem = CreateCompatibleDC(Some(hdc_screen));
        if hdc_mem.0.is_null() {
            return Err(AppError::Platform("CreateCompatibleDC failed".into()));
        }
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
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
                return Err(AppError::Platform(format!("CreateDIBSection failed: {e}")));
            }
        };
        if bits.is_null() {
            let _ = DeleteObject(hbmp.into());
            let _ = DeleteDC(hdc_mem);
            return Err(AppError::Platform("CreateDIBSection null bits".into()));
        }
        let old = SelectObject(hdc_mem, hbmp.into());
        Ok(Self {
            hdc_mem,
            hbmp,
            old,
            bits: bits as *mut u8,
            width,
            height,
        })
    }
}

impl Drop for LayeredDib {
    fn drop(&mut self) {
        unsafe {
            let _ = SelectObject(self.hdc_mem, self.old);
            let _ = DeleteObject(self.hbmp.into());
            let _ = DeleteDC(self.hdc_mem);
        }
    }
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

// --- Launcher outside-click guard -------------------------------------------
//
// While the launcher (快捷启动坞) is open, the pet window is a layered window
// that only receives input over its own rect. A low-level mouse hook watches
// for left clicks landing outside that rect (desktop / other apps) and flags
// the app to close the dock. The hook is purely observant: it never consumes
// the click, so the target window below still receives it normally.
//
// The hook must NOT run on the winit thread: WH_MOUSE_LL funnels *every*
// system mouse packet (moves included) through the installing thread via a
// synchronous message, so a busy render thread stalls input system-wide.
// It runs on a dedicated idle thread with its own message pump instead; the
// winit thread only polls an atomic flag once per frame.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, PostThreadMessageW, TranslateMessage, MSG, WM_QUIT,
};

/// Launcher window rect (physical screen coords), shared with the hook proc.
/// `i32::MIN` sentinel = not installed.
static LAUNCHER_RECT: [AtomicI32; 4] = [
    AtomicI32::new(i32::MIN),
    AtomicI32::new(i32::MIN),
    AtomicI32::new(i32::MIN),
    AtomicI32::new(i32::MIN),
];
/// Set by the hook proc when a left click landed outside the rect.
static OUTSIDE_CLICKED: AtomicBool = AtomicBool::new(false);

/// Observational low-level mouse hook active while the launcher is open.
/// Owns the dedicated hook thread; the winit thread never pumps hook messages.
pub struct OutsideClickGuard {
    thread_id: u32,
    join: Option<JoinHandle<()>>,
}

impl OutsideClickGuard {
    /// Spawn a thread that installs the WH_MOUSE_LL hook and pumps messages,
    /// so input delivery never waits on the render thread. `rect` is the
    /// launcher window rect in physical pixels.
    pub fn install(rect: Rect) -> Option<Self> {
        OUTSIDE_CLICKED.store(false, Ordering::Relaxed);
        set_launcher_rect(&rect);

        let quitting = Arc::new(AtomicBool::new(false));
        let thread_quitting = Arc::clone(&quitting);
        let (tx, rx) = mpsc::channel::<(u32, bool)>();
        let join = thread::Builder::new()
            .name("launcher-outside-hook".into())
            .spawn(move || {
                let tid = unsafe { GetCurrentThreadId() };
                let hook = unsafe {
                    let hmod = GetModuleHandleW(PCWSTR::null())
                        .ok()
                        .map(|h| HINSTANCE(h.0));
                    SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook_proc), hmod, 0)
                };
                let Ok(hook) = hook else {
                    warn!("WH_MOUSE_LL install failed; outside-click close disabled");
                    let _ = tx.send((tid, false));
                    return;
                };
                let _ = tx.send((tid, true));
                debug!("launcher outside-click hook installed (hook thread tid={tid})");

                // Pump: low-level hook procs are delivered while this thread
                // retrieves messages. The thread is idle otherwise.
                let mut msg = MSG::default();
                loop {
                    if thread_quitting.load(Ordering::Relaxed) {
                        break;
                    }
                    let r = unsafe { GetMessageW(&mut msg, None, 0, 0) };
                    if r.0 <= 0 {
                        break; // WM_QUIT or error
                    }
                    unsafe {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
                unsafe {
                    // Unhook from the installing thread (same thread).
                    let _ = UnhookWindowsHookEx(hook);
                }
            })
            .ok()?;

        let (tid, installed) = rx.recv().ok()?;
        if !installed {
            let _ = join.join();
            return None;
        }
        Some(Self {
            thread_id: tid,
            join: Some(join),
        })
    }

    /// Keep the rect in sync (window may move while open) — cheap atomic writes.
    pub fn update_rect(&self, rect: Rect) {
        set_launcher_rect(&rect);
    }

    /// Whether an outside left-click is pending; clears the flag.
    pub fn take_outside_click() -> bool {
        OUTSIDE_CLICKED.swap(false, Ordering::Relaxed)
    }
}

impl Drop for OutsideClickGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
        LAUNCHER_RECT[0].store(i32::MIN, Ordering::Relaxed);
        OUTSIDE_CLICKED.store(false, Ordering::Relaxed);
        debug!("launcher outside-click hook removed");
    }
}

fn set_launcher_rect(rect: &Rect) {
    LAUNCHER_RECT[0].store(rect.x, Ordering::Relaxed);
    LAUNCHER_RECT[1].store(rect.y, Ordering::Relaxed);
    LAUNCHER_RECT[2].store(
        rect.x.saturating_add(rect.width),
        Ordering::Relaxed,
    );
    LAUNCHER_RECT[3].store(
        rect.y.saturating_add(rect.height),
        Ordering::Relaxed,
    );
}

unsafe extern "system" fn mouse_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && wparam.0 as u32 == WM_LBUTTONDOWN && !cursor_inside_launcher() {
        OUTSIDE_CLICKED.store(true, Ordering::Relaxed);
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Cursor position vs the launcher rect. Unknown cursor → treat as inside so a
/// failed query never spuriously closes the dock.
fn cursor_inside_launcher() -> bool {
    let Ok((x, y)) = cursor_pos() else {
        return true;
    };
    let rx = LAUNCHER_RECT[0].load(Ordering::Relaxed);
    let ry = LAUNCHER_RECT[1].load(Ordering::Relaxed);
    let rr = LAUNCHER_RECT[2].load(Ordering::Relaxed);
    let rb = LAUNCHER_RECT[3].load(Ordering::Relaxed);
    if rr <= rx || rb <= ry {
        return true; // rect not installed yet — do nothing
    }
    x >= rx && y >= ry && x < rr && y < rb
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Smoke test: the hook thread installs WH_MOUSE_LL and the guard drops
    /// cleanly (unhook + join) without hanging. No mouse input is needed.
    #[test]
    fn outside_click_guard_installs_and_drops() {
        let rect = Rect {
            x: 100,
            y: 100,
            width: 200,
            height: 150,
        };
        let guard = OutsideClickGuard::install(rect);
        assert!(guard.is_some(), "WH_MOUSE_LL hook should install on Windows");
        let guard = guard.expect("hook installed");
        guard.update_rect(rect);
        assert!(!OutsideClickGuard::take_outside_click());
        drop(guard);
        // Drop reset the sentinel rect.
        assert_eq!(LAUNCHER_RECT[0].load(Ordering::Relaxed), i32::MIN);
    }
}

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
