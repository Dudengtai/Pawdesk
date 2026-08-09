//! Font display QA: compare face/weight combos exactly as text.rs draws them
//! (GDI black-on-white DIB → coverage curve), and report objective legibility
//! stats + a composite PNG for OCR/visual checks.
//!
//! Run: cargo run --release --example font_display_qa
//! Output: target/font_qa/old_vs_new.png (+ stats on stdout)

use image::{Rgba, RgbaImage};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    AddFontResourceExW, CreateCompatibleDC, CreateDIBSection, CreateFontW, CreateSolidBrush,
    DeleteDC, DeleteObject, DrawTextW, EnumFontFamiliesExW, FillRect, FR_PRIVATE, GetDC,
    GetObjectW, ReleaseDC, SelectObject, SetBkMode, SetTextColor, ANTIALIASED_QUALITY, BI_RGB,
    BITMAPINFO, BITMAPINFOHEADER, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH,
    DIB_RGB_COLORS, DT_CALCRECT, DT_LEFT, DT_NOPREFIX, DT_WORDBREAK, FF_DONTCARE, FW_BOLD,
    FW_MEDIUM, FW_SEMIBOLD, HFONT, LOGFONTW, OUT_TT_PRECIS, TEXTMETRICW, TRANSPARENT,
};

fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

/// Register assets/fonts/*.ttf FR_PRIVATE, mirroring text.rs's loader.
fn register_bundled(fonts_dir: &str) {
    if let Ok(rd) = std::fs::read_dir(fonts_dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            let Some(ext) = p.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            if !matches!(ext.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc") {
                continue;
            }
            let name = to_wide(&p.to_string_lossy());
            let added = unsafe {
                AddFontResourceExW(PCWSTR(name.as_ptr()), FR_PRIVATE, None)
            };
            println!("register {} → added={added}", p.display());
        }
    }
}

fn make_font(face: &str, px: i32, weight: i32) -> HFONT {
    let name = to_wide(face);
    unsafe {
        CreateFontW(
            -px.abs().max(9),
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_TT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            DEFAULT_PITCH.0 as u32 | FF_DONTCARE.0 as u32,
            PCWSTR(name.as_ptr()),
        )
    }
}

/// Ask GDI what weight it actually resolved for a face+request combo.
fn resolved_weight(face: &str, px: i32, weight: i32) -> (String, i32) {
    let hdc = unsafe { GetDC(None) };
    if hdc.0.is_null() {
        return (face.to_string(), weight);
    }
    let font = make_font(face, px, weight);
    let mut lf = LOGFONTW::default();
    unsafe {
        let _ = GetObjectW(font.into(), std::mem::size_of::<LOGFONTW>() as i32, Some(&mut lf as *mut _ as *mut core::ffi::c_void));
        ReleaseDC(None, hdc);
    }
    let _ = unsafe { DeleteObject(font.into()) };
    let mut name = String::new();
    for &u in lf.lfFaceName.iter() {
        if u == 0 {
            break;
        }
        name.push(char::from_u32(u as u32).unwrap_or('?'));
    }
    (name, lf.lfWeight)
}

/// Faithful copy of text.rs: black-on-white DIB + DrawTextW → coverage → RGBA.
fn render(font: HFONT, text: &str, max_w: u32) -> (u32, u32, Vec<u8>) {
    unsafe {
        let hdc_screen = GetDC(None);
        let hdc = CreateCompatibleDC(Some(hdc_screen));
        let old_font = SelectObject(hdc, font.into());

        let mut wide: Vec<u16> = text.encode_utf16().collect();
        let mut rc = RECT { left: 0, top: 0, right: max_w.max(8) as i32, bottom: 4096 };
        let flags = DT_LEFT | DT_NOPREFIX | DT_WORDBREAK;
        let h_calc = DrawTextW(hdc, &mut wide, &mut rc, flags | DT_CALCRECT);
        if h_calc == 0 {
            return (0, 0, Vec::new());
        }
        let pad = 1i32;
        let tw = (rc.right - rc.left).clamp(1, max_w as i32) + pad * 2;
        let th = (rc.bottom - rc.top).clamp(1, 4096) + pad * 2;

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
        let hbmp = CreateDIBSection(Some(hdc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
            .unwrap_or_default();
        let old_bmp = SelectObject(hdc, hbmp.into());

        let brush = CreateSolidBrush(COLORREF(0x00FFFFFF));
        let fill = RECT { left: 0, top: 0, right: tw, bottom: th };
        let _ = FillRect(hdc, &fill, brush);
        let _ = DeleteObject(brush.into());
        let _ = SetBkMode(hdc, TRANSPARENT);
        let _ = SetTextColor(hdc, COLORREF(0x00000000));

        let mut text_rc = RECT { left: pad, top: pad, right: pad + (rc.right - rc.left), bottom: pad + (rc.bottom - rc.top) };
        let _ = DrawTextW(hdc, &mut wide, &mut text_rc, flags);

        let n = (tw * th) as usize;
        let bgra = std::slice::from_raw_parts(bits as *const u8, n * 4);
        let mut rgba = vec![0u8; n * 4];
        for i in 0..n {
            let o = i * 4;
            let b = bgra[o] as f32;
            let g = bgra[o + 1] as f32;
            let r = bgra[o + 2] as f32;
            let gray = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0;
            let mut cov = (1.0 - gray).clamp(0.0, 1.0);
            if cov < 0.14 {
                cov = 0.0;
            } else {
                cov = ((cov - 0.14) / 0.86).clamp(0.0, 1.0).powf(0.58);
            }
            let a = (255.0 * cov).round().clamp(0.0, 255.0) as u8;
            rgba[o] = 0x0F;
            rgba[o + 1] = 0x17;
            rgba[o + 2] = 0x2B;
            rgba[o + 3] = a;
        }
        let _ = SelectObject(hdc, old_bmp);
        let _ = SelectObject(hdc, old_font);
        let _ = DeleteObject(hbmp.into());
        let _ = DeleteObject(font.into());
        let _ = DeleteDC(hdc);
        let _ = ReleaseDC(None, hdc_screen);
        (tw as u32, th as u32, rgba)
    }
}

#[derive(Default)]
struct Stats {
    ink: u32,      // alpha >= 128
    soft: u32,     // 32 <= alpha < 128 (AA fringe)
    max_stem: u32, // widest horizontal run of ink
}

fn stats(rgba: &[u8], w: u32, h: u32) -> Stats {
    let mut s = Stats::default();
    for y in 0..h {
        let mut run = 0u32;
        for x in 0..w {
            let a = rgba[((y * w + x) * 4 + 3) as usize];
            if a >= 128 {
                s.ink += 1;
                run += 1;
                s.max_stem = s.max_stem.max(run);
            } else {
                run = 0;
                if a >= 32 {
                    s.soft += 1;
                }
            }
        }
    }
    s
}

fn main() {
    std::fs::create_dir_all("target/font_qa").ok();

    // Register bundled font exactly like text.rs does.
    let fonts_dir = format!("{}\\assets\\fonts", env!("CARGO_MANIFEST_DIR"));
    register_bundled(&fonts_dir);

    // (label, face, weight, note)
    let combos: &[(&str, &str, i32, &str)] = &[
        ("old", "Microsoft YaHei UI", FW_MEDIUM.0 as i32, "现状：雅黑 Medium(500)"),
        ("new", "Microsoft YaHei UI", FW_SEMIBOLD.0 as i32, "新方案：雅黑 Semibold(600)"),
        ("hos", "HarmonyOS Sans SC", FW_SEMIBOLD.0 as i32, "打包：鸿蒙 Semibold(600)"),
        ("bold", "Microsoft YaHei UI", FW_BOLD.0 as i32, "参考上限：雅黑 Bold(700)"),
    ];

    // Settings panel strings at DPR 2.0 (screenshot was 840x1280 = 420x640 @ 2x).
    let samples: &[(&str, f32)] = &[
        ("设置", 20.0),
        ("提醒与常用应用", 13.5),
        ("健康提醒", 13.5),
        ("●  启用提醒", 15.0),
        ("15 分钟", 15.0),
        ("宠物大小", 13.5),
        ("70%", 16.0),
        ("常用应用", 13.5),
        ("●  Spotify", 14.0),
        ("●  Docker Desktop", 14.0),
        ("相对默认尺寸 · 托盘也可调", 12.5),
        ("添加应用", 15.0),
        ("运行中", 13.5),
    ];
    const DPR: f32 = 2.0;

    println!("== GDI resolved weight (face, request → actual) ==");
    for &(_, face, w, _) in combos {
        let (name, rw) = resolved_weight(face, 26, w);
        println!("  {face} req={w} → face='{name}' weight={rw}");
    }

    // Composite: rows per combo, columns per sample.
    let col_w = 400u32;
    let row_h = 56u32;
    let mut img = RgbaImage::from_pixel(col_w * 4 + 8, row_h * (samples.len() as u32 + 1) + 8, Rgba([245, 245, 245, 255]));

    println!("\n== legibility stats (px = logical pt × {DPR}) ==");
    println!("{:<26} {:<6} {:>4} {:>6} {:>6} {:>6}", "text", "combo", "w", "ink%", "soft%", "stem");
    for (row, &(text, pt)) in samples.iter().enumerate() {
        let px = (pt * DPR + 0.35).round().clamp(9.0, 96.0) as i32;
        for (col, &(label, face, weight, _)) in combos.iter().enumerate() {
            let font = make_font(face, px, weight);
            let (tw, th, rgba) = render(font, text, 400);
            let s = stats(&rgba, tw, th);
            let total = (tw * th).max(1) as f32;
            println!(
                "{:<26} {:<6} {:>4} {:>6.2} {:>6.2} {:>4}",
                text, label, tw, s.ink as f32 / total * 100.0, s.soft as f32 / total * 100.0, s.max_stem
            );
            // Blit into composite
            let dx = 4 + col as u32 * col_w + (col_w - tw) / 2;
            let dy = 4 + row as u32 * row_h + (row_h - th) / 2;
            for y in 0..th {
                for x in 0..tw {
                    let i = ((y * tw + x) * 4) as usize;
                    let a = rgba[i + 3] as f32 / 255.0;
                    if a < 0.05 {
                        continue;
                    }
                    let tx = dx + x;
                    let ty = dy + y;
                    if tx >= img.width() || ty >= img.height() {
                        continue;
                    }
                    let p = img.get_pixel_mut(tx, ty);
                    for k in 0..3 {
                        p.0[k] = (rgba[i + k] as f32 * a + p.0[k] as f32 * (1.0 - a)) as u8;
                    }
                }
            }
        }
    }
    let path = "target/font_qa/old_vs_new.png";
    img.save(path).unwrap();
    println!("\nwrote {path}");
    println!("columns: old=雅黑500  new=雅黑600  hos=鸿蒙600(bundled)  bold=雅黑700  (rows: settings strings @2x)");
}
