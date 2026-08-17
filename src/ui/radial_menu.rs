//! Quick-launch dock layout & hit-test (pin-pet glass card).
//!
//! Shortcut list is **scrollable**: a fixed viewport shows [`LIST_VISIBLE_ROWS`]
//! rows; mouse wheel moves [`list_scroll`]. No hard product limit of 5 apps —
//! only a large soft cap for safety.

use crate::event::Point;
use crate::platform::Rect;
use crate::shortcut::{IconRgba, ShortcutItem};
use std::sync::Arc;
use uuid::Uuid;

/// Legacy full-window size (pre pin-pet). Prefer [`CARD_LOGICAL_W`] + placement.
pub const MENU_WINDOW_W: u32 = 480;
pub const MENU_WINDOW_H: u32 = 320;
pub const MENU_WINDOW: u32 = MENU_WINDOW_W;

/// Glass card content size (logical @ 96 DPI) — pet is outside this rect (pin-pet).
pub const CARD_LOGICAL_W: u32 = 360;
/// Height = chrome + frequent-icon strip + viewport for [`LIST_VISIBLE_ROWS`] + hint.
///
/// Strip (label + box) + list caption are always reserved so `place_launcher` stays stable.
pub const CARD_LOGICAL_H: u32 = 450;

/// Rows visible in the dock list without scrolling.
pub const LIST_VISIBLE_ROWS: usize = 5;
/// Soft safety cap only (not a product UX limit). Scroll covers the rest.
pub const MAX_SHORTCUTS: usize = 128;
/// @deprecated name — kept as alias for call sites / docs.
pub const MAX_SHORTCUTS_VISIBLE: usize = LIST_VISIBLE_ROWS;

pub const ITEM_DIAMETER: f32 = 44.0;
pub const RING_RADIUS: f32 = 0.0;

/// Vertical stack metrics (logical px).
pub const TITLE_BAND: f32 = 56.0;
pub const CARD_MARGIN: f32 = 12.0;
pub const PRIMARY_H: f32 = 40.0;
pub const PRIMARY_GAP: f32 = 8.0;
/// Settings paw-mark hit target in the title band (logical px).
pub const GEAR_SIZE: f32 = 32.0;
pub const ROW_H: f32 = 42.0;
pub const ROW_GAP: f32 = 4.0;
/// Space under list for “滚轮查看更多” hint.
pub const SCROLL_HINT_H: f32 = 16.0;

/// Frequent-launch icon strip (between「再叼一个」and the list).
pub const RECENT_MAX: usize = 6;
pub const RECENT_LABEL_H: f32 = 14.0;
pub const RECENT_LABEL_GAP: f32 = 4.0;
pub const RECENT_BOX_PAD: f32 = 6.0;
pub const RECENT_BOX_H: f32 = 48.0;
pub const RECENT_STRIP_H: f32 = RECENT_LABEL_H + RECENT_LABEL_GAP + RECENT_BOX_H;
pub const RECENT_ICON: f32 = 30.0;
pub const RECENT_SLOT: f32 = 36.0;
pub const RECENT_SLOT_GAP: f32 = 8.0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuEntry {
    Manage,
    AddShortcut,
    /// Icon-only frequent-launch slot (does not participate in list scroll).
    Recent {
        id: Uuid,
        name: String,
        valid: bool,
        icon: Option<Arc<IconRgba>>,
    },
    Shortcut {
        id: Uuid,
        name: String,
        valid: bool,
        /// Real app icon (extracted from the shortcut target); None → letter disc.
        icon: Option<Arc<IconRgba>>,
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
    /// Glass panel rect in **window-local logical** coords (pin-pet).
    pub card_x: f32,
    pub card_y: f32,
    pub card_w: f32,
    pub card_h: f32,
    /// Total enabled shortcuts (may exceed viewport).
    pub list_total: usize,
    /// First visible shortcut index (scroll offset in rows).
    pub list_scroll: usize,
    pub list_can_scroll_up: bool,
    pub list_can_scroll_down: bool,
    /// List viewport top/bottom in window-local logical coords.
    pub list_top: f32,
    pub list_bottom: f32,
    /// Caption baseline area for「最近启用」(window-local logical).
    pub recent_label_y: f32,
    /// Fixed frequent-icon box (window-local logical).
    pub recent_box_x: f32,
    pub recent_box_y: f32,
    pub recent_box_w: f32,
    pub recent_box_h: f32,
    /// Caption baseline area for「应用列表」(window-local logical).
    pub list_label_y: f32,
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

/// Clamp scroll so a full viewport (or remaining tail) is always valid.
pub fn clamp_list_scroll(scroll: usize, total: usize) -> usize {
    let max_scroll = total.saturating_sub(LIST_VISIBLE_ROWS);
    scroll.min(max_scroll)
}

pub fn build_entries(
    shortcuts: &[ShortcutItem],
    mut icon_of: impl FnMut(&ShortcutItem) -> Option<Arc<IconRgba>>,
) -> Vec<MenuEntry> {
    let mut entries = vec![MenuEntry::AddShortcut, MenuEntry::Manage];
    for s in crate::shortcut::rank_frequent(shortcuts.iter(), RECENT_MAX) {
        entries.push(MenuEntry::Recent {
            id: s.id,
            name: s.name.clone(),
            valid: true,
            icon: icon_of(&s),
        });
    }
    for s in shortcuts
        .iter()
        .filter(|s| s.enabled)
        .take(MAX_SHORTCUTS)
    {
        entries.push(MenuEntry::Shortcut {
            id: s.id,
            name: s.name.clone(),
            valid: s.is_path_valid(),
            icon: icon_of(s),
        });
    }
    entries
}

/// Count shortcut entries in a built list.
pub fn count_shortcuts(entries: &[MenuEntry]) -> usize {
    entries
        .iter()
        .filter(|e| matches!(e, MenuEntry::Shortcut { .. }))
        .count()
}

/// Apple-like dock: avatar column + content column (legacy single-card window).
pub fn layout(entries: &[MenuEntry], dir: ExpandDir, open_t: f32) -> RadialLayout {
    layout_with_scroll(entries, dir, open_t, 0)
}

fn layout_with_scroll(
    entries: &[MenuEntry],
    dir: ExpandDir,
    open_t: f32,
    list_scroll: usize,
) -> RadialLayout {
    let t = open_t.clamp(0.0, 1.0);
    let ww = MENU_WINDOW_W as f32;
    let wh = MENU_WINDOW_H as f32;

    let pet_w = 77.0;
    let pet_h = 108.0;
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
    let built = layout_items_in_card(entries, content_x, 0.0, content_w, wh, list_scroll);

    RadialLayout {
        items: built.items,
        open_t: t,
        window: MENU_WINDOW_W,
        window_w: MENU_WINDOW_W,
        window_h: MENU_WINDOW_H,
        pet_x,
        pet_y,
        pet_w,
        pet_h,
        card_x: 0.0,
        card_y: 0.0,
        card_w: ww,
        card_h: wh,
        list_total: built.list_total,
        list_scroll: built.list_scroll,
        list_can_scroll_up: built.list_can_scroll_up,
        list_can_scroll_down: built.list_can_scroll_down,
        list_top: built.list_top,
        list_bottom: built.list_bottom,
        recent_label_y: built.recent_label_y,
        recent_box_x: built.recent_box_x,
        recent_box_y: built.recent_box_y,
        recent_box_w: built.recent_box_w,
        recent_box_h: built.recent_box_h,
        list_label_y: built.list_label_y,
    }
}

/// Pin-pet layout: pet and glass card are separate rects in a dynamic window.
///
/// Coordinates are **window-local logical** pixels (compose multiplies by dpr).
pub fn layout_pinned(
    entries: &[MenuEntry],
    window_w: u32,
    window_h: u32,
    pet: (f32, f32, f32, f32),
    card: (f32, f32, f32, f32),
    _dir: ExpandDir,
    open_t: f32,
) -> RadialLayout {
    layout_pinned_scroll(entries, window_w, window_h, pet, card, _dir, open_t, 0)
}

/// Like [`layout_pinned`] with explicit list scroll (row index).
pub fn layout_pinned_scroll(
    entries: &[MenuEntry],
    window_w: u32,
    window_h: u32,
    pet: (f32, f32, f32, f32),
    card: (f32, f32, f32, f32),
    _dir: ExpandDir,
    open_t: f32,
    list_scroll: usize,
) -> RadialLayout {
    let t = open_t.clamp(0.0, 1.0);
    let (pet_x, pet_y, pet_w, pet_h) = pet;
    let (card_x, card_y, card_w, card_h) = card;
    let margin = 16.0;
    let content_x = card_x + margin;
    let content_w = (card_w - margin * 2.0).max(40.0);
    let built = layout_items_in_card(entries, content_x, card_y, content_w, card_h, list_scroll);

    RadialLayout {
        items: built.items,
        open_t: t,
        window: window_w,
        window_w,
        window_h,
        pet_x,
        pet_y,
        pet_w,
        pet_h,
        card_x,
        card_y,
        card_w,
        card_h,
        list_total: built.list_total,
        list_scroll: built.list_scroll,
        list_can_scroll_up: built.list_can_scroll_up,
        list_can_scroll_down: built.list_can_scroll_down,
        list_top: built.list_top,
        list_bottom: built.list_bottom,
        recent_label_y: built.recent_label_y,
        recent_box_x: built.recent_box_x,
        recent_box_y: built.recent_box_y,
        recent_box_w: built.recent_box_w,
        recent_box_h: built.recent_box_h,
        list_label_y: built.list_label_y,
    }
}

struct BuiltList {
    items: Vec<ItemGeom>,
    list_total: usize,
    list_scroll: usize,
    list_can_scroll_up: bool,
    list_can_scroll_down: bool,
    list_top: f32,
    list_bottom: f32,
    recent_label_y: f32,
    recent_box_x: f32,
    recent_box_y: f32,
    recent_box_w: f32,
    recent_box_h: f32,
    list_label_y: f32,
}

/// Items in a vertical card stack. Shortcut rows fill a fixed viewport and scroll.
fn layout_items_in_card(
    entries: &[MenuEntry],
    content_x: f32,
    card_y: f32,
    content_w: f32,
    card_h: f32,
    list_scroll: usize,
) -> BuiltList {
    let mut y = card_y + TITLE_BAND;
    let mut items = Vec::new();

    if let Some(e) = entries.iter().find(|e| matches!(e, MenuEntry::AddShortcut)) {
        items.push(rect_item(e.clone(), content_x, y, content_w, PRIMARY_H));
        y += PRIMARY_H + PRIMARY_GAP;
    }

    if let Some(e) = entries.iter().find(|e| matches!(e, MenuEntry::Manage)) {
        let gx = content_x + content_w - GEAR_SIZE;
        // Sit in the title band, aligned with the two-line header.
        let gy = card_y + (TITLE_BAND - GEAR_SIZE) * 0.5;
        items.push(rect_item(e.clone(), gx, gy, GEAR_SIZE, GEAR_SIZE));
    }

    let recent_label_y = y;
    let recent_box_x = content_x;
    let recent_box_y = y + RECENT_LABEL_H + RECENT_LABEL_GAP;
    let recent_box_w = content_w;
    let recent_box_h = RECENT_BOX_H;
    let recents: Vec<&MenuEntry> = entries
        .iter()
        .filter(|e| matches!(e, MenuEntry::Recent { .. }))
        .take(RECENT_MAX)
        .collect();
    let slot = RECENT_SLOT;
    let mut rx = recent_box_x + RECENT_BOX_PAD;
    let ry = recent_box_y + ((recent_box_h - slot) * 0.5).max(0.0);
    let inner_right = recent_box_x + recent_box_w - RECENT_BOX_PAD;
    for e in recents {
        if rx + slot > inner_right + 0.5 {
            break;
        }
        items.push(rect_item(e.clone(), rx, ry, slot, slot));
        rx += slot + RECENT_SLOT_GAP;
    }
    y += RECENT_STRIP_H + PRIMARY_GAP;
    let list_label_y = y;
    y += RECENT_LABEL_H + RECENT_LABEL_GAP;

    let shortcuts: Vec<&MenuEntry> = entries
        .iter()
        .filter(|e| matches!(e, MenuEntry::Shortcut { .. }))
        .collect();
    let list_total = shortcuts.len();
    let list_scroll = clamp_list_scroll(list_scroll, list_total);
    let list_top = y;
    // Fixed viewport for N rows (independent of card_h shrink — still clamp to card).
    let viewport_h = LIST_VISIBLE_ROWS as f32 * ROW_H
        + (LIST_VISIBLE_ROWS.saturating_sub(1) as f32) * ROW_GAP;
    let list_bottom = (list_top + viewport_h).min(card_y + card_h - CARD_MARGIN - SCROLL_HINT_H);

    let mut row_y = list_top;
    let mut shown = 0usize;
    for e in shortcuts.iter().skip(list_scroll) {
        if shown >= LIST_VISIBLE_ROWS {
            break;
        }
        // Hard stop if card was shrunk by placement (small work area).
        if row_y + ROW_H > list_bottom + 0.5 {
            break;
        }
        items.push(rect_item((*e).clone(), content_x, row_y, content_w, ROW_H));
        row_y += ROW_H + ROW_GAP;
        shown += 1;
    }

    let list_can_scroll_up = list_scroll > 0;
    let list_can_scroll_down = list_scroll + shown < list_total;

    BuiltList {
        items,
        list_total,
        list_scroll,
        list_can_scroll_up,
        list_can_scroll_down,
        list_top,
        list_bottom: row_y - if shown > 0 { ROW_GAP } else { 0.0 },
        recent_label_y,
        recent_box_x,
        recent_box_y,
        recent_box_w,
        recent_box_h,
        list_label_y,
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
    hit_test_index(layout, local_x, local_y).map(|i| &layout.items[i].entry)
}

/// Topmost item index under point (for hover / press).
pub fn hit_test_index(layout: &RadialLayout, local_x: f32, local_y: f32) -> Option<usize> {
    for (i, item) in layout.items.iter().enumerate().rev() {
        if local_x >= item.x
            && local_x <= item.x + item.w
            && local_y >= item.y
            && local_y <= item.y + item.h
        {
            return Some(i);
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

    fn chrome_plus_shortcuts(n: usize) -> Vec<MenuEntry> {
        let mut entries = vec![MenuEntry::AddShortcut, MenuEntry::Manage];
        for i in 0..n {
            entries.push(MenuEntry::Shortcut {
                id: Uuid::nil(),
                name: format!("App{i}"),
                valid: true,
                icon: None,
            });
        }
        entries
    }

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

    #[test]
    fn gear_sits_in_title_band_not_over_add() {
        let entries = chrome_plus_shortcuts(1);
        let lay = layout_pinned(
            &entries,
            500,
            480,
            (8.0, 80.0, 128.0, 128.0),
            (
                150.0,
                10.0,
                CARD_LOGICAL_W as f32,
                CARD_LOGICAL_H as f32,
            ),
            ExpandDir::Right,
            1.0,
        );
        let gear = lay
            .items
            .iter()
            .find(|it| matches!(it.entry, MenuEntry::Manage))
            .expect("settings paw mark");
        let add = lay
            .items
            .iter()
            .find(|it| matches!(it.entry, MenuEntry::AddShortcut))
            .expect("add button");
        assert!((gear.w - GEAR_SIZE).abs() < 0.1);
        assert!(gear.y >= lay.card_y);
        assert!(gear.y + gear.h <= lay.card_y + TITLE_BAND + 0.5);
        assert!(gear.x + gear.w <= lay.card_x + lay.card_w + 0.5);
        assert!(
            gear.y + gear.h <= add.y + 0.5,
            "paw mark must stay in the title band above 添加应用"
        );
        assert!(
            (add.x - lay.card_x).abs() < 24.0,
            "add/title column stays on the left of the card"
        );
        assert!(
            gear.x > add.x + add.w * 0.5,
            "paw mark stays on the right, away from title"
        );
    }

    #[test]
    fn layout_pinned_items_inside_card() {
        let entries = chrome_plus_shortcuts(0);
        let lay = layout_pinned(
            &entries,
            500,
            480,
            (8.0, 80.0, 128.0, 128.0),
            (
                150.0,
                10.0,
                CARD_LOGICAL_W as f32,
                CARD_LOGICAL_H as f32,
            ),
            ExpandDir::Right,
            1.0,
        );
        assert_eq!(lay.window_w, 500);
        for it in &lay.items {
            assert!(it.x >= lay.card_x);
            assert!(it.x + it.w <= lay.card_x + lay.card_w + 0.5);
            assert!(it.y >= lay.card_y);
            assert!(it.y + it.h <= lay.card_y + lay.card_h + 0.5);
        }
    }

    #[test]
    fn three_shortcuts_all_visible_without_scroll() {
        let entries = chrome_plus_shortcuts(3);
        let lay = layout_pinned_scroll(
            &entries,
            500,
            500,
            (8.0, 40.0, 100.0, 100.0),
            (120.0, 8.0, CARD_LOGICAL_W as f32, CARD_LOGICAL_H as f32),
            ExpandDir::Right,
            1.0,
            0,
        );
        let n = lay
            .items
            .iter()
            .filter(|it| matches!(it.entry, MenuEntry::Shortcut { .. }))
            .count();
        assert_eq!(n, 3, "3 apps must all fit in viewport of {LIST_VISIBLE_ROWS}");
        assert_eq!(lay.list_total, 3);
        assert!(!lay.list_can_scroll_down);
    }

    #[test]
    fn many_shortcuts_scroll_window() {
        let entries = chrome_plus_shortcuts(12);
        let lay0 = layout_pinned_scroll(
            &entries,
            500,
            500,
            (8.0, 40.0, 100.0, 100.0),
            (120.0, 8.0, CARD_LOGICAL_W as f32, CARD_LOGICAL_H as f32),
            ExpandDir::Right,
            1.0,
            0,
        );
        assert_eq!(lay0.list_total, 12);
        assert_eq!(
            lay0.items
                .iter()
                .filter(|it| matches!(it.entry, MenuEntry::Shortcut { .. }))
                .count(),
            LIST_VISIBLE_ROWS
        );
        assert!(lay0.list_can_scroll_down);
        assert!(!lay0.list_can_scroll_up);

        let lay2 = layout_pinned_scroll(
            &entries,
            500,
            500,
            (8.0, 40.0, 100.0, 100.0),
            (120.0, 8.0, CARD_LOGICAL_W as f32, CARD_LOGICAL_H as f32),
            ExpandDir::Right,
            1.0,
            2,
        );
        assert_eq!(lay2.list_scroll, 2);
        assert!(lay2.list_can_scroll_up);
        assert!(lay2.list_can_scroll_down);

        // Names should shift with scroll
        let names0: Vec<_> = lay0
            .items
            .iter()
            .filter_map(|it| match &it.entry {
                MenuEntry::Shortcut { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        let names2: Vec<_> = lay2
            .items
            .iter()
            .filter_map(|it| match &it.entry {
                MenuEntry::Shortcut { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(names0[0], "App0");
        assert_eq!(names2[0], "App2");
    }

    #[test]
    fn clamp_list_scroll_bounds() {
        assert_eq!(clamp_list_scroll(0, 3), 0);
        assert_eq!(clamp_list_scroll(10, 3), 0); // fewer than viewport
        assert_eq!(clamp_list_scroll(10, 12), 12 - LIST_VISIBLE_ROWS);
    }

    #[test]
    fn build_entries_includes_more_than_five() {
        let mut items = Vec::new();
        for i in 0..10 {
            items.push(ShortcutItem::new(format!("A{i}"), std::path::PathBuf::from("x"), i as u32));
        }
        let e = build_entries(&items, |_| None);
        assert_eq!(count_shortcuts(&e), 10);
        assert!(e.iter().all(|e| !matches!(e, MenuEntry::Recent { .. })));
    }

    fn chrome_plus_recents_and_shortcuts(n_recent: usize, n_list: usize) -> Vec<MenuEntry> {
        let mut entries = vec![MenuEntry::AddShortcut, MenuEntry::Manage];
        for i in 0..n_recent {
            entries.push(MenuEntry::Recent {
                id: Uuid::nil(),
                name: format!("R{i}"),
                valid: true,
                icon: None,
            });
        }
        for i in 0..n_list {
            entries.push(MenuEntry::Shortcut {
                id: Uuid::nil(),
                name: format!("App{i}"),
                valid: true,
                icon: None,
            });
        }
        entries
    }

    #[test]
    fn recents_sit_between_add_and_list() {
        let entries = chrome_plus_recents_and_shortcuts(3, 2);
        let lay = layout_pinned(
            &entries,
            500,
            500,
            (8.0, 40.0, 100.0, 100.0),
            (120.0, 8.0, CARD_LOGICAL_W as f32, CARD_LOGICAL_H as f32),
            ExpandDir::Right,
            1.0,
        );
        let add = lay
            .items
            .iter()
            .find(|it| matches!(it.entry, MenuEntry::AddShortcut))
            .unwrap();
        let recents: Vec<_> = lay
            .items
            .iter()
            .filter(|it| matches!(it.entry, MenuEntry::Recent { .. }))
            .collect();
        let list: Vec<_> = lay
            .items
            .iter()
            .filter(|it| matches!(it.entry, MenuEntry::Shortcut { .. }))
            .collect();
        assert_eq!(recents.len(), 3);
        assert_eq!(list.len(), 2);
        assert_eq!(lay.list_total, 2, "recents must not count toward list scroll");
        assert!(lay.recent_label_y >= add.y + add.h - 0.5);
        assert!((lay.recent_box_h - RECENT_BOX_H).abs() < 0.1);
        assert!((lay.recent_box_w - (CARD_LOGICAL_W as f32 - 32.0)).abs() < 0.5);
        for r in &recents {
            assert!(r.y >= lay.recent_box_y - 0.5, "icon inside box");
            assert!(r.y + r.h <= lay.recent_box_y + lay.recent_box_h + 0.5);
            assert!(r.x >= lay.recent_box_x - 0.5);
            assert!(r.x + r.w <= lay.recent_box_x + lay.recent_box_w + 0.5);
            assert!(r.y + r.h <= list[0].y + 0.5, "recent above first list row");
            assert!((r.w - RECENT_SLOT).abs() < 0.1);
            assert!((r.h - RECENT_SLOT).abs() < 0.1);
        }
        assert!(recents[1].x > recents[0].x);
        assert!(recents[2].x > recents[1].x);
        assert!(lay.list_label_y >= lay.recent_box_y + lay.recent_box_h - 0.5);
        assert!(list[0].y >= lay.list_label_y + RECENT_LABEL_H - 0.5);
    }

    #[test]
    fn empty_recents_still_reserves_labeled_box() {
        let with = layout_pinned(
            &chrome_plus_recents_and_shortcuts(0, 1),
            500,
            500,
            (8.0, 40.0, 100.0, 100.0),
            (120.0, 8.0, CARD_LOGICAL_W as f32, CARD_LOGICAL_H as f32),
            ExpandDir::Right,
            1.0,
        );
        let add = with
            .items
            .iter()
            .find(|it| matches!(it.entry, MenuEntry::AddShortcut))
            .unwrap();
        let row = with
            .items
            .iter()
            .find(|it| matches!(it.entry, MenuEntry::Shortcut { .. }))
            .unwrap();
        assert!(with.recent_box_w > 40.0);
        assert!((with.recent_box_h - RECENT_BOX_H).abs() < 0.1);
        assert!(with.recent_label_y >= add.y + add.h - 0.5);
        assert!(with.list_label_y >= with.recent_box_y + with.recent_box_h - 0.5);
        assert!(row.y >= with.list_label_y + RECENT_LABEL_H - 0.5);
        assert!(with.items.iter().all(|it| !matches!(it.entry, MenuEntry::Recent { .. })));
    }

    #[test]
    fn build_entries_ranks_frequent_and_caps() {
        let mut items = Vec::new();
        for i in 0..8 {
            let mut s = ShortcutItem::new(
                format!("A{i}"),
                std::path::PathBuf::from("."),
                i as u32,
            );
            s.launch_count = (8 - i) as u32;
            s.last_launched_at_ms = Some(1000 + i as u64);
            items.push(s);
        }
        let e = build_entries(&items, |_| None);
        let recents: Vec<_> = e
            .iter()
            .filter_map(|en| match en {
                MenuEntry::Recent { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(recents.len(), RECENT_MAX);
        assert_eq!(recents[0], "A0");
        assert_eq!(recents[5], "A5");
        assert_eq!(count_shortcuts(&e), 8);
    }
}
