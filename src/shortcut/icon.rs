//! Extract a real application icon from a shortcut target (.exe / .lnk).
//!
//! For `.lnk` files the shell would draw the shortcut-arrow overlay on top of
//! the target's icon (`SHGetFileInfoW` + `SHGFI_ICON` does exactly that), so
//! this module first resolves the shortcut's *icon source*:
//!
//! 1. `SHGFI_ICONLOCATION` → the icon path + index the shortcut displays
//!    (with the documented drive-letter quirk: the returned path starts with
//!    NUL meaning "same drive as the shortcut").
//! 2. `IShellLinkW::GetPath` → the real target exe, when the icon location
//!    is stale or missing.
//! 3. Fall back to `SHGetFileInfoW` on the `.lnk` itself (may keep the arrow
//!    overlay — still better than a missing icon).
//!
//! The icon is pulled from the system image list (`SHIL_JUMBO`, up to 256 px,
//! shell-composited alpha — no opaque-black artifacts, crisp on HiDPI) and
//! decoded with `GetIconInfo` + `GetDIBits` into straight (non-premultiplied)
//! RGBA rows. Each icon is then classified by shape ([`IconShape`]) and
//! downscaled to ≤ 64 px for cheap caching. Any failure yields `None` and the
//! launcher keeps its letter-tile fallback.

use std::path::Path;

#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::HICON;

/// Icon silhouette shape, used to pick the launcher presentation:
/// round icons get a dark tile container, square icons stand alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconShape {
    Round,
    Square,
}

/// A decoded icon: top-down rows of straight RGBA, `w * h * 4` bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconRgba {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
    pub shape: IconShape,
}

/// Cache-storage cap: plenty above the ~26 px display slot, 8× cheaper than
/// keeping the full 256 px image list frame.
const STORE_MAX: u32 = 64;

impl IconRgba {
    /// Classify the silhouette: transparent corners + low coverage → round;
    /// otherwise square (covers full-bleed squares and subtle rounded corners).
    fn classify(w: u32, h: u32, rgba: &[u8]) -> IconShape {
        let w = w.max(1) as usize;
        let h = h.max(1) as usize;
        // 15% corner blocks: a full-bleed circle (radius ≈ 48% of canvas)
        // stays clear of them, while rounded-square corners (r ≲ 12%) fill them.
        let k = ((w.min(h) as f32) * 0.15).round().max(2.0) as usize;
        let corners = [
            (0usize, 0usize),
            (w.saturating_sub(k), 0),
            (0, h.saturating_sub(k)),
            (w.saturating_sub(k), h.saturating_sub(k)),
        ];
        let mut sum = 0u64;
        let mut n = 0u64;
        for &(cx, cy) in &corners {
            for y in cy..(cy + k).min(h) {
                for x in cx..(cx + k).min(w) {
                    sum += rgba[(y * w + x) * 4 + 3] as u64;
                    n += 1;
                }
            }
        }
        let corner_alpha = sum as f32 / n.max(1) as f32;
        let mut opaque = 0u64;
        for px in rgba.chunks_exact(4) {
            if px[3] > 8 {
                opaque += 1;
            }
        }
        let coverage = opaque as f32 / (w * h) as f32;
        if corner_alpha < 32.0 && coverage < 0.92 {
            IconShape::Round
        } else {
            IconShape::Square
        }
    }
}

/// Extract the shell icon (JUMBO, up to 256 px @ 96 DPI) for `path`.
#[cfg(windows)]
pub fn extract_icon(path: &Path) -> Option<IconRgba> {
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};

    // SHGetFileInfo / IShellLink are most reliable from a COM-initialized thread.
    let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let own_init = hr.0 == 0; // S_OK(0) → we own the init; S_FALSE → pre-existing
    let result = extract_icon_impl(path);
    if own_init {
        unsafe { CoUninitialize() };
    }
    result
}

#[cfg(windows)]
fn extract_icon_impl(path: &Path) -> Option<IconRgba> {
    if is_lnk(path) {
        // 1. The icon the shortcut actually displays (no overlay arrow).
        if let Some((src, index)) = icon_location(path) {
            if src.exists() {
                if let Some(ic) = extract_icon_at(&src, index) {
                    return Some(ic);
                }
            }
        }
        // 2. Stale icon location → resolve the real target.
        if let Some(target) = lnk_target(path) {
            if target.exists() {
                if let Some(ic) = extract_icon_at(&target, 0) {
                    return Some(ic);
                }
            }
        }
        // 3. Last resort: the .lnk itself (may include the overlay arrow).
    }
    extract_icon_at(path, 0)
}

#[cfg(windows)]
fn is_lnk(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
}

/// Icon source of a shortcut: (path, icon index). Handles the drive quirk.
#[cfg(windows)]
pub fn icon_location(path: &Path) -> Option<(std::path::PathBuf, i32)> {
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICONLOCATION};

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut sfi = SHFILEINFOW::default();
    let got = unsafe {
        SHGetFileInfoW(
            windows::core::PCWSTR(wide.as_ptr()),
            windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut sfi as *mut SHFILEINFOW),
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICONLOCATION,
        )
    };
    if got == 0 {
        return None;
    }
    let s = String::from_utf16_lossy(&sfi.szDisplayName);
    let s = s.trim_end_matches('\0').to_string();
    if s.is_empty() {
        return None;
    }
    // Documented quirk: the path starts with NUL, meaning "same drive as the
    // shortcut" — borrow that drive letter.
    let s = if s.starts_with('\0') {
        let drive = path
            .components()
            .next()
            .and_then(|c| match c {
                std::path::Component::Prefix(p) => p.as_os_str().to_str().map(String::from),
                _ => None,
            })
            .unwrap_or_else(|| "C".into());
        format!("{}{}", drive.trim_end_matches(':'), &s[1..])
    } else {
        s
    };
    if s.is_empty() {
        return None;
    }
    Some((std::path::PathBuf::from(s), sfi.iIcon))
}

/// Resolve the real target path of a .lnk via IShellLinkW.
#[cfg(windows)]
pub fn lnk_target(path: &Path) -> Option<std::path::PathBuf> {
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::System::Com::{
        CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER, STGM_READ,
    };
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    use windows::core::Interface;

    let link: IShellLinkW =
        unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }.ok()?;
    let persist: IPersistFile = link.cast().ok()?;

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe { persist.Load(windows::core::PCWSTR(wide.as_ptr()), STGM_READ) }.ok()?;

    let mut buf = [0u16; 4096];
    unsafe { link.GetPath(&mut buf, core::ptr::null_mut(), 0) }.ok()?;
    let s = String::from_utf16_lossy(&buf);
    let s = s.trim_end_matches('\0').to_string();
    if s.is_empty() {
        return None;
    }
    Some(std::path::PathBuf::from(s))
}

/// Extract the icon at `index` from a non-.lnk source (exe / dll / ico).
#[cfg(windows)]
fn extract_icon_at(path: &Path, index: i32) -> Option<IconRgba> {
    if index == 0 {
        // Primary: system image list — shell-composited alpha, up to 256 px.
        if let Some(ic) = extract_icon_image_list(path) {
            return Some(ic);
        }
    }
    // Fallbacks: SHGetFileInfoW (32 px) for index 0, ExtractIconW for others.
    fallback_icon(path, index)
}

/// Pull the file's icon from the system image list (SHIL_JUMBO).
#[cfg(windows)]
fn extract_icon_image_list(path: &Path) -> Option<IconRgba> {
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::UI::Controls::{IImageList, ILD_TRANSPARENT};
    use windows::Win32::UI::Shell::{
        SHGetFileInfoW, SHGetImageList, SHFILEINFOW, SHGFI_SYSICONINDEX, SHIL_JUMBO,
    };

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // Register the file's icon in the system image list and get its index.
    let mut sfi = SHFILEINFOW::default();
    let got = unsafe {
        SHGetFileInfoW(
            windows::core::PCWSTR(wide.as_ptr()),
            windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut sfi as *mut SHFILEINFOW),
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_SYSICONINDEX,
        )
    };
    if got == 0 {
        return None;
    }
    // Low word = icon index; high word = overlay index (strip it just in case).
    let index = sfi.iIcon & 0xFFFF;

    let list: IImageList = unsafe { SHGetImageList(SHIL_JUMBO as i32) }.ok()?;
    let hicon = unsafe { list.GetIcon(index, ILD_TRANSPARENT.0) }.ok()?;
    if hicon.is_invalid() {
        return None;
    }
    hicon_to_rgba(hicon)
}

/// 32 px SHGFI_ICON / ExtractIconW fallback (pre-JUMBO or exotic files).
#[cfg(windows)]
fn fallback_icon(path: &Path, index: i32) -> Option<IconRgba> {
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::UI::Shell::{ExtractIconW, SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON};

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let hicon = if index == 0 {
        // SHGFI_ICON handles all file kinds (exe / dll / ico) and never adds
        // the link overlay for non-.lnk paths. SHGFI_LARGEICON is 0 (implied).
        let mut sfi = SHFILEINFOW::default();
        let got = unsafe {
            SHGetFileInfoW(
                windows::core::PCWSTR(wide.as_ptr()),
                windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(0),
                Some(&mut sfi as *mut SHFILEINFOW),
                size_of::<SHFILEINFOW>() as u32,
                SHGFI_ICON,
            )
        };
        if got == 0 || sfi.hIcon.is_invalid() {
            return None;
        }
        sfi.hIcon
    } else {
        let h = unsafe { ExtractIconW(None, windows::core::PCWSTR(wide.as_ptr()), index as u32) };
        if h.is_invalid() {
            return None;
        }
        h
    };

    hicon_to_rgba(hicon)
}

/// Decode an owned HICON into a classified, cache-sized RGBA icon
/// (destroys the icon on every path).
#[cfg(windows)]
fn hicon_to_rgba(icon: HICON) -> Option<IconRgba> {
    use std::mem::size_of;

    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DeleteDC, DeleteObject,
        DIB_RGB_COLORS, GetDIBits, GetObjectW, HBITMAP, HDC, HGDIOBJ, RGBQUAD,
    };
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, ICONINFO};

    let mut info = ICONINFO::default();
    let has_info = unsafe { GetIconInfo(icon, &mut info as *mut ICONINFO) }.is_ok();
    if !has_info {
        unsafe { let _ = DestroyIcon(icon); }
        return None;
    }

    // Guard: always release GDI objects on every exit path.
    struct Cleanup {
        color: Option<HBITMAP>,
        mask: Option<HBITMAP>,
        dc: Option<HDC>,
    }
    impl Drop for Cleanup {
        fn drop(&mut self) {
            unsafe {
                if let Some(h) = self.color {
                    let _ = DeleteObject(HGDIOBJ(h.0));
                }
                if let Some(h) = self.mask {
                    let _ = DeleteObject(HGDIOBJ(h.0));
                }
                if let Some(dc) = self.dc {
                    let _ = DeleteDC(dc);
                }
            }
        }
    }
    let mut clean = Cleanup {
        color: (!info.hbmColor.is_invalid()).then_some(info.hbmColor),
        mask: (!info.hbmMask.is_invalid()).then_some(info.hbmMask),
        dc: None,
    };

    if info.hbmColor.is_invalid() {
        // Monochrome-only icon: nothing to draw → letter fallback.
        unsafe { let _ = DestroyIcon(icon); }
        return None;
    }

    let mut bm = BITMAP::default();
    let objsz = unsafe {
        GetObjectW(
            HGDIOBJ(info.hbmColor.0),
            size_of::<BITMAP>() as i32,
            Some(&mut bm as *mut BITMAP as *mut core::ffi::c_void),
        )
    };
    if objsz == 0 {
        unsafe { let _ = DestroyIcon(icon); }
        return None;
    }

    let w = bm.bmWidth.max(0) as u32;
    let h = bm.bmHeight.max(0) as u32;
    // Sanity cap (icons are ≤ 256 px; anything larger is corrupt).
    if w == 0 || h == 0 || w > 1024 || h > 1024 {
        unsafe { let _ = DestroyIcon(icon); }
        return None;
    }
    let bpp = bm.bmBitsPixel as u32;

    let dc = unsafe { CreateCompatibleDC(None) };
    if dc.is_invalid() {
        unsafe { let _ = DestroyIcon(icon); }
        return None;
    }
    clean.dc = Some(dc);

    let mut bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            biHeight: -(h as i32), // top-down rows
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        bmiColors: [RGBQUAD::default()],
    };

    let mut buf = vec![0u8; (w * h * 4) as usize];
    let got = unsafe {
        GetDIBits(
            dc,
            info.hbmColor,
            0,
            h,
            Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
            &mut bmi,
            DIB_RGB_COLORS,
        )
    };
    if got == 0 {
        unsafe { let _ = DestroyIcon(icon); }
        return None;
    }

    // Icons are BGRA in the buffer → swap to RGBA.
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for (dst, src) in rgba.chunks_exact_mut(4).zip(buf.chunks_exact(4)) {
        dst[0] = src[2];
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = src[3];
    }

    // Pre-Vista icons (16/24 bpp) carry no alpha channel: apply the AND mask
    // (1 bit = transparent). 32 bpp icons already have real alpha.
    if bpp < 32 && !info.hbmMask.is_invalid() {
        let mut mask_buf = vec![0u8; (w * h * 4) as usize];
        let got_mask = unsafe {
            GetDIBits(
                dc,
                info.hbmMask,
                0,
                h,
                Some(mask_buf.as_mut_ptr() as *mut core::ffi::c_void),
                &mut bmi,
                DIB_RGB_COLORS,
            )
        };
        if got_mask != 0 {
            for i in 0..(w * h) as usize {
                // Monochrome mask: set bit → white pixel → transparent.
                rgba[i * 4 + 3] = if mask_buf[i * 4] > 127 { 0 } else { 255 };
            }
        }
    }

    unsafe { let _ = DestroyIcon(icon); }

    // Fully transparent icon is useless → keep the letter fallback.
    if rgba.chunks_exact(4).all(|px| px[3] == 0) {
        return None;
    }

    // Classify on the full-res silhouette, then shrink for the cache.
    let shape = IconRgba::classify(w, h, &rgba);
    let (w, h, rgba) = if w > STORE_MAX || h > STORE_MAX {
        scale_icon_rgba(&rgba, w, h, STORE_MAX, STORE_MAX)
    } else {
        (w, h, rgba)
    };
    Some(IconRgba { w, h, rgba, shape })
}

/// Bilinear scale in **premultiplied-alpha space** (icon edges stay clean;
/// straight-alpha bilinear darkens semi-transparent borders).
pub(crate) fn scale_icon_rgba(
    src: &[u8],
    sw: u32,
    sh: u32,
    max_w: u32,
    max_h: u32,
) -> (u32, u32, Vec<u8>) {
    if sw == 0 || sh == 0 || src.len() < (sw * sh * 4) as usize {
        return (1, 1, vec![0, 0, 0, 0]);
    }
    let fit = (max_w as f32 / sw as f32).min(max_h as f32 / sh as f32);
    let dw = ((sw as f32) * fit).round().max(1.0) as u32;
    let dh = ((sh as f32) * fit).round().max(1.0) as u32;
    let mut out = vec![0u8; (dw * dh * 4) as usize];
    for y in 0..dh {
        for x in 0..dw {
            let fx = (x as f32 + 0.5) * sw as f32 / dw as f32 - 0.5;
            let fy = (y as f32 + 0.5) * sh as f32 / dh as f32 - 0.5;
            let x0 = fx.floor() as i32;
            let y0 = fy.floor() as i32;
            let tx = fx - x0 as f32;
            let ty = fy - y0 as f32;
            let mut acc = [0.0f32; 4];
            for (oy, wy) in [(0, 1.0 - ty), (1, ty)] {
                for (ox, wx) in [(0, 1.0 - tx), (1, tx)] {
                    let sx = (x0 + ox).clamp(0, sw as i32 - 1) as u32;
                    let sy = (y0 + oy).clamp(0, sh as i32 - 1) as u32;
                    let si = ((sy * sw + sx) * 4) as usize;
                    let a = src[si + 3] as f32 / 255.0;
                    let wgt = wx * wy;
                    for k in 0..3 {
                        acc[k] += src[si + k] as f32 * a * wgt;
                    }
                    acc[3] += a * wgt;
                }
            }
            let di = ((y * dw + x) * 4) as usize;
            let a = acc[3].clamp(0.0, 1.0);
            if a <= 0.0 {
                out[di + 3] = 0;
                continue;
            }
            for k in 0..3 {
                out[di + k] = (acc[k] / a).round().clamp(0.0, 255.0) as u8;
            }
            out[di + 3] = (a * 255.0).round() as u8;
        }
    }
    (dw, dh, out)
}

#[cfg(not(windows))]
pub fn extract_icon(_path: &Path) -> Option<IconRgba> {
    None
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn extracts_real_exe_icon() {
        // notepad.exe ships on every Windows install and always has an icon.
        let path = Path::new(r"C:\Windows\System32\notepad.exe");
        if !path.exists() {
            return; // exotic system layout — skip
        }
        let icon = extract_icon(path)
            .unwrap_or_else(|| panic!("icon extraction must succeed for {}", path.display()));
        assert!(icon.w > 0 && icon.h > 0, "icon has a size");
        assert!(icon.w <= STORE_MAX && icon.h <= STORE_MAX, "icon is cache-sized");
        assert_eq!(icon.rgba.len(), (icon.w * icon.h * 4) as usize);
        assert!(
            icon.rgba.chunks_exact(4).any(|px| px[3] > 0),
            "icon must contain visible pixels"
        );
    }

    #[test]
    fn missing_path_yields_none() {
        assert!(extract_icon(Path::new(r"Z:\no_such_pawdesk_icon_xyz.exe")).is_none());
    }

    /// Build a synthetic icon where `inside(nx, ny)` decides opacity
    /// (normalized coords in [-0.5, 0.5]).
    fn make_icon(inside: impl Fn(f32, f32) -> bool) -> IconRgba {
        let (w, h) = (64u32, 64u32);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let nx = (x as f32 + 0.5) / w as f32 - 0.5;
                let ny = (y as f32 + 0.5) / h as f32 - 0.5;
                if inside(nx, ny) {
                    let i = ((y * w + x) * 4) as usize;
                    rgba[i] = 200;
                    rgba[i + 1] = 100;
                    rgba[i + 2] = 50;
                    rgba[i + 3] = 255;
                }
            }
        }
        IconRgba {
            w,
            h,
            rgba,
            shape: IconShape::Square,
        }
    }

    #[test]
    fn circle_classified_round() {
        let icon = make_icon(|nx, ny| nx * nx + ny * ny <= 0.48 * 0.48);
        assert_eq!(IconRgba::classify(icon.w, icon.h, &icon.rgba), IconShape::Round);
    }

    #[test]
    fn full_square_classified_square() {
        let icon = make_icon(|nx, ny| nx.abs() <= 0.49 && ny.abs() <= 0.49);
        assert_eq!(
            IconRgba::classify(icon.w, icon.h, &icon.rgba),
            IconShape::Square
        );
    }

    #[test]
    fn rounded_square_classified_square() {
        // Corner radius ≈ 12% of size — corners mostly opaque, high coverage.
        let r = 0.12f32;
        let icon = make_icon(|nx, ny| {
            let qx = nx.abs() - (0.49 - r);
            let qy = ny.abs() - (0.49 - r);
            (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() <= r
        });
        assert_eq!(
            IconRgba::classify(icon.w, icon.h, &icon.rgba),
            IconShape::Square
        );
    }

    #[test]
    fn padded_square_falls_back_to_round_container() {
        // Square with a big transparent margin — treated as round (gets the
        // dark container). Accepted trade-off, never looks broken.
        let icon = make_icon(|nx, ny| nx.abs() <= 0.34 && ny.abs() <= 0.34);
        assert_eq!(
            IconRgba::classify(icon.w, icon.h, &icon.rgba),
            IconShape::Round
        );
    }
}
