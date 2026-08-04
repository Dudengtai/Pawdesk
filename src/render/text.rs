//! System-font text rasterization via fontdue.
//!
//! Critical: place glyphs by their real ymin/height relative to a baseline,
//! then crop to ink. Earlier “ascent=0.92×px” logic clipped the lower half of
//! CJK (simhei ymin≈−2, height≈em), making labels look garbled.

use std::path::PathBuf;
use std::sync::OnceLock;

use fontdue::{Font, FontSettings};
use tracing::warn;

static FONT: OnceLock<Option<Font>> = OnceLock::new();

fn load_font() -> Option<Font> {
    // Prefer clear CJK UI faces. TTC collection_index must match a real face.
    let candidates: &[(&str, u32)] = &[
        (r"C:\Windows\Fonts\msyh.ttc", 0), // Microsoft YaHei
        (r"C:\Windows\Fonts\msyh.ttc", 1),
        (r"C:\Windows\Fonts\Deng.ttf", 0), // DengXian
        (r"C:\Windows\Fonts\simhei.ttf", 0),
        (r"C:\Windows\Fonts\simsun.ttc", 0),
        (r"C:\Windows\Fonts\msyhbd.ttc", 0),
        (r"C:\Windows\Fonts\segoeui.ttf", 0),
    ];
    for &(path, collection_index) in candidates {
        let path = PathBuf::from(path);
        if !path.exists() {
            continue;
        }
        match std::fs::read(&path) {
            Ok(bytes) => {
                let settings = FontSettings {
                    collection_index,
                    scale: 40.0,
                    ..FontSettings::default()
                };
                match Font::from_bytes(bytes.as_slice(), settings) {
                    Ok(font) => {
                        // Must produce a solid CJK glyph
                        let (m, bmp) = font.rasterize('管', 20.0);
                        let ink = bmp.iter().filter(|&&v| v > 16).count();
                        if ink > 40 && m.width > 8 && m.height > 8 {
                            tracing::info!(
                                path = %path.display(),
                                collection_index,
                                ink,
                                "loaded UI font (CJK ok)"
                            );
                            return Some(font);
                        }
                        warn!(
                            path = %path.display(),
                            collection_index,
                            ink,
                            "font loaded but CJK weak; trying next"
                        );
                    }
                    Err(e) => {
                        warn!(path = %path.display(), error = %e, "font load failed");
                    }
                }
            }
            Err(e) => warn!(path = %path.display(), error = %e, "font read failed"),
        }
    }
    warn!("no system font loaded; UI text will be omitted");
    None
}

fn font() -> Option<&'static Font> {
    FONT.get_or_init(load_font).as_ref()
}

struct GlyphInk {
    ch: char,
    metrics: fontdue::Metrics,
    bitmap: Vec<u8>,
}

/// Rasterize `text` into a tight RGBA buffer (ink bounds + 1px pad).
/// Returns (width, height, pixels).
pub fn rasterize_text(
    text: &str,
    max_width: u32,
    px: f32,
    color: [u8; 4],
) -> Option<(u32, u32, Vec<u8>)> {
    let font = font()?;
    if text.is_empty() || px < 4.0 {
        return None;
    }

    // ── 1) wrap into lines by advance width ──────────────────────────────
    let mut lines: Vec<Vec<GlyphInk>> = Vec::new();
    let mut cur: Vec<GlyphInk> = Vec::new();
    let mut cur_w = 0.0f32;

    for ch in text.chars() {
        if ch == '\n' {
            lines.push(std::mem::take(&mut cur));
            cur_w = 0.0;
            continue;
        }
        let (metrics, bitmap) = font.rasterize(ch, px);
        let adv = metrics.advance_width.max(1.0);
        if cur_w + adv > max_width as f32 && !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
            cur_w = 0.0;
        }
        cur_w += adv;
        cur.push(GlyphInk {
            ch,
            metrics,
            bitmap,
        });
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.is_empty() {
        return None;
    }

    // ── 2) measure tight bounds relative to a shared baseline per line ───
    // Glyph y range vs baseline: [ymin, ymin+height]
    let mut global_min_y = 0i32;
    let mut global_max_y = 0i32;
    let mut max_line_w = 1.0f32;
    let mut any = false;

    for line in &lines {
        let mut w = 0.0f32;
        for g in line {
            w += g.metrics.advance_width.max(1.0);
            if g.metrics.width == 0 || g.metrics.height == 0 {
                continue;
            }
            let top = g.metrics.ymin;
            let bot = g.metrics.ymin + g.metrics.height as i32;
            if !any {
                global_min_y = top;
                global_max_y = bot;
                any = true;
            } else {
                global_min_y = global_min_y.min(top);
                global_max_y = global_max_y.max(bot);
            }
        }
        max_line_w = max_line_w.max(w);
    }

    // Fall back to line metrics if glyphs were empty (spaces only)
    let (ascent, descent) = if let Some(lm) = font.horizontal_line_metrics(px) {
        // fontdue: ascent > 0 above baseline; descent typically negative
        (lm.ascent.ceil() as i32, lm.descent.floor() as i32)
    } else {
        ((px * 0.85).ceil() as i32, -((px * 0.15).ceil() as i32))
    };

    if !any {
        // descent is typically negative (below baseline)
        global_min_y = -ascent;
        global_max_y = (-descent).max(1);
    }

    // Ensure we cover at least font line box
    global_min_y = global_min_y.min(-ascent);
    global_max_y = global_max_y.max((-descent).max(1));

    let glyph_span = (global_max_y - global_min_y).max(px.ceil() as i32);
    // Line pitch: glyph span + small gap (not 1.45× which wasted space)
    let line_gap = (px * 0.25).ceil() as i32;
    let line_pitch = glyph_span + line_gap;

    let pad = 2i32;
    let width = (max_line_w.ceil() as i32 + pad * 2)
        .clamp(4, (max_width.max(4) as i32) + pad * 2) as u32;
    let height = (lines.len() as i32 * line_pitch - line_gap + pad * 2).max(4) as u32;

    let mut rgba = vec![0u8; (width * height * 4) as usize];

    // ── 3) paint each glyph ──────────────────────────────────────────────
    // baseline_y for line 0: pad - global_min_y  (so top of tallest glyph at pad)
    for (li, line) in lines.iter().enumerate() {
        let baseline = pad - global_min_y + li as i32 * line_pitch;
        let mut x = pad as f32;
        for g in line {
            if g.metrics.width == 0 || g.metrics.height == 0 {
                x += g.metrics.advance_width.max(1.0);
                continue;
            }
            let gx = x as i32 + g.metrics.xmin;
            let gy = baseline + g.metrics.ymin;
            let gw = g.metrics.width as i32;
            let gh = g.metrics.height as i32;
            for row in 0..gh {
                for col in 0..gw {
                    let px_x = gx + col;
                    let px_y = gy + row;
                    if px_x < 0 || px_y < 0 || px_x >= width as i32 || px_y >= height as i32 {
                        continue;
                    }
                    let idx = (row * gw + col) as usize;
                    if idx >= g.bitmap.len() {
                        continue;
                    }
                    // Gamma > 1 tightens AA fringes so type looks less soft/fuzzy.
                    let raw = g.bitmap[idx] as f32 / 255.0;
                    if raw < 0.06 {
                        continue;
                    }
                    let coverage = raw.powf(1.25);
                    let i = ((px_y as u32 * width + px_x as u32) * 4) as usize;
                    let a = (color[3] as f32 * coverage) as u8;
                    // Max-blend for crisp CJK strokes
                    if a > rgba[i + 3] {
                        rgba[i] = color[0];
                        rgba[i + 1] = color[1];
                        rgba[i + 2] = color[2];
                        rgba[i + 3] = a;
                    }
                }
            }
            x += g.metrics.advance_width.max(1.0);
            let _ = g.ch;
        }
    }

    crop_ink(&rgba, width, height)
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
            if src[i + 3] > 8 {
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
