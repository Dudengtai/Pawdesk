//! Compose reminder window bitmap (panel + pet + text + food button).
//!
//! `ReminderCard` path: the whole-image mockup `tishi.png` replaces the
//! programmatic card (option A). `compose_reminder_frame` stays as fallback.

use std::collections::VecDeque;
use std::path::Path;

use image::imageops::FilterType;

use super::text::rasterize_text;
use crate::pet::{FOOD_BUTTON_SIZE, REMINDER_WINDOW_H, REMINDER_WINDOW_W};

/// Preprocessed reminder card bitmap (already window-sized RGBA).
pub struct ReminderCard {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

/// Keying threshold: pixels this white and connected to the image border are
/// treated as paper background. 244 also drops the lightest anti-aliased rim
/// so the artwork doesn't get a white outline after downscaling.
const BG_MIN_RGB: u8 = 244;
/// Padding (source pixels) kept around the artwork when auto-cropping.
const CROP_PAD: u32 = 12;
/// Feed hint pill colors (match the mockup's warm cream / ink brown).
const HINT_BG: [u8; 4] = [0xFF, 0xF3, 0xE4, 0xCE];
const HINT_TEXT: [u8; 4] = [0x45, 0x40, 0x36, 0xFF];

/// Load and prepare the reminder card image for the reminder window.
///
/// The mockup has no alpha channel: near-white background (as seen from the
/// border) is removed with a border flood-fill, so warm-tinted fur and light
/// fills stay opaque. The artwork is then auto-cropped to its bounding box and
/// contain-fitted into the window (fills the height), so the cat reads big.
/// Color is downsampled premultiplied by alpha, which prevents white / dark
/// halos on the soft edges.
pub fn load_reminder_card(path: &Path, out_w: u32, out_h: u32) -> Option<ReminderCard> {
    let img = image::open(path).ok()?.to_rgba8();
    let (iw, ih) = img.dimensions();
    if iw == 0 || ih == 0 || out_w == 0 || out_h == 0 {
        return None;
    }
    let src = img.into_raw();

    let mask = remove_bg_flood(&src, iw, ih, BG_MIN_RGB);
    let (x0, y0, cw, ch) = artwork_bbox(&mask, iw, ih, CROP_PAD);
    if cw == 0 || ch == 0 {
        return None;
    }

    // Contain-fit (aspect-preserving) into the window, centered.
    let scale = (out_w as f32 / cw as f32).min(out_h as f32 / ch as f32).min(1.0);
    let sw = (cw as f32 * scale).round().max(1.0) as u32;
    let sh = (ch as f32 * scale).round().max(1.0) as u32;
    let dst_x = (out_w - sw) / 2;
    let dst_y = (out_h - sh) / 2;

    let (rgb, alpha) = downscale_card(&src, &mask, iw, x0, y0, cw, ch, sw, sh);

    let mut rgba = vec![0u8; (out_w * out_h * 4) as usize];
    for y in 0..sh {
        for x in 0..sw {
            let si = ((y * sw + x) * 3) as usize;
            let mi = (y * sw + x) as usize;
            let di = (((dst_y + y) * out_w + (dst_x + x)) * 4) as usize;
            rgba[di] = rgb[si];
            rgba[di + 1] = rgb[si + 1];
            rgba[di + 2] = rgb[si + 2];
            rgba[di + 3] = alpha[mi];
        }
    }
    Some(ReminderCard {
        w: out_w,
        h: out_h,
        rgba,
    })
}

/// Final reminder frame: the card image plus a subtle feed hint pill.
///
/// The mockup has no food button, but the feed loop still needs a discoverable
/// target; a small cream pill keeps the click zone obvious without covering
/// the artwork.
pub fn compose_reminder_card_frame(card: &ReminderCard, feeding: bool) -> (u32, u32, Vec<u8>) {
    let mut out = card.rgba.clone();
    let w = card.w;
    let h = card.h;
    let label = if feeding { "呼～活力恢复了！" } else { "点击投喂" };
    if let Some((tw, th, trgba)) = rasterize_text(label, w - 40, 15.0, HINT_TEXT) {
        let pad_x = 14u32;
        let pad_y = 6u32;
        let pw = tw + pad_x * 2;
        let ph = th + pad_y * 2;
        let x = ((w as i32 - pw as i32) / 2).max(0) as u32;
        let y = (h as i32 - ph as i32 - 12).max(0) as u32;
        fill_round_rect(&mut out, w, h, x, y, pw, ph, (ph / 2) as i32, HINT_BG);
        blit(&mut out, w, h, &trgba, tw, th, x + pad_x, y + pad_y);
    }
    (w, h, out)
}

/// Border flood-fill over near-white pixels. Returns an alpha mask:
/// 0 for the paper background, 255 elsewhere.
fn remove_bg_flood(src: &[u8], w: u32, h: u32, thr: u8) -> Vec<u8> {
    let (w, h) = (w as usize, h as usize);
    let is_bg = |x: usize, y: usize| {
        let i = (y * w + x) * 4;
        src[i].min(src[i + 1]).min(src[i + 2]) >= thr
    };
    let mut visited = vec![false; w * h];
    let mut queue = VecDeque::new();
    let seed = |x: usize, y: usize, visited: &mut Vec<bool>, queue: &mut VecDeque<(usize, usize)>| {
        if is_bg(x, y) && !visited[y * w + x] {
            visited[y * w + x] = true;
            queue.push_back((x, y));
        }
    };
    for x in 0..w {
        seed(x, 0, &mut visited, &mut queue);
        seed(x, h - 1, &mut visited, &mut queue);
    }
    for y in 0..h {
        seed(0, y, &mut visited, &mut queue);
        seed(w - 1, y, &mut visited, &mut queue);
    }
    while let Some((x, y)) = queue.pop_front() {
        for (dx, dy) in [(1i32, 0), (-1, 0), (0, 1), (0, -1)] {
            let nx = x as i32 + dx;
            let ny = y as i32 + dy;
            if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                continue;
            }
            let (nx, ny) = (nx as usize, ny as usize);
            if !visited[ny * w + nx] && is_bg(nx, ny) {
                visited[ny * w + nx] = true;
                queue.push_back((nx, ny));
            }
        }
    }
    visited.iter().map(|&v| if v { 0 } else { 255 }).collect()
}

/// Bounding box of opaque artwork pixels, padded by `pad` (clamped to image).
fn artwork_bbox(mask: &[u8], w: u32, h: u32, pad: u32) -> (u32, u32, u32, u32) {
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0u32, 0u32);
    for y in 0..h {
        let row = y * w;
        for x in 0..w {
            if mask[(row + x) as usize] != 0 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    if x1 <= x0 || y1 <= y0 {
        return (0, 0, 0, 0);
    }
    let px = |v: u32, lo: u32, hi: u32| v.clamp(lo, hi);
    let x0 = px(x0.saturating_sub(pad), 0, w.saturating_sub(1));
    let y0 = px(y0.saturating_sub(pad), 0, h.saturating_sub(1));
    let x1 = px(x1 + pad, 0, w.saturating_sub(1));
    let y1 = px(y1 + pad, 0, h.saturating_sub(1));
    (x0, y0, x1 - x0 + 1, y1 - y0 + 1)
}

/// Downscale the artwork crop to (sw, sh) premultiplied by alpha, then
/// un-premultiply. Dividing premultiplied colors by smoothed alpha keeps edge
/// pixels colored like the art instead of the white paper (no halo).
fn downscale_card(
    src: &[u8],
    mask: &[u8],
    iw: u32,
    x0: u32,
    y0: u32,
    cw: u32,
    ch: u32,
    sw: u32,
    sh: u32,
) -> (Vec<u8>, Vec<u8>) {
    let (cw, ch) = (cw as usize, ch as usize);
    let iw = iw as usize;
    let mut pm = Vec::with_capacity(cw * ch * 3);
    let mut alpha_crop = Vec::with_capacity(cw * ch);
    for y in 0..ch {
        let sy = (y0 as usize + y) * iw;
        for x in 0..cw {
            let s = (sy + x0 as usize + x) * 4;
            let a = mask[(sy + x0 as usize + x) as usize] as u32;
            alpha_crop.push(a as u8);
            pm.push((src[s] as u32 * a / 255) as u8);
            pm.push((src[s + 1] as u32 * a / 255) as u8);
            pm.push((src[s + 2] as u32 * a / 255) as u8);
        }
    }
    let pm_img = image::RgbImage::from_raw(cw as u32, ch as u32, pm)
        .expect("premultiplied buffer length matches crop size");
    let al_img = image::GrayImage::from_raw(cw as u32, ch as u32, alpha_crop)
        .expect("alpha buffer length matches crop size");
    let pm_small = image::imageops::resize(&pm_img, sw, sh, FilterType::CatmullRom);
    let al_small = image::imageops::resize(&al_img, sw, sh, FilterType::Triangle);
    let pm_raw = pm_small.into_raw();
    let al_raw = al_small.into_raw();

    let mut rgb = Vec::with_capacity(pm_raw.len());
    for (i, px) in pm_raw.chunks_exact(3).enumerate() {
        let a = al_raw[i] as u32;
        if a == 0 {
            rgb.extend_from_slice(&[0, 0, 0]);
        } else {
            rgb.push(((px[0] as u32 * 255) / a).min(255) as u8);
            rgb.push(((px[1] as u32 * 255) / a).min(255) as u8);
            rgb.push(((px[2] as u32 * 255) / a).min(255) as u8);
        }
    }
    (rgb, al_raw)
}

/// Design tokens (light theme).
const PANEL: [u8; 4] = [0xFF, 0xF8, 0xF2, 0xF5]; // nearly opaque cream
const ACCENT: [u8; 4] = [0xFF, 0x6B, 0xA8, 0xFF]; // strong pink
const ACCENT_RING: [u8; 4] = [0xFF, 0xD0, 0xE4, 0xFF];
const TEXT: [u8; 4] = [0x3A, 0x35, 0x40, 0xFF];
const WHITE: [u8; 4] = [0xFF, 0xFF, 0xFF, 0xFF];
const ON_ACCENT: [u8; 4] = [0xFF, 0xFF, 0xFF, 0xFF];

/// Build a full-window RGBA for the reminder Showing/Feeding stage.
pub fn compose_reminder_frame(
    pet_rgba: &[u8],
    pet_w: u32,
    pet_h: u32,
    message: &str,
    button_scale: f32,
    feeding: bool,
) -> (u32, u32, Vec<u8>) {
    let w = REMINDER_WINDOW_W;
    let h = REMINDER_WINDOW_H;
    let mut out = vec![0u8; (w * h * 4) as usize];

    // Soft panel card (opaque enough to read on desktop wallpaper).
    fill_round_rect(&mut out, w, h, 12, 64, w - 24, h - 76, 18, PANEL);

    // Pet near top-center.
    let pet_x = ((w as i32 - pet_w as i32) / 2).max(0) as u32;
    let pet_y = 4u32;
    blit(&mut out, w, h, pet_rgba, pet_w, pet_h, pet_x, pet_y);

    // Message text on panel.
    if !feeding {
        if let Some((tw, th, trgba)) = rasterize_text(message, w - 40, 15.0, TEXT) {
            blit(&mut out, w, h, &trgba, tw, th, 20, 132);
        }
    } else if let Some((tw, th, trgba)) = rasterize_text("呼～活力恢复了！", w - 40, 18.0, TEXT)
    {
        blit(&mut out, w, h, &trgba, tw, th, 20, 150);
    }

    // Large food button (must stay fully inside the bitmap).
    if !feeding {
        let (bx, by, bs, _) = food_button_layout();
        let s = (bs * button_scale).max(40.0) as i32;
        let cx = (bx + bs * 0.5) as i32;
        let cy = (by + bs * 0.5) as i32;
        // Outer ring for visibility
        fill_circle(&mut out, w, h, cx, cy, s / 2 + 4, ACCENT_RING);
        fill_circle(&mut out, w, h, cx, cy, s / 2, ACCENT);
        // Fish / food glyph
        fill_circle(&mut out, w, h, cx - 6, cy - 2, s / 6, WHITE);
        fill_circle(&mut out, w, h, cx + 8, cy, s / 8, WHITE);
        // Label under icon
        if let Some((tw, th, trgba)) = rasterize_text("点击投喂", 120, 14.0, ON_ACCENT) {
            let tx = (cx - tw as i32 / 2).max(0) as u32;
            let ty = (cy + s / 6).max(0) as u32;
            // Dark label below button for contrast
            if let Some((tw2, th2, trgba2)) = rasterize_text("点击投喂", 120, 14.0, TEXT) {
                let tx2 = ((w as i32 - tw2 as i32) / 2).max(0) as u32;
                let ty2 = (by as u32 + bs as u32 + 4).min(h.saturating_sub(th2));
                blit(&mut out, w, h, &trgba2, tw2, th2, tx2, ty2);
            }
            let _ = (tx, ty, trgba, th);
        }
    }

    (w, h, out)
}

/// Food button rect in reminder layout coordinates (360×260).
pub fn food_button_layout() -> (f32, f32, f32, f32) {
    let s = FOOD_BUTTON_SIZE;
    let x = (REMINDER_WINDOW_W as f32 - s) * 0.5;
    // Keep fully above the bottom edge so it is never cropped.
    let y = REMINDER_WINDOW_H as f32 - s - 36.0;
    (x, y, s, s)
}

/// Map a window-client physical pixel into reminder layout space.
pub fn client_to_layout(local_x: f64, local_y: f64, client_w: f64, client_h: f64) -> (f64, f64) {
    let w = client_w.max(1.0);
    let h = client_h.max(1.0);
    (
        local_x / w * REMINDER_WINDOW_W as f64,
        local_y / h * REMINDER_WINDOW_H as f64,
    )
}

fn put(px: &mut [u8], w: u32, x: i32, y: i32, c: [u8; 4]) {
    if x < 0 || y < 0 || x >= w as i32 {
        return;
    }
    let h = (px.len() / 4) as u32 / w;
    if y >= h as i32 {
        return;
    }
    let i = ((y as u32 * w + x as u32) * 4) as usize;
    // Alpha blend over existing.
    let src_a = c[3] as f32 / 255.0;
    if src_a <= 0.0 {
        return;
    }
    let dst_a = px[i + 3] as f32 / 255.0;
    let out_a = src_a + dst_a * (1.0 - src_a);
    if out_a <= 0.0 {
        return;
    }
    for k in 0..3 {
        let s = c[k] as f32 / 255.0;
        let d = px[i + k] as f32 / 255.0;
        let v = (s * src_a + d * dst_a * (1.0 - src_a)) / out_a;
        px[i + k] = (v * 255.0) as u8;
    }
    px[i + 3] = (out_a * 255.0) as u8;
}

#[allow(clippy::too_many_arguments)]
fn fill_round_rect(
    px: &mut [u8],
    w: u32,
    h: u32,
    x: u32,
    y: u32,
    rw: u32,
    rh: u32,
    radius: i32,
    c: [u8; 4],
) {
    let x0 = x as i32;
    let y0 = y as i32;
    let x1 = (x + rw) as i32;
    let y1 = (y + rh).min(h) as i32;
    let r = radius;
    for py in y0..y1 {
        for px_ in x0..x1 {
            let in_corner = {
                let cx = if px_ < x0 + r {
                    x0 + r
                } else if px_ >= x1 - r {
                    x1 - r - 1
                } else {
                    px_
                };
                let cy = if py < y0 + r {
                    y0 + r
                } else if py >= y1 - r {
                    y1 - r - 1
                } else {
                    py
                };
                if (px_ < x0 + r || px_ >= x1 - r) && (py < y0 + r || py >= y1 - r) {
                    let dx = px_ - cx;
                    let dy = py - cy;
                    dx * dx + dy * dy <= r * r
                } else {
                    true
                }
            };
            if in_corner {
                put(px, w, px_, py, c);
            }
        }
    }
}

fn fill_circle(px: &mut [u8], w: u32, h: u32, cx: i32, cy: i32, radius: i32, c: [u8; 4]) {
    let r2 = radius * radius;
    for y in (cy - radius)..(cy + radius + 1) {
        for x in (cx - radius)..(cx + radius + 1) {
            if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
                continue;
            }
            let dx = x - cx;
            let dy = y - cy;
            if dx * dx + dy * dy <= r2 {
                put(px, w, x, y, c);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn blit(dest: &mut [u8], dw: u32, dh: u32, src: &[u8], sw: u32, sh: u32, dx: u32, dy: u32) {
    for y in 0..sh {
        for x in 0..sw {
            let sx = x;
            let sy = y;
            let tx = dx + x;
            let ty = dy + y;
            if tx >= dw || ty >= dh {
                continue;
            }
            let si = ((sy * sw + sx) * 4) as usize;
            if si + 3 >= src.len() {
                continue;
            }
            let c = [src[si], src[si + 1], src[si + 2], src[si + 3]];
            if c[3] == 0 {
                continue;
            }
            put(dest, dw, tx as i32, ty as i32, c);
        }
    }
}

#[cfg(test)]
mod card_tests {
    use super::*;
    use std::path::PathBuf;

    fn card_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/ui/reminder_card.png")
    }

    fn opaque_frac(rgba: &[u8]) -> f64 {
        let n = rgba.chunks_exact(4).count() as f64;
        let opaque = rgba.chunks_exact(4).filter(|p| p[3] > 200).count() as f64;
        opaque / n.max(1.0)
    }

    #[test]
    fn card_loads_to_window_size() {
        let card = load_reminder_card(&card_path(), REMINDER_WINDOW_W, REMINDER_WINDOW_H)
            .expect("reminder card should load");
        assert_eq!((card.w, card.h), (REMINDER_WINDOW_W, REMINDER_WINDOW_H));
        assert_eq!(card.rgba.len(), (card.w * card.h * 4) as usize);
        // White paper background is keyed out…
        let transparent = card.rgba.chunks_exact(4).filter(|p| p[3] == 0).count();
        assert!(transparent > 0, "white background should be transparent");
        // …while the bubble + cat + tail remain (matches the mockup artwork).
        let frac = opaque_frac(&card.rgba);
        assert!(frac > 0.25 && frac < 0.55, "opaque fraction {frac:.2}");
    }

    #[test]
    fn card_frame_has_feed_hint() {
        let card = load_reminder_card(&card_path(), REMINDER_WINDOW_W, REMINDER_WINDOW_H).unwrap();
        let (w, h, frame) = compose_reminder_card_frame(&card, false);
        assert_eq!((w, h), (REMINDER_WINDOW_W, REMINDER_WINDOW_H));
        assert!(frame != card.rgba, "feed hint pill should alter the frame");
        // Feeding label must not panic and should also draw.
        let (_, _, fed) = compose_reminder_card_frame(&card, true);
        assert_ne!(fed, frame);
    }

    #[test]
    fn card_missing_falls_back() {
        assert!(load_reminder_card(Path::new("nope/absent.png"), 10, 10).is_none());
        assert!(load_reminder_card(&card_path(), 0, 0).is_none());
    }

    /// Dev preview for iterating on the card asset: writes a PNG (and stats)
    /// under target/ so the composed reminder frame can be inspected.
    #[test]
    #[ignore]
    fn dump_card_preview() {
        let card = load_reminder_card(&card_path(), REMINDER_WINDOW_W, REMINDER_WINDOW_H).unwrap();
        let (w, h, frame) = compose_reminder_card_frame(&card, false);
        let out = {
            let mut img = image::RgbaImage::new(w, h);
            for (i, px) in img.pixels_mut().enumerate() {
                let s = i * 4;
                *px = image::Rgba([
                    frame[s],
                    frame[s + 1],
                    frame[s + 2],
                    frame[s + 3],
                ]);
            }
            img
        };
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/card_preview.png");
        out.save(&path).unwrap();
        eprintln!(
            "card {}x{} opaque={:.1}% saved to {}",
            w,
            h,
            opaque_frac(&frame) * 100.0,
            path.display()
        );
    }
}
