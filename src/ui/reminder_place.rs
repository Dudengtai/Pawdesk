//! Reminder travel overlay: union(origin pet, center card, hop arc).
//!
//! All geometry is **physical screen pixels**. HWND origin/size are set once;
//! the pet slot walks inside the overlay.

use crate::platform::Rect;
use crate::ui::launcher_place::logical_to_physical;

/// Extra padding around the union when forming the overlay window.
pub const TRAVEL_PAD: i32 = 8;

/// Result of pinning origin + dest slots and the reminder card (physical px).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReminderPlacement {
    /// Overlay window in screen coordinates.
    pub window: Rect,
    /// Pet rectangle at the desk origin (screen).
    pub origin_pet: Rect,
    /// Pet rectangle centered in the card (screen).
    pub dest_pet: Rect,
    /// Card rectangle, work-area centered (screen).
    pub card_screen: Rect,
    /// Origin pet slot relative to `window`.
    pub origin_local: Rect,
    /// Dest pet slot relative to `window`.
    pub dest_local: Rect,
    /// Card origin relative to `window`.
    pub card_local: Rect,
}

/// Place the travel overlay: one HWND covering origin, hop arc, and center card.
pub fn place_reminder_travel(
    origin_pet: Rect,
    work: Rect,
    desk: Rect,
    dpr: f64,
    lift_px: i32,
    card_log_w: u32,
    card_log_h: u32,
) -> ReminderPlacement {
    let card_w = logical_to_physical(card_log_w, dpr).max(1);
    let card_h = logical_to_physical(card_log_h, dpr).max(1);
    let card_screen = Rect {
        x: work.x + (work.width - card_w).max(0) / 2,
        y: work.y + (work.height - card_h).max(0) / 2,
        width: card_w,
        height: card_h,
    };

    let pw = origin_pet.width.max(1);
    let ph = origin_pet.height.max(1);
    let dest_pet = Rect {
        x: card_screen.x + (card_w - pw) / 2,
        y: card_screen.y + (card_h - ph) / 2,
        width: pw,
        height: ph,
    };

    let lift = lift_px.max(0);
    let arc = hop_arc_bounds(origin_pet, dest_pet, lift);
    let union = union_rect(union_rect(origin_pet, card_screen), arc);
    let padded = inflate(union, TRAVEL_PAD);
    let window = clamp_into(padded, desk);

    ReminderPlacement {
        window,
        origin_pet,
        dest_pet,
        card_screen,
        origin_local: to_local(origin_pet, window),
        dest_local: to_local(dest_pet, window),
        card_local: to_local(card_screen, window),
    }
}

fn hop_arc_bounds(a: Rect, b: Rect, lift: i32) -> Rect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y) - lift;
    let x1 = (a.x + a.width).max(b.x + b.width);
    let y1 = (a.y + a.height).max(b.y + b.height);
    Rect {
        x: x0,
        y: y0,
        width: (x1 - x0).max(1),
        height: (y1 - y0).max(1),
    }
}

fn union_rect(a: Rect, b: Rect) -> Rect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = (a.x + a.width).max(b.x + b.width);
    let y1 = (a.y + a.height).max(b.y + b.height);
    Rect {
        x: x0,
        y: y0,
        width: (x1 - x0).max(1),
        height: (y1 - y0).max(1),
    }
}

fn inflate(r: Rect, pad: i32) -> Rect {
    Rect {
        x: r.x - pad,
        y: r.y - pad,
        width: r.width + pad * 2,
        height: r.height + pad * 2,
    }
}

fn clamp_into(mut r: Rect, desk: Rect) -> Rect {
    if desk.width <= 0 || desk.height <= 0 {
        return r;
    }
    r.width = r.width.min(desk.width).max(1);
    r.height = r.height.min(desk.height).max(1);
    if r.x < desk.x {
        r.x = desk.x;
    }
    if r.y < desk.y {
        r.y = desk.y;
    }
    if r.x + r.width > desk.x + desk.width {
        r.x = desk.x + desk.width - r.width;
    }
    if r.y + r.height > desk.y + desk.height {
        r.y = desk.y + desk.height - r.height;
    }
    r
}

fn to_local(r: Rect, window: Rect) -> Rect {
    Rect {
        x: r.x - window.x,
        y: r.y - window.y,
        width: r.width,
        height: r.height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work() -> Rect {
        Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        }
    }

    fn desk() -> Rect {
        Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }
    }

    fn pet_at(x: i32, y: i32) -> Rect {
        Rect {
            x,
            y,
            width: 77,
            height: 77,
        }
    }

    #[test]
    fn overlay_contains_origin_card_and_arc() {
        let origin = pet_at(1800, 900);
        let p = place_reminder_travel(origin, work(), desk(), 1.0, 48, 640, 360);
        assert!(p.window.width >= 640);
        assert!(p.window.height >= 360);
        assert!(p.window.contains(origin.x + 1, origin.y + 1));
        assert!(p.window.contains(p.card_screen.x + 8, p.card_screen.y + 8));
        assert!(p.window.contains(p.dest_pet.x + 1, p.dest_pet.y + 1));
        assert!(
            p.origin_local.y + 48 <= p.dest_local.y + p.dest_local.height
                || p.window.y <= origin.y - 40,
            "arc lift should expand the overlay upward"
        );
    }

    #[test]
    fn dest_pet_is_centered_in_card() {
        let p = place_reminder_travel(pet_at(100, 100), work(), desk(), 1.0, 32, 640, 360);
        let (cx, cy) = p.card_screen.center();
        let (px, py) = p.dest_pet.center();
        assert!((cx - px).abs() <= 1);
        assert!((cy - py).abs() <= 1);
        assert_eq!(p.dest_pet.width, 77);
    }

    #[test]
    fn near_center_still_covers_card() {
        let card_x = (1920 - 640) / 2;
        let card_y = (1040 - 360) / 2;
        let origin = pet_at(card_x + 280, card_y + 140);
        let p = place_reminder_travel(origin, work(), desk(), 1.0, 28, 640, 360);
        assert!(p.window.width >= 640);
        assert!(p.window.height >= 360);
        assert_eq!(p.card_screen.width, 640);
        assert_eq!(p.card_screen.height, 360);
    }

    #[test]
    fn hidpi_scales_card() {
        let p = place_reminder_travel(pet_at(40, 40), work(), desk(), 2.0, 64, 640, 360);
        assert_eq!(p.card_screen.width, 1280);
        assert_eq!(p.card_screen.height, 720);
        assert_eq!(p.origin_local.width, 77);
    }

    #[test]
    fn overlay_stays_on_desk() {
        let origin = pet_at(-20, 2000);
        let p = place_reminder_travel(origin, work(), desk(), 1.0, 80, 640, 360);
        assert!(p.window.x >= desk().x);
        assert!(p.window.y >= desk().y);
        assert!(p.window.x + p.window.width <= desk().x + desk().width);
        assert!(p.window.y + p.window.height <= desk().y + desk().height);
    }

    #[test]
    fn locals_match_screen() {
        let origin = pet_at(1600, 800);
        let p = place_reminder_travel(origin, work(), desk(), 1.0, 40, 640, 360);
        assert_eq!(p.origin_local.x + p.window.x, p.origin_pet.x);
        assert_eq!(p.dest_local.x + p.window.x, p.dest_pet.x);
        assert_eq!(p.card_local.x + p.window.x, p.card_screen.x);
    }
}
