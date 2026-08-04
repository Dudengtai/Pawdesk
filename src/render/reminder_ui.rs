//! Compose reminder window bitmap (panel + pet + text + food button).

use super::text::rasterize_text;
use crate::pet::{FOOD_BUTTON_SIZE, REMINDER_WINDOW_H, REMINDER_WINDOW_W};

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
