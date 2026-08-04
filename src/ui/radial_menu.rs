//! Apple-inspired quick-launch dock layout & hit-test.

use crate::event::Point;
use crate::platform::Rect;
use crate::shortcut::ShortcutItem;
use uuid::Uuid;

/// Launcher card (logical @ 96 DPI) — wider for breathing room.
pub const MENU_WINDOW_W: u32 = 480;
pub const MENU_WINDOW_H: u32 = 320;
pub const MENU_WINDOW: u32 = MENU_WINDOW_W;

pub const MAX_SHORTCUTS_VISIBLE: usize = 5;
pub const ITEM_DIAMETER: f32 = 44.0;
pub const RING_RADIUS: f32 = 0.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuEntry {
    Manage,
    AddShortcut,
    PauseReminder,
    Shortcut {
        id: Uuid,
        name: String,
        valid: bool,
    },
}

#[derive(Debug, Clone)]
pub struct ItemGeom {
    pub entry: MenuEntry,
    pub cx: f32,
    pub cy: f32,
    pub radius: f32,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone)]
pub struct RadialLayout {
    pub items: Vec<ItemGeom>,
    pub open_t: f32,
    pub window: u32,
    pub window_w: u32,
    pub window_h: u32,
    pub pet_x: f32,
    pub pet_y: f32,
    pub pet_w: f32,
    pub pet_h: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpandDir {
    Right,
    Left,
    Down,
    Up,
}

pub fn prefer_direction(pet_center_screen: Point, work: Rect) -> ExpandDir {
    let left = pet_center_screen.x - work.x as f64;
    let right = (work.x + work.width) as f64 - pet_center_screen.x;
    let top = pet_center_screen.y - work.y as f64;
    let bottom = (work.y + work.height) as f64 - pet_center_screen.y;
    let mut best = ExpandDir::Right;
    let mut best_s = right;
    if left > best_s {
        best = ExpandDir::Left;
        best_s = left;
    }
    if bottom > best_s {
        best = ExpandDir::Down;
        best_s = bottom;
    }
    if top > best_s {
        best = ExpandDir::Up;
    }
    let _ = best_s;
    best
}

pub fn build_entries(shortcuts: &[ShortcutItem], reminder_paused: bool) -> Vec<MenuEntry> {
    let mut entries = vec![
        MenuEntry::AddShortcut,
        MenuEntry::Manage,
        MenuEntry::PauseReminder,
    ];
    let _ = reminder_paused;
    for s in shortcuts
        .iter()
        .filter(|s| s.enabled)
        .take(MAX_SHORTCUTS_VISIBLE)
    {
        entries.push(MenuEntry::Shortcut {
            id: s.id,
            name: s.name.clone(),
            valid: s.is_path_valid(),
        });
    }
    entries
}

/// Apple-like dock: avatar column + content column with clear vertical rhythm.
pub fn layout(entries: &[MenuEntry], dir: ExpandDir, open_t: f32) -> RadialLayout {
    let t = open_t.clamp(0.0, 1.0);
    let ww = MENU_WINDOW_W as f32;
    let wh = MENU_WINDOW_H as f32;

    // Avatar column — slightly taller plate, more air
    let pet_w = 128.0;
    let pet_h = 176.0;
    let margin = 22.0;
    let gap_col = 20.0;
    let (pet_x, content_x, content_w) = match dir {
        ExpandDir::Left => {
            let px = ww - pet_w - margin;
            (px, margin, px - margin - gap_col)
        }
        _ => (
            margin,
            margin + pet_w + gap_col,
            ww - margin - pet_w - gap_col - margin,
        ),
    };
    let pet_y = (wh - pet_h) * 0.5;
    let slide = (1.0 - t) * 12.0;

    // Content below nav header (title y16 + subtitle y38 + breathing room)
    let mut y = 68.0 + slide;
    let mut items = Vec::new();

    // Primary: full-width system blue
    let primary_h = 44.0;
    if let Some(e) = entries.iter().find(|e| matches!(e, MenuEntry::AddShortcut)) {
        items.push(rect_item(e.clone(), content_x, y, content_w, primary_h));
        y += primary_h + 10.0;
    }

    // Secondary: Manage | Pause — equal chips
    let secondary: Vec<&MenuEntry> = entries
        .iter()
        .filter(|e| matches!(e, MenuEntry::Manage | MenuEntry::PauseReminder))
        .collect();
    if !secondary.is_empty() {
        let gap = 8.0;
        let n = secondary.len() as f32;
        let chip_w = (content_w - gap * (n - 1.0)) / n;
        let chip_h = 34.0;
        for (i, e) in secondary.iter().enumerate() {
            let x = content_x + i as f32 * (chip_w + gap);
            items.push(rect_item((*e).clone(), x, y, chip_w, chip_h));
        }
        y += chip_h + 16.0;
    }

    // Shortcut list rows
    let row_h = 46.0;
    let row_gap = 6.0;
    for e in entries.iter().filter(|e| matches!(e, MenuEntry::Shortcut { .. })) {
        if y + row_h > wh - margin {
            break;
        }
        items.push(rect_item(e.clone(), content_x, y, content_w, row_h));
        y += row_h + row_gap;
    }

    RadialLayout {
        items,
        open_t: t,
        window: MENU_WINDOW_W,
        window_w: MENU_WINDOW_W,
        window_h: MENU_WINDOW_H,
        pet_x,
        pet_y,
        pet_w,
        pet_h,
    }
}

fn rect_item(entry: MenuEntry, x: f32, y: f32, w: f32, h: f32) -> ItemGeom {
    ItemGeom {
        entry,
        x,
        y,
        w,
        h,
        cx: x + w * 0.5,
        cy: y + h * 0.5,
        radius: h * 0.5,
    }
}

pub fn hit_test(layout: &RadialLayout, local_x: f32, local_y: f32) -> Option<&MenuEntry> {
    for item in layout.items.iter().rev() {
        if local_x >= item.x
            && local_x <= item.x + item.w
            && local_y >= item.y
            && local_y <= item.y + item.h
        {
            return Some(&item.entry);
        }
    }
    None
}

pub fn hit_center(layout: &RadialLayout, local_x: f32, local_y: f32) -> bool {
    local_x >= layout.pet_x
        && local_x <= layout.pet_x + layout.pet_w
        && local_y >= layout.pet_y
        && local_y <= layout.pet_y + layout.pet_h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefer_right_when_on_left() {
        let work = Rect {
            x: 0,
            y: 0,
            width: 1000,
            height: 800,
        };
        assert_eq!(
            prefer_direction(Point::new(50.0, 400.0), work),
            ExpandDir::Right
        );
    }

    #[test]
    fn prefer_left_when_on_right() {
        let work = Rect {
            x: 0,
            y: 0,
            width: 1000,
            height: 800,
        };
        assert_eq!(
            prefer_direction(Point::new(950.0, 400.0), work),
            ExpandDir::Left
        );
    }

    #[test]
    fn layout_expands_with_t() {
        let entries = vec![MenuEntry::Manage, MenuEntry::AddShortcut];
        let open = layout(&entries, ExpandDir::Right, 1.0);
        assert!(!open.items.is_empty());
    }

    #[test]
    fn hit_test_finds_item() {
        let entries = vec![MenuEntry::AddShortcut, MenuEntry::Manage];
        let lay = layout(&entries, ExpandDir::Right, 1.0);
        let item = &lay.items[0];
        assert!(hit_test(&lay, item.cx, item.cy).is_some());
    }
}
