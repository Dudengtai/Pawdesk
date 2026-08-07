//! Windows GDI text rasterization (hinted, high-contrast).
//!
//! fontdue has **no TrueType hinting** (Latin looked wavy). We draw with
//! `CreateFontW` + `DrawTextW` into a 32-bit DIB, then convert coverage → RGBA.
//!
//! Sharpness notes (发虚 fix):
//! - Draw **black on white** into a DIBSection (1:1 device px, no stretch)
//! - Use **Microsoft YaHei UI** + medium weight
//! - Crush soft AA fringes with a contrast curve so stems read solid on glass

use std::sync::OnceLock;

use tracing::warn;

#[cfg(windows)]
mod gdi {
    use super::crop_ink;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{COLORREF, RECT};
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, CreateFontW, CreateSolidBrush, DeleteDC,
        DeleteObject, DrawTextW, FillRect, GetDC, ReleaseDC, SelectObject, SetBkMode, SetTextColor,
        ANTIALIASED_QUALITY, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CLIP_DEFAULT_PRECIS,
        DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DT_CALCRECT, DT_LEFT, DT_NOPREFIX,
        DT_WORDBREAK, FF_DONTCARE, FW_MEDIUM, HFONT, OUT_TT_PRECIS, TRANSPARENT,
    };

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
    }

    fn create_ui_font(px: i32) -> Option<HFONT> {
        // YaHei UI: mixed CJK+Latin system chrome face.
        let faces = [
            "Microsoft YaHei UI",
            "Microsoft YaHei",
            "Segoe UI",
            "DengXian",
            "SimHei",
        ];
        // Negative height = character cell in device pixels (already DPR-scaled by caller).
        let height = -px.abs().max(9);
        for face in faces {
            let name = to_wide(face);
            let font = unsafe {
                CreateFontW(
                    height,
                    0,
                    0,
                    0,
                    // Medium weight — Normal often looks washed at UI sizes on glass.
                    FW_MEDIUM.0 as i32,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET,
                    OUT_TT_PRECIS,
                    CLIP_DEFAULT_PRECIS,
                    // Grayscale AA (recolors cleanly onto glass). ClearType RGB fringes
                    // become mud when we replace color with LABEL slate.
                    ANTIALIASED_QUALITY,
                    DEFAULT_PITCH.0 as u32 | FF_DONTCARE.0 as u32,
                    PCWSTR(name.as_ptr()),
                )
            };
            if !font.is_invalid() {
                return Some(font);
            }
        }
        None
    }

    /// Map GDI grayscale coverage → alpha with a **crisper** curve.
    /// Soft fringes get crushed; stem interiors push toward opaque.
    #[inline]
    fn coverage_to_alpha(gray_on_white: f32, color_a: u8) -> u8 {
        // gray_on_white: 1.0 = background white, 0.0 = solid black ink
        let mut cov = (1.0 - gray_on_white).clamp(0.0, 1.0);
        // Drop near-invisible haze that reads as blur on light glass.
        if cov < 0.14 {
            return 0;
        }
        // Remap [0.14, 1] → [0, 1] and apply power < 1 to fatten stems.
        cov = ((cov - 0.14) / 0.86).clamp(0.0, 1.0);
        cov = cov.powf(0.58);
        // Soft ceiling so 100% coverage stays solid without blooming.
        let a = (color_a as f32 * cov).round().clamp(0.0, 255.0);
        a as u8
    }

    /// Rasterize with GDI. `px` is already device pixels (caller multiplies DPR).
    pub fn rasterize(
        text: &str,
        max_width: u32,
        px: f32,
        color: [u8; 4],
    ) -> Option<(u32, u32, Vec<u8>)> {
        if text.is_empty() || px < 4.0 {
            return None;
        }
        // Prefer integer device px; +0.5 bias so 14.4 → 15 (slightly fuller).
        let px_i = (px + 0.35).round().clamp(9.0, 96.0) as i32;
        let max_w = max_width.max(8) as i32;

        unsafe {
            let hdc_screen = GetDC(None);
            if hdc_screen.0.is_null() {
                return None;
            }
            let hdc = CreateCompatibleDC(Some(hdc_screen));
            if hdc.0.is_null() {
                ReleaseDC(None, hdc_screen);
                return None;
            }

            let font = match create_ui_font(px_i) {
                Some(f) => f,
                None => {
                    let _ = DeleteDC(hdc);
                    ReleaseDC(None, hdc_screen);
                    return None;
                }
            };
            let old_font = SelectObject(hdc, font.into());

            let mut wide: Vec<u16> = text.encode_utf16().collect();
            let mut rc = RECT {
                left: 0,
                top: 0,
                right: max_w,
                bottom: 4096,
            };
            let flags = DT_LEFT | DT_NOPREFIX | DT_WORDBREAK;
            let h_calc = DrawTextW(hdc, &mut wide, &mut rc, flags | DT_CALCRECT);
            if h_calc == 0 {
                SelectObject(hdc, old_font);
                let _ = DeleteObject(font.into());
                let _ = DeleteDC(hdc);
                ReleaseDC(None, hdc_screen);
                return None;
            }

            // Minimal pad — large pads + soft AA looked blurrier after crop.
            let pad = 1i32;
            let tw = (rc.right - rc.left).clamp(1, max_w) + pad * 2;
            let th = (rc.bottom - rc.top).clamp(1, 4096) + pad * 2;

            // Top-down 32-bit DIB — draw and read the same buffer (no GetDIBits scale).
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: tw,
                    biHeight: -th,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0 as u32,
                    biSizeImage: (tw * th * 4) as u32,
                    biXPelsPerMeter: 0,
                    biYPelsPerMeter: 0,
                    biClrUsed: 0,
                    biClrImportant: 0,
                },
                bmiColors: [Default::default(); 1],
            };
            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let hbmp = match CreateDIBSection(
                Some(hdc),
                &bmi,
                DIB_RGB_COLORS,
                &mut bits,
                None,
                0,
            ) {
                Ok(h) if !h.is_invalid() && !bits.is_null() => h,
                _ => {
                    SelectObject(hdc, old_font);
                    let _ = DeleteObject(font.into());
                    let _ = DeleteDC(hdc);
                    ReleaseDC(None, hdc_screen);
                    return None;
                }
            };
            let old_bmp = SelectObject(hdc, hbmp.into());

            // White background, black text (standard GDI AA path).
            {
                let brush = CreateSolidBrush(COLORREF(0x00FFFFFF));
                let fill = RECT {
                    left: 0,
                    top: 0,
                    right: tw,
                    bottom: th,
                };
                let _ = FillRect(hdc, &fill, brush);
                let _ = DeleteObject(brush.into());
            }

            let _ = SetBkMode(hdc, TRANSPARENT);
            let _ = SetTextColor(hdc, COLORREF(0x00000000)); // black BGR

            let mut text_rc = RECT {
                left: pad,
                top: pad,
                right: pad + (rc.right - rc.left),
                bottom: pad + (rc.bottom - rc.top),
            };
            let _ = DrawTextW(hdc, &mut wide, &mut text_rc, flags);

            // Sample DIB: BGRA, white bg → coverage from darkness.
            let n = (tw * th) as usize;
            let bgra = std::slice::from_raw_parts(bits as *const u8, n * 4);
            let mut rgba = vec![0u8; n * 4];
            for i in 0..n {
                let o = i * 4;
                let b = bgra[o] as f32;
                let g = bgra[o + 1] as f32;
                let r = bgra[o + 2] as f32;
                // Luma of white-bg glyph (1 = empty, 0 = full ink).
                let gray = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0;
                let a = coverage_to_alpha(gray, color[3]);
                rgba[o] = color[0];
                rgba[o + 1] = color[1];
                rgba[o + 2] = color[2];
                rgba[o + 3] = a;
            }

            SelectObject(hdc, old_bmp);
            SelectObject(hdc, old_font);
            let _ = DeleteObject(hbmp.into());
            let _ = DeleteObject(font.into());
            let _ = DeleteDC(hdc);
            ReleaseDC(None, hdc_screen);

            crop_ink(&rgba, tw as u32, th as u32)
        }
    }
}

/// Crop RGBA buffer to non-transparent ink + 1px pad.
fn crop_ink(src: &[u8], w: u32, h: u32) -> Option<(u32, u32, Vec<u8>)> {
    if w == 0 || h == 0 || src.len() < (w * h * 4) as usize {
        return None;
    }
    let mut min_x = w as i32;
    let mut min_y = h as i32;
    let mut max_x = -1i32;
    let mut max_y = -1i32;
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let i = ((y as u32 * w + x as u32) * 4) as usize;
            if src[i + 3] > 10 {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }
    if max_x < min_x || max_y < min_y {
        return None;
    }
    let pad = 1i32;
    min_x = (min_x - pad).max(0);
    min_y = (min_y - pad).max(0);
    max_x = (max_x + pad).min(w as i32 - 1);
    max_y = (max_y + pad).min(h as i32 - 1);
    let nw = (max_x - min_x + 1) as u32;
    let nh = (max_y - min_y + 1) as u32;
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh {
        for x in 0..nw {
            let sx = min_x as u32 + x;
            let sy = min_y as u32 + y;
            let si = ((sy * w + sx) * 4) as usize;
            let di = ((y * nw + x) * 4) as usize;
            out[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
    Some((nw, nh, out))
}

static GDI_WARNED: OnceLock<()> = OnceLock::new();

/// Rasterize `text` into a tight RGBA buffer (ink bounds + 1px pad).
/// Returns (width, height, pixels).
pub fn rasterize_text(
    text: &str,
    max_width: u32,
    px: f32,
    color: [u8; 4],
) -> Option<(u32, u32, Vec<u8>)> {
    #[cfg(windows)]
    {
        if let Some(r) = gdi::rasterize(text, max_width, px, color) {
            return Some(r);
        }
        let _ = GDI_WARNED.get_or_init(|| {
            warn!("GDI text rasterize failed; UI labels may be missing");
        });
        return None;
    }
    #[cfg(not(windows))]
    {
        let _ = (text, max_width, px, color);
        None
    }
}

/// Top-left to center a (tw×th) glyph block inside rect (x,y,w,h).
/// `optical_dy`: slight downward bias often reads better for CJK in buttons.
pub fn center_in_rect(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    tw: u32,
    th: u32,
    optical_dy: f32,
) -> (u32, u32) {
    let tx = (x + (w - tw as f32) * 0.5).round().max(0.0) as u32;
    let ty = (y + (h - th as f32) * 0.5 + optical_dy).round().max(0.0) as u32;
    (tx, ty)
}
