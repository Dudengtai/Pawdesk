//! Long-press reorder + feed-bowl delete for the dock app list (PRD F-SC-05).

use std::time::{Duration, Instant};

use uuid::Uuid;

/// Hold still this long before a press becomes a drag.
pub const LONG_PRESS: Duration = Duration::from_millis(400);
/// Movement (logical px) that cancels a pending long-press.
pub const SLOP_PX: f32 = 8.0;
/// Auto-scroll when the pointer is this close to the list viewport edge.
pub const EDGE_SCROLL_PX: f32 = 12.0;
pub const BOWL_SIZE: f32 = 48.0;
pub const BOWL_GAP: f32 = 6.0;
pub const BOWL_HIT_PAD: f32 = 8.0;

#[derive(Debug, Clone)]
pub enum ListDrag {
    Idle,
    /// Finger down on a shortcut row; may become a click or a drag.
    Pressing {
        id: Uuid,
        /// `layout.items` index at press time (sentinel so the pet window is not dragged).
        item_idx: usize,
        /// Index among enabled+sorted shortcuts.
        from: usize,
        origin: (f32, f32),
        /// False once the pointer walks past [`SLOP_PX`] before the long-press fires.
        armed: bool,
        t0: Instant,
    },
    Dragging {
        id: Uuid,
        from: usize,
        grab_dx: f32,
        grab_dy: f32,
        pointer: (f32, f32),
        /// Destination among remaining items (0..=n-1 for n original items).
        insert_at: usize,
        over_bowl: bool,
        row_w: f32,
        row_h: f32,
    },
}

impl Default for ListDrag {
    fn default() -> Self {
        Self::Idle
    }
}

impl ListDrag {
    pub fn is_dragging(&self) -> bool {
        matches!(self, Self::Dragging { .. })
    }

    pub fn is_active(&self) -> bool {
        !matches!(self, Self::Idle)
    }
}

pub fn pointer_dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    let dx = a.0 - b.0;
    let dy = a.1 - b.1;
    (dx * dx + dy * dy).sqrt()
}

pub fn should_start_drag(held: Duration, dist: f32) -> bool {
    held >= LONG_PRESS && dist < SLOP_PX
}

/// Insert index in the list *after* the dragged item is removed.
///
/// `max_insert` is `remaining.len()` (= original count − 1). Returns `0..=max_insert`.
pub fn insert_index_from_y(
    y: f32,
    list_top: f32,
    stride: f32,
    scroll: usize,
    max_insert: usize,
) -> usize {
    if max_insert == 0 {
        return 0;
    }
    let relative = y - list_top;
    if relative <= 0.0 {
        return scroll.min(max_insert);
    }
    let stride = stride.max(1.0);
    let raw = (relative / stride + 0.5).floor() as i32;
    (scroll as i32 + raw).clamp(0, max_insert as i32) as usize
}

/// Empty-bowl slot centered under the pet plate, clamped into the overlay.
pub fn bowl_rect(
    pet_x: f32,
    pet_y: f32,
    pet_w: f32,
    pet_h: f32,
    win_w: f32,
    win_h: f32,
) -> (f32, f32, f32, f32) {
    let w = BOWL_SIZE;
    let h = BOWL_SIZE;
    let mut x = pet_x + (pet_w - w) * 0.5;
    let mut y = pet_y + pet_h + BOWL_GAP;
    if y + h > win_h - 4.0 {
        // Prefer under the plate; if the overlay is short, overlap the lower plate.
        y = (win_h - h - 4.0).max(pet_y + pet_h * 0.45);
    }
    x = x.clamp(2.0, (win_w - w - 2.0).max(2.0));
    y = y.clamp(2.0, (win_h - h - 2.0).max(2.0));
    (x, y, w, h)
}

pub fn hit_bowl(px: f32, py: f32, bowl: (f32, f32, f32, f32)) -> bool {
    let pad = BOWL_HIT_PAD;
    px >= bowl.0 - pad
        && py >= bowl.1 - pad
        && px <= bowl.0 + bowl.2 + pad
        && py <= bowl.1 + bowl.3 + pad
}

/// Move `id` so it lands at `insert_at` among the items that remain after removal.
pub fn reorder_ids(ids: &[Uuid], id: Uuid, insert_at: usize) -> Vec<Uuid> {
    let mut rest: Vec<Uuid> = ids.iter().copied().filter(|&x| x != id).collect();
    let at = insert_at.min(rest.len());
    rest.insert(at, id);
    rest
}

/// +1 = scroll toward earlier rows, −1 = later. 0 = stay.
pub fn edge_scroll_delta(y: f32, list_top: f32, list_bottom: f32) -> i32 {
    if y < list_top + EDGE_SCROLL_PX {
        1
    } else if y > list_bottom - EDGE_SCROLL_PX {
        -1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_press_needs_time_and_stillness() {
        assert!(!should_start_drag(Duration::from_millis(200), 0.0));
        assert!(should_start_drag(Duration::from_millis(400), 0.0));
        assert!(should_start_drag(Duration::from_millis(500), 7.0));
        assert!(!should_start_drag(Duration::from_millis(500), 8.0));
    }

    #[test]
    fn insert_index_clamps_and_uses_midpoints() {
        // 4 remaining slots → insert 0..=4
        let top = 100.0;
        let stride = 46.0;
        assert_eq!(insert_index_from_y(80.0, top, stride, 0, 4), 0);
        assert_eq!(insert_index_from_y(100.0, top, stride, 0, 4), 0);
        // past midpoint of first slot
        assert_eq!(insert_index_from_y(123.1, top, stride, 0, 4), 1);
        assert_eq!(insert_index_from_y(169.0, top, stride, 0, 4), 2);
        assert_eq!(insert_index_from_y(800.0, top, stride, 0, 4), 4);
        assert_eq!(insert_index_from_y(100.0, top, stride, 2, 4), 2);
        assert_eq!(insert_index_from_y(0.0, top, stride, 0, 0), 0);
    }

    #[test]
    fn bowl_sits_below_pet_and_stays_in_window() {
        let (x, y, w, h) = bowl_rect(10.0, 20.0, 100.0, 80.0, 400.0, 400.0);
        assert!((w - BOWL_SIZE).abs() < 0.1);
        assert!(y >= 20.0 + 80.0);
        assert!(x + w <= 400.0);
        assert!(y + h <= 400.0);
        // Tight window: clamp, do not hang off the bottom.
        let (_x2, y2, _w2, h2) = bowl_rect(10.0, 300.0, 80.0, 80.0, 200.0, 360.0);
        assert!(y2 + h2 <= 360.0);
    }

    #[test]
    fn bowl_hit_includes_padding() {
        let b = (100.0, 100.0, 72.0, 72.0);
        assert!(hit_bowl(136.0, 136.0, b));
        assert!(hit_bowl(96.0, 96.0, b));
        assert!(!hit_bowl(80.0, 80.0, b));
    }

    #[test]
    fn reorder_moves_id_among_remaining() {
        let a = Uuid::from_u128(1);
        let b = Uuid::from_u128(2);
        let c = Uuid::from_u128(3);
        let d = Uuid::from_u128(4);
        let ids = vec![a, b, c, d];
        assert_eq!(reorder_ids(&ids, b, 0), vec![b, a, c, d]);
        assert_eq!(reorder_ids(&ids, b, 1), vec![a, b, c, d]);
        assert_eq!(reorder_ids(&ids, b, 2), vec![a, c, b, d]);
        assert_eq!(reorder_ids(&ids, b, 3), vec![a, c, d, b]);
        assert_eq!(reorder_ids(&ids, b, 99), vec![a, c, d, b]);
    }

    #[test]
    fn edge_scroll_signs() {
        assert_eq!(edge_scroll_delta(100.0, 100.0, 300.0), 1);
        assert_eq!(edge_scroll_delta(200.0, 100.0, 300.0), 0);
        assert_eq!(edge_scroll_delta(295.0, 100.0, 300.0), -1);
    }
}
