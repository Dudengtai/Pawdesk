//! Pin-pet launcher placement: Flip → Shift → Union (task §14 L0).
//!
//! All geometry is in **physical screen pixels**. Convert logical sizes with
//! [`logical_to_physical`] / [`snap_dpr`] before calling [`place_launcher`].

use crate::platform::Rect;
use crate::ui::radial_menu::ExpandDir;

/// Extra padding around union(pet, card) when forming the overlay window.
pub const WINDOW_PADDING: i32 = 4;

/// Default gap between pet rect and card rect.
pub const DEFAULT_GAP: i32 = 8;

/// Default inset from work-area edges.
pub const DEFAULT_MARGIN: i32 = 8;

/// Result of pinning the pet and placing the glass card (physical pixels).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LauncherPlacement {
    /// Overlay window in screen coordinates.
    pub window: Rect,
    /// Pet rectangle after placement (screen).
    pub pet_screen: Rect,
    /// Card rectangle after placement (screen).
    pub card_screen: Rect,
    /// Pet origin relative to `window` top-left.
    pub pet_local: Rect,
    /// Card origin relative to `window` top-left.
    pub card_local: Rect,
    /// Final primary expand direction (horizontal flip result, or vertical fallback).
    pub dir: ExpandDir,
    /// How far the pet moved from the input rect (union clamp). Ideal `(0, 0)`.
    pub pet_screen_delta: (i32, i32),
}

/// Snap DPR like `app.rs` / menu compose (near-integer → round).
pub fn snap_dpr(dpr: f64) -> f64 {
    let d = dpr.clamp(1.0, 3.0);
    if (d - d.round()).abs() < 0.08 {
        d.round()
    } else {
        d
    }
}

/// Logical px → physical px using snapped DPR.
pub fn logical_to_physical(v: u32, dpr: f64) -> i32 {
    let d = snap_dpr(dpr);
    ((v as f64) * d).round().max(1.0) as i32
}

/// Physical px → logical px (for layout/compose).
pub fn physical_to_logical(v: i32, dpr: f64) -> f32 {
    let d = snap_dpr(dpr).max(0.01);
    v as f32 / d as f32
}

pub fn physical_to_logical_u32(v: i32, dpr: f64) -> u32 {
    physical_to_logical(v, dpr).round().max(1.0) as u32
}

/// Place card relative to pet: Flip (horizontal) → Shift (card only) → Union + micro-shift.
pub fn place_launcher(
    pet: Rect,
    card_w: i32,
    card_h: i32,
    gap: i32,
    work: Rect,
    margin: i32,
) -> LauncherPlacement {
    let margin = margin.max(0);
    let gap = gap.max(0);
    let (card_w, card_h) = fit_card_to_work(card_w, card_h, work, margin);

    let pet0 = pet;
    let (mut card, dir) = flip_horizontal(pet, card_w, card_h, gap, work, margin);
    shift_card_into_work(&mut card, work, margin);

    // If horizontal still leaves huge vertical issues only, try vertical attach as better card seed.
    // Keep pet fixed; only reconsider card when horizontal placement still overflows heavily.
    if !rect_mostly_inside(&card, work, margin) {
        if let Some((alt_card, alt_dir)) = try_vertical_attach(pet, card_w, card_h, gap, work, margin)
        {
            let mut c = alt_card;
            shift_card_into_work(&mut c, work, margin);
            if overflow_area(&c, work, margin) < overflow_area(&card, work, margin) {
                card = c;
                // dir from vertical only if we adopt it
                let _ = alt_dir;
                // Prefer reporting vertical dir when used
                return finish(pet0, pet, card, alt_dir, work, margin);
            }
        }
    }

    finish(pet0, pet, card, dir, work, margin)
}

fn finish(
    pet0: Rect,
    mut pet: Rect,
    mut card: Rect,
    dir: ExpandDir,
    work: Rect,
    margin: i32,
) -> LauncherPlacement {
    // Ensure card inside work after any path.
    shift_card_into_work(&mut card, work, margin);

    // Final safety: card inside work (pet still pinned).
    shift_card_into_work(&mut card, work, margin);

    // Union content, then pad only where work has room — avoids moving pet for padding.
    let content = union_rects(pet, card);
    let window_probe = inflate_within_work(content, WINDOW_PADDING, work);

    // If content itself sticks out (pet was half off-screen), micro-shift everything.
    let (dx, dy) = shift_rect_into_work_delta(&window_probe, work, 0);
    if dx != 0 || dy != 0 {
        pet = offset_rect(pet, dx, dy);
        card = offset_rect(card, dx, dy);
        shift_card_into_work(&mut card, work, margin);
    }
    let content = union_rects(pet, card);
    let mut window = inflate_within_work(content, WINDOW_PADDING, work);
    let (dx2, dy2) = shift_rect_into_work_delta(&window, work, 0);
    if dx2 != 0 || dy2 != 0 {
        window = offset_rect(window, dx2, dy2);
        pet = offset_rect(pet, dx2, dy2);
        card = offset_rect(card, dx2, dy2);
    }

    let pet_screen_delta = (pet.x - pet0.x, pet.y - pet0.y);
    let pet_local = Rect {
        x: pet.x - window.x,
        y: pet.y - window.y,
        width: pet.width,
        height: pet.height,
    };
    let card_local = Rect {
        x: card.x - window.x,
        y: card.y - window.y,
        width: card.width,
        height: card.height,
    };

    LauncherPlacement {
        window,
        pet_screen: pet,
        card_screen: card,
        pet_local,
        card_local,
        dir,
        pet_screen_delta,
    }
}

fn fit_card_to_work(card_w: i32, card_h: i32, work: Rect, margin: i32) -> (i32, i32) {
    let max_w = (work.width - 2 * margin).max(1);
    let max_h = (work.height - 2 * margin).max(1);
    (card_w.clamp(1, max_w), card_h.clamp(1, max_h))
}

/// Horizontal flip: prefer Right if it fits (or leaks less), else Left.
fn flip_horizontal(
    pet: Rect,
    card_w: i32,
    card_h: i32,
    gap: i32,
    work: Rect,
    margin: i32,
) -> (Rect, ExpandDir) {
    let pet_cy = pet.y + pet.height / 2;
    let y0 = pet_cy - card_h / 2;

    let right = Rect {
        x: pet.x + pet.width + gap,
        y: y0,
        width: card_w,
        height: card_h,
    };
    let left = Rect {
        x: pet.x - gap - card_w,
        y: y0,
        width: card_w,
        height: card_h,
    };

    let o_right = overflow_area(&right, work, margin);
    let o_left = overflow_area(&left, work, margin);

    // Prefer right when equal (common case: pet left-of-center).
    if o_right <= o_left {
        (right, ExpandDir::Right)
    } else {
        (left, ExpandDir::Left)
    }
}

fn try_vertical_attach(
    pet: Rect,
    card_w: i32,
    card_h: i32,
    gap: i32,
    work: Rect,
    margin: i32,
) -> Option<(Rect, ExpandDir)> {
    let pet_cx = pet.x + pet.width / 2;
    let x0 = pet_cx - card_w / 2;

    let down = Rect {
        x: x0,
        y: pet.y + pet.height + gap,
        width: card_w,
        height: card_h,
    };
    let up = Rect {
        x: x0,
        y: pet.y - gap - card_h,
        width: card_w,
        height: card_h,
    };

    let o_down = overflow_area(&down, work, margin);
    let o_up = overflow_area(&up, work, margin);
    let o_best_h = {
        let (h, _) = flip_horizontal(pet, card_w, card_h, gap, work, margin);
        overflow_area(&h, work, margin)
    };

    // Only suggest vertical if clearly better than horizontal seed.
    if o_down <= o_up && o_down < o_best_h {
        Some((down, ExpandDir::Down))
    } else if o_up < o_best_h {
        Some((up, ExpandDir::Up))
    } else {
        None
    }
}

/// Shift card only so it lies inside work (margin). Does not move pet.
fn shift_card_into_work(card: &mut Rect, work: Rect, margin: i32) {
    let (dx, dy) = shift_rect_into_work_delta(card, work, margin);
    card.x += dx;
    card.y += dy;
}

fn shift_rect_into_work_delta(r: &Rect, work: Rect, margin: i32) -> (i32, i32) {
    let min_x = work.x + margin;
    let min_y = work.y + margin;
    let max_x = work.x + work.width - margin - r.width;
    let max_y = work.y + work.height - margin - r.height;

    let nx = if max_x < min_x {
        // Card wider than work-2margin: pin to left margin.
        min_x
    } else {
        r.x.clamp(min_x, max_x)
    };
    let ny = if max_y < min_y {
        min_y
    } else {
        r.y.clamp(min_y, max_y)
    };
    (nx - r.x, ny - r.y)
}

fn union_rects(a: Rect, b: Rect) -> Rect {
    let x1 = a.x.min(b.x);
    let y1 = a.y.min(b.y);
    let x2 = (a.x + a.width).max(b.x + b.width);
    let y2 = (a.y + a.height).max(b.y + b.height);
    Rect {
        x: x1,
        y: y1,
        width: x2 - x1,
        height: y2 - y1,
    }
}

/// Grow `r` by up to `pad` on each side without leaving `work`.
fn inflate_within_work(r: Rect, pad: i32, work: Rect) -> Rect {
    let pad = pad.max(0);
    let x1 = (r.x - pad).max(work.x);
    let y1 = (r.y - pad).max(work.y);
    let x2 = (r.x + r.width + pad).min(work.x + work.width);
    let y2 = (r.y + r.height + pad).min(work.y + work.height);
    Rect {
        x: x1,
        y: y1,
        width: (x2 - x1).max(r.width),
        height: (y2 - y1).max(r.height),
    }
}

fn offset_rect(r: Rect, dx: i32, dy: i32) -> Rect {
    Rect {
        x: r.x + dx,
        y: r.y + dy,
        width: r.width,
        height: r.height,
    }
}

/// Approximate overflow area outside work (margin-inset), 0 if fully inside.
fn overflow_area(r: &Rect, work: Rect, margin: i32) -> i64 {
    let left = work.x + margin;
    let top = work.y + margin;
    let right = work.x + work.width - margin;
    let bottom = work.y + work.height - margin;

    let ox1 = (left - r.x).max(0) as i64;
    let oy1 = (top - r.y).max(0) as i64;
    let ox2 = (r.x + r.width - right).max(0) as i64;
    let oy2 = (r.y + r.height - bottom).max(0) as i64;

    // Weighted edge overflow (not true area, but ranks candidates stably).
    ox1 + oy1 + ox2 + oy2
        + ox1 * r.height as i64 / 100
        + ox2 * r.height as i64 / 100
        + oy1 * r.width as i64 / 100
        + oy2 * r.width as i64 / 100
}

fn rect_mostly_inside(r: &Rect, work: Rect, margin: i32) -> bool {
    overflow_area(r, work, margin) == 0
}

/// True if `inner` is fully inside `outer` shrunk by `margin` (or outer itself if margin=0).
pub fn fully_inside(inner: &Rect, outer: Rect, margin: i32) -> bool {
    let m = margin.max(0);
    let left = outer.x + m;
    let top = outer.y + m;
    let right = outer.x + outer.width - m;
    let bottom = outer.y + outer.height - m;
    inner.x >= left
        && inner.y >= top
        && inner.x + inner.width <= right
        && inner.y + inner.height <= bottom
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work_1920() -> Rect {
        Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040, // taskbar-ish
        }
    }

    fn pet_at(x: i32, y: i32, s: i32) -> Rect {
        Rect {
            x,
            y,
            width: s,
            height: s,
        }
    }

    fn place(pet: Rect) -> LauncherPlacement {
        place_launcher(pet, 420, 300, DEFAULT_GAP, work_1920(), DEFAULT_MARGIN)
    }

    #[test]
    fn center_leftish_opens_right_zero_delta() {
        let pet = pet_at(200, 400, 128);
        let p = place(pet);
        assert_eq!(p.dir, ExpandDir::Right);
        assert_eq!(p.pet_screen_delta, (0, 0));
        assert!(fully_inside(&p.card_screen, work_1920(), DEFAULT_MARGIN));
        assert!(fully_inside(&p.window, work_1920(), 0));
        assert_eq!(p.pet_screen.x, pet.x);
        assert_eq!(p.pet_screen.y, pet.y);
        // Card to the right of pet
        assert!(p.card_screen.x >= pet.x + pet.width);
    }

    #[test]
    fn pin_right_flips_left() {
        let pet = pet_at(1920 - 128 - 10, 400, 128);
        let p = place(pet);
        assert_eq!(p.dir, ExpandDir::Left);
        assert!(fully_inside(&p.card_screen, work_1920(), DEFAULT_MARGIN));
        assert!(p.card_screen.x + p.card_screen.width <= pet.x + 20 || p.dir == ExpandDir::Left);
        // Pet should stay pinned when there is room after flip
        assert_eq!(p.pet_screen_delta, (0, 0));
        assert!(p.card_screen.x + p.card_screen.width <= p.pet_screen.x + DEFAULT_GAP + 1
            || p.card_screen.x < p.pet_screen.x);
    }

    #[test]
    fn pin_left_opens_right() {
        let pet = pet_at(12, 400, 128);
        let p = place(pet);
        assert_eq!(p.dir, ExpandDir::Right);
        assert_eq!(p.pet_screen_delta, (0, 0));
        assert!(fully_inside(&p.card_screen, work_1920(), DEFAULT_MARGIN));
        assert!(p.card_screen.x >= p.pet_screen.x + p.pet_screen.width);
    }

    #[test]
    fn pin_bottom_shifts_card_up() {
        let pet = pet_at(800, 1040 - 128 - 4, 128);
        let p = place(pet);
        assert!(fully_inside(&p.card_screen, work_1920(), DEFAULT_MARGIN));
        assert!(fully_inside(&p.window, work_1920(), 0));
        // Card bottom not below work
        assert!(p.card_screen.y + p.card_screen.height <= 1040 - DEFAULT_MARGIN);
        // Pet ideally unmoved
        assert_eq!(p.pet_screen_delta, (0, 0));
    }

    #[test]
    fn pin_top_shifts_card_down() {
        let pet = pet_at(800, 4, 128);
        let p = place(pet);
        assert!(fully_inside(&p.card_screen, work_1920(), DEFAULT_MARGIN));
        assert!(p.card_screen.y >= DEFAULT_MARGIN);
        assert_eq!(p.pet_screen_delta, (0, 0));
    }

    #[test]
    fn bottom_right_corner_card_and_window_inside() {
        let pet = pet_at(1920 - 128 - 4, 1040 - 128 - 4, 128);
        let p = place(pet);
        assert!(fully_inside(&p.card_screen, work_1920(), DEFAULT_MARGIN));
        assert!(fully_inside(&p.window, work_1920(), 0));
        // Delta allowed but bounded
        let (dx, dy) = p.pet_screen_delta;
        assert!(dx.abs() < 500 && dy.abs() < 500);
    }

    #[test]
    fn top_left_corner_inside() {
        let pet = pet_at(4, 4, 128);
        let p = place(pet);
        assert!(fully_inside(&p.card_screen, work_1920(), DEFAULT_MARGIN));
        assert!(fully_inside(&p.window, work_1920(), 0));
        assert_eq!(p.dir, ExpandDir::Right);
    }

    #[test]
    fn narrow_work_no_panic_and_clamped() {
        let work = Rect {
            x: 0,
            y: 0,
            width: 500,
            height: 400,
        };
        let pet = pet_at(20, 20, 128);
        let p = place_launcher(pet, 420, 300, DEFAULT_GAP, work, DEFAULT_MARGIN);
        assert!(p.card_screen.width <= work.width - 2 * DEFAULT_MARGIN);
        assert!(p.card_screen.height <= work.height - 2 * DEFAULT_MARGIN);
        assert!(fully_inside(&p.card_screen, work, DEFAULT_MARGIN));
        assert!(fully_inside(&p.window, work, 0));
    }

    #[test]
    fn dpr_1_5_physical_sizes_fit() {
        let dpr = 1.5;
        let pet_s = logical_to_physical(128, dpr);
        let card_w = logical_to_physical(420, dpr);
        let card_h = logical_to_physical(300, dpr);
        assert_eq!(pet_s, 192);
        assert_eq!(card_w, 630);
        assert_eq!(card_h, 450);

        let work = Rect {
            x: 0,
            y: 0,
            width: 2880,
            height: 1560,
        };
        let pet = pet_at(100, 500, pet_s);
        let p = place_launcher(pet, card_w, card_h, DEFAULT_GAP, work, DEFAULT_MARGIN);
        assert!(fully_inside(&p.card_screen, work, DEFAULT_MARGIN));
        assert!(fully_inside(&p.window, work, 0));
        assert_eq!(p.pet_local.width, pet_s);
        assert_eq!(p.card_local.width, card_w);
    }

    #[test]
    fn dpr_snap_near_integer() {
        assert_eq!(snap_dpr(1.0), 1.0);
        assert_eq!(snap_dpr(1.25), 1.25);
        assert_eq!(snap_dpr(1.5), 1.5);
        assert_eq!(snap_dpr(1.98), 2.0);
        assert_eq!(logical_to_physical(100, 1.0), 100);
        assert_eq!(logical_to_physical(100, 2.0), 200);
    }

    #[test]
    fn local_coords_match_screen() {
        let pet = pet_at(300, 300, 128);
        let p = place(pet);
        assert_eq!(p.pet_local.x + p.window.x, p.pet_screen.x);
        assert_eq!(p.pet_local.y + p.window.y, p.pet_screen.y);
        assert_eq!(p.card_local.x + p.window.x, p.card_screen.x);
        assert_eq!(p.card_local.y + p.window.y, p.card_screen.y);
        // Locals non-negative within window
        assert!(p.pet_local.x >= 0 && p.pet_local.y >= 0);
        assert!(p.card_local.x >= 0 && p.card_local.y >= 0);
        assert!(p.pet_local.x + p.pet_local.width <= p.window.width);
        assert!(p.card_local.x + p.card_local.width <= p.window.width);
    }

    #[test]
    fn fully_inside_helper() {
        let outer = work_1920();
        let inner = Rect {
            x: 10,
            y: 10,
            width: 100,
            height: 100,
        };
        assert!(fully_inside(&inner, outer, 8));
        let bad = Rect {
            x: -5,
            y: 10,
            width: 100,
            height: 100,
        };
        assert!(!fully_inside(&bad, outer, 8));
    }
}
