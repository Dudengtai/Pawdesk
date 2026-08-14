//! Comic speech bubble for the idle yawn oneshot.

use crate::platform::Rect;
use crate::render::menu_ui::blit_rgba;
use crate::render::text::rasterize_text;
use crate::ui::launcher_place::{logical_to_physical, snap_dpr};

pub const YAWN_TEXT: &str = "困死我了…";
pub const BUBBLE_LOGICAL_W: u32 = 112;
pub const BUBBLE_LOGICAL_H: u32 = 44;
const GAP: i32 = 6;

const CREAM: [u8; 4] = [0xF6, 0xEC, 0xD8, 0xFF];
const INK: [u8; 4] = [0x2A, 0x22, 0x1C, 0xFF];
const TEXT: [u8; 4] = [0x2A, 0x22, 0x1C, 0xFF];

#[derive(Debug, Clone, Copy)]
pub struct YawnPlacement {
    pub window: Rect,
    pub pet_local: (i32, i32),
    pub bubble_local: (i32, i32),
    pub bubble_w: i32,
    pub bubble_h: i32,
    pub bubble_on_left: bool,
}

/// Pin the pet. Prefer growing to the right so the window origin does not move
/// (moving a layered HWND left leaves a DWM ghost of the previous pet).
pub fn place_yawn_bubble(pet: Rect, work: Rect, dpr: f64) -> YawnPlacement {
    let dpr = snap_dpr(dpr);
    let bw = logical_to_physical(BUBBLE_LOGICAL_W, dpr);
    let bh = logical_to_physical(BUBBLE_LOGICAL_H, dpr);
    let gap = logical_to_physical(GAP as u32, dpr);
    let head_y = (pet.height / 8).max(4);

    let fits_right = pet.x + pet.width + gap + bw <= work.x + work.width - 4;
    let bubble_on_left = !fits_right;
    let win_h = pet.height.max(bh + head_y);
    let (win_x, pet_lx, bubble_lx) = if bubble_on_left {
        (pet.x - gap - bw, gap + bw, 0)
    } else {
        (pet.x, 0, pet.width + gap)
    };
    YawnPlacement {
        window: Rect {
            x: win_x,
            y: pet.y,
            width: pet.width + gap + bw,
            height: win_h,
        },
        pet_local: (pet_lx, 0),
        bubble_local: (bubble_lx, head_y),
        bubble_w: bw,
        bubble_h: bh,
        bubble_on_left,
    }
}

/// Compose pet + comic bubble at physical pixels (1:1 layered present).
pub fn compose_yawn_frame(
    pet_rgba: &[u8],
    pet_sw: u32,
    pet_sh: u32,
    place: YawnPlacement,
    pet_phys: u32,
    bubble_alpha: f32,
    dpr: f64,
) -> (u32, u32, Vec<u8>) {
    let w = place.window.width.max(1) as u32;
    let h = place.window.height.max(1) as u32;
    let mut out = vec![0u8; (w * h * 4) as usize];

    // Caller must pass a pet buffer already presented at pet_phys×pet_phys
    // (same letterbox as idle `scale_rgba_centered`). Scaling 256→phys here
    // without those margins made the sit pop larger for the whole yawn.
    let (pw, ph, scaled) = scale_premul_bilinear(pet_rgba, pet_sw, pet_sh, pet_phys, pet_phys);
    blit_rgba(
        &mut out,
        w,
        h,
        &scaled,
        pw,
        ph,
        place.pet_local.0.max(0) as u32,
        place.pet_local.1.max(0) as u32,
    );

    let a = bubble_alpha.clamp(0.0, 1.0);
    if a > 0.01 {
        draw_bubble(
            &mut out,
            w,
            h,
            place.bubble_local.0,
            place.bubble_local.1,
            place.bubble_w,
            place.bubble_h,
            place.bubble_on_left,
            a,
            dpr,
        );
    }
    (w, h, out)
}

fn scale_premul_bilinear(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> (u32, u32, Vec<u8>) {
    if sw == 0 || sh == 0 || dw == 0 || dh == 0 {
        return (1, 1, vec![0, 0, 0, 0]);
    }
    if sw == dw && sh == dh {
        return (dw, dh, src.to_vec());
    }
    let mut out = vec![0u8; (dw * dh * 4) as usize];
    let swi = sw as i32;
    let shi = sh as i32;
    for y in 0..dh {
        let fy = (y as f32 + 0.5) * sh as f32 / dh as f32 - 0.5;
        let y0 = fy.floor() as i32;
        let ty = fy - y0 as f32;
        for x in 0..dw {
            let fx = (x as f32 + 0.5) * sw as f32 / dw as f32 - 0.5;
            let x0 = fx.floor() as i32;
            let tx = fx - x0 as f32;
            let mut acc = [0.0f32; 4];
            for (oy, wy) in [(0, 1.0 - ty), (1, ty)] {
                for (ox, wx) in [(0, 1.0 - tx), (1, tx)] {
                    let sx = (x0 + ox).clamp(0, swi - 1) as u32;
                    let sy = (y0 + oy).clamp(0, shi - 1) as u32;
                    let si = ((sy * sw + sx) * 4) as usize;
                    let a = src[si + 3] as f32 / 255.0;
                    let wgt = wx * wy;
                    acc[0] += src[si] as f32 * a * wgt;
                    acc[1] += src[si + 1] as f32 * a * wgt;
                    acc[2] += src[si + 2] as f32 * a * wgt;
                    acc[3] += src[si + 3] as f32 * wgt;
                }
            }
            let di = ((y * dw + x) * 4) as usize;
            let ao = acc[3].clamp(0.0, 255.0);
            if ao > 0.5 {
                let inv = 255.0 / ao;
                out[di] = (acc[0] * inv).round().clamp(0.0, 255.0) as u8;
                out[di + 1] = (acc[1] * inv).round().clamp(0.0, 255.0) as u8;
                out[di + 2] = (acc[2] * inv).round().clamp(0.0, 255.0) as u8;
                out[di + 3] = ao.round() as u8;
            }
        }
    }
    (dw, dh, out)
}

fn draw_bubble(
    px: &mut [u8],
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    bw: i32,
    bh: i32,
    on_left: bool,
    alpha: f32,
    dpr: f64,
) {
    let cream = with_alpha(CREAM, alpha);
    let ink = with_alpha(INK, alpha);
    fill_rrect(px, w, h, x, y, x + bw, y + bh, (bh / 2).max(10), cream);
    stroke_rrect(px, w, h, x, y, x + bw, y + bh, (bh / 2).max(10), ink, 2);

    // Tail toward the mouth (pet side of the bubble).
    let ty = y + bh * 2 / 3;
    if on_left {
        fill_triangle(px, w, h, x + bw - 2, ty - 7, x + bw - 2, ty + 7, x + bw + 10, ty + 2, cream);
        stroke_line(px, w, h, x + bw - 2, ty - 7, x + bw + 10, ty + 2, ink);
        stroke_line(px, w, h, x + bw - 2, ty + 7, x + bw + 10, ty + 2, ink);
    } else {
        fill_triangle(px, w, h, x + 2, ty - 7, x + 2, ty + 7, x - 10, ty + 2, cream);
        stroke_line(px, w, h, x + 2, ty - 7, x - 10, ty + 2, ink);
        stroke_line(px, w, h, x + 2, ty + 7, x - 10, ty + 2, ink);
    }

    let text_px = (15.0 * snap_dpr(dpr) as f32).clamp(13.0, 22.0);
    if let Some((tw, th, trgba)) = rasterize_text(YAWN_TEXT, (bw as u32).saturating_sub(16), text_px, with_alpha(TEXT, alpha))
    {
        let tx = (x + (bw - tw as i32) / 2).max(x + 6) as u32;
        let ty = (y + (bh - th as i32) / 2).max(y + 4) as u32;
        blit_rgba(px, w, h, &trgba, tw, th, tx, ty);
    }
}

fn with_alpha(c: [u8; 4], a: f32) -> [u8; 4] {
    let mut o = c;
    o[3] = ((c[3] as f32) * a.clamp(0.0, 1.0)).round() as u8;
    o
}

fn put(px: &mut [u8], w: u32, x: i32, y: i32, c: [u8; 4]) {
    if x < 0 || y < 0 || c[3] == 0 {
        return;
    }
    let h = (px.len() / 4) as u32 / w.max(1);
    if x >= w as i32 || y >= h as i32 {
        return;
    }
    let i = ((y as u32 * w + x as u32) * 4) as usize;
    let sa = c[3] as f32 / 255.0;
    let da = px[i + 3] as f32 / 255.0;
    let oa = sa + da * (1.0 - sa);
    if oa <= 0.0 {
        return;
    }
    for k in 0..3 {
        let s = c[k] as f32 / 255.0;
        let d = px[i + k] as f32 / 255.0;
        px[i + k] = ((s * sa + d * da * (1.0 - sa)) / oa * 255.0) as u8;
    }
    px[i + 3] = (oa * 255.0) as u8;
}

fn fill_rrect(px: &mut [u8], w: u32, h: u32, x0: i32, y0: i32, x1: i32, y1: i32, r: i32, c: [u8; 4]) {
    let r = r.max(0);
    for y in y0..y1 {
        for x in x0..x1 {
            if inside_rrect(x, y, x0, y0, x1, y1, r) {
                put(px, w, x, y, c);
            }
        }
    }
    let _ = h;
}

fn stroke_rrect(
    px: &mut [u8],
    w: u32,
    h: u32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    r: i32,
    c: [u8; 4],
    thick: i32,
) {
    let r = r.max(0);
    for y in y0..y1 {
        for x in x0..x1 {
            if inside_rrect(x, y, x0, y0, x1, y1, r)
                && !inside_rrect(x, y, x0 + thick, y0 + thick, x1 - thick, y1 - thick, (r - thick).max(0))
            {
                put(px, w, x, y, c);
            }
        }
    }
    let _ = h;
}

fn inside_rrect(x: i32, y: i32, x0: i32, y0: i32, x1: i32, y1: i32, r: i32) -> bool {
    if x < x0 || y < y0 || x >= x1 || y >= y1 {
        return false;
    }
    let r = r.max(0);
    let cx = if x < x0 + r {
        x0 + r
    } else if x >= x1 - r {
        x1 - 1 - r
    } else {
        return true;
    };
    let cy = if y < y0 + r {
        y0 + r
    } else if y >= y1 - r {
        y1 - 1 - r
    } else {
        return true;
    };
    let dx = x - cx;
    let dy = y - cy;
    dx * dx + dy * dy <= r * r
}

fn fill_triangle(
    px: &mut [u8],
    w: u32,
    h: u32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    x2: i32,
    y2: i32,
    c: [u8; 4],
) {
    let minx = x0.min(x1).min(x2);
    let maxx = x0.max(x1).max(x2);
    let miny = y0.min(y1).min(y2);
    let maxy = y0.max(y1).max(y2);
    for y in miny..=maxy {
        for x in minx..=maxx {
            if point_in_tri(x, y, x0, y0, x1, y1, x2, y2) {
                put(px, w, x, y, c);
            }
        }
    }
    let _ = h;
}

fn point_in_tri(px: i32, py: i32, x0: i32, y0: i32, x1: i32, y1: i32, x2: i32, y2: i32) -> bool {
    let d1 = (px - x1) * (y0 - y1) - (x0 - x1) * (py - y1);
    let d2 = (px - x2) * (y1 - y2) - (x1 - x2) * (py - y2);
    let d3 = (px - x0) * (y2 - y0) - (x2 - x0) * (py - y0);
    let has_neg = d1 < 0 || d2 < 0 || d3 < 0;
    let has_pos = d1 > 0 || d2 > 0 || d3 > 0;
    !(has_neg && has_pos)
}

fn stroke_line(px: &mut [u8], w: u32, h: u32, x0: i32, y0: i32, x1: i32, y1: i32, c: [u8; 4]) {
    let mut x = x0;
    let mut y = y0;
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        put(px, w, x, y, c);
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
    let _ = h;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_right_when_room() {
        let pet = Rect {
            x: 400,
            y: 400,
            width: 128,
            height: 128,
        };
        let work = Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let p = place_yawn_bubble(pet, work, 1.0);
        assert!(!p.bubble_on_left);
        assert_eq!(p.window.x, pet.x);
        assert_eq!(p.pet_local, (0, 0));
    }

    #[test]
    fn flips_left_near_right_edge() {
        let pet = Rect {
            x: 1784,
            y: 400,
            width: 128,
            height: 128,
        };
        let work = Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let p = place_yawn_bubble(pet, work, 1.0);
        assert!(p.bubble_on_left);
        assert!(p.window.x < pet.x);
    }
}
