//! Playful pin-pet dock: the cat presents a warm glass card (speech-tail).
//!
//! Drawn at **device pixel ratio** so text/edges stay sharp on HiDPI
//! (no low-res compose → bilinear upscale blur). No system Acrylic.

use std::path::PathBuf;
use std::sync::OnceLock;

use crate::render::easing::lerp;
use crate::render::rgba::sample_rgba_bilinear;
use crate::render::text::{center_in_rect, rasterize_text};
use crate::shortcut::{scale_icon_rgba, IconRgba, IconShape};
use crate::ui::list_drag::{bowl_rect, BOWL_SIZE};
use crate::ui::radial_menu::{ExpandDir, MenuEntry, RadialLayout, RECENT_ICON, ROW_GAP, ROW_H};

// ── Palette (playful warm glass · design §2 tokens) ───────────────────────
/// Cream card, a touch rosier than the old near-white.
const CARD: [u8; 4] = [0xFF, 0xF8, 0xF4, 0xF0];
/// Soft muted surfaces (rows / chips) — pink wash.
const GROUPED_BG: [u8; 4] = [0xFF, 0xEC, 0xF2, 0x8C];
const GROUPED_HOVER: [u8; 4] = [0xFF, 0xD6, 0xE2, 0xB8];
const GROUPED_PRESS: [u8; 4] = [0xF6, 0xD0, 0xDC, 0xD0];
const INVALID_BG: [u8; 4] = [0xFF, 0xB0, 0x20, 0x30];
const INVALID_BG_HOVER: [u8; 4] = [0xFF, 0xB0, 0x20, 0x48];
const BORDER: [u8; 4] = [0xFF, 0x9E, 0xC4, 0x48];
const INNER_HL: [u8; 4] = [0xFF, 0xFF, 0xFF, 0x55];
const PAW_INK: [u8; 4] = [0x9A, 0x40, 0x68, 0xFF];
const PAW_KICKER: [u8; 4] = [0xFF, 0x7A, 0xAF, 0xFF];
const TITLE: &str = "给你叼来了";
const SUBTITLE: &str = "想开哪个？";
const RECENT_CAPTION: &str = "最近启用";
const LIST_CAPTION: &str = "应用列表";
const ADD_LABEL: &str = "再叼一个";
const EMPTY_TITLE: &str = "还没叼来应用";
const EMPTY_HINT: &str = "点「再叼一个」选 exe / 快捷方式";
pub const SAY_LAUNCH: &str = "收到，马上打开～";
pub const SAY_FAIL: &str = "这个应用好像搬家了…";
pub const SAY_EATEN: &str = "唔，吃掉啦";
/// Settings 「暂停」 is decorative: the cat refuses in first person.
pub const SAY_NO_PAUSE: &str = "我没有这个功能，\n该喝水喝水，该活动活动";
const DELETE_HINT: &str = "喂给我删除";
/// text.intense
const LABEL: [u8; 4] = [0x0F, 0x17, 0x2B, 0xFF];
const SECONDARY: [u8; 4] = [0x47, 0x55, 0x69, 0xC8];
const TERTIARY: [u8; 4] = [0x64, 0x74, 0x8B, 0xA0];
/// Appica-like deep primary (slate).
const PRIMARY: [u8; 4] = [0x1E, 0x29, 0x3B, 0xFF];
const PRIMARY_HOVER: [u8; 4] = [0x33, 0x41, 0x55, 0xFF];
const PRIMARY_PRESS: [u8; 4] = [0x0F, 0x17, 0x2B, 0xFF];
/// Kept for settings status / links that still read as “system”.
const BLUE: [u8; 4] = [0x25, 0x63, 0xEB, 0xFF];
const BLUE_PRESS: [u8; 4] = [0x1D, 0x4E, 0xD8, 0xFF];
const FILL_OPAQUE: [u8; 4] = [0xF1, 0xEF, 0xED, 0xE8];
const FILL_HOVER: [u8; 4] = [0xFF, 0xFF, 0xFF, 0xF0];
const SOFT_BORDER: [u8; 4] = [0x1E, 0x1B, 0x2E, 0x14];
const ORANGE: [u8; 4] = [0xC2, 0x71, 0x0A, 0xFF];
const WHITE: [u8; 4] = [0xFF, 0xFF, 0xFF, 0xFF];
const ACCENT_PINK: [u8; 4] = [0xFF, 0x9E, 0xC4, 0xFF];
const SHADOW_A: [u8; 4] = [0x0F, 0x17, 0x2B, 0x0A];
const SHADOW_B: [u8; 4] = [0x0F, 0x17, 0x2B, 0x12];
const SHADOW_BTN: [u8; 4] = [0x0F, 0x17, 0x2B, 0x28];

/// Live list-drag overlay (not Copy — holds the row snapshot).
#[derive(Debug, Clone)]
pub struct MenuDragChrome {
    pub id: uuid::Uuid,
    pub name: String,
    pub valid: bool,
    pub icon: Option<std::sync::Arc<crate::shortcut::IconRgba>>,
    pub pointer_x: f32,
    pub pointer_y: f32,
    pub grab_dx: f32,
    pub grab_dy: f32,
    pub from: usize,
    pub insert_at: usize,
    pub over_bowl: bool,
    pub row_w: f32,
    pub row_h: f32,
    /// Pre-rendered lifted row (no drop shadow), device px. Built once at drag
    /// start so per-frame drag composition never re-rasterizes text / icons.
    pub ghost_img: Option<(u32, u32, Vec<u8>)>,
    /// Pre-rendered delete bowl at rest size (see [`prerender_drag_images`]).
    pub bowl_img: Option<(u32, u32, Vec<u8>)>,
    /// Bowl at 1.08× while the pointer is over it.
    pub bowl_over_img: Option<(u32, u32, Vec<u8>)>,
    /// 「喂给我删除」glyphs in both states (ink / kicker).
    pub hint_ink: Option<(u32, u32, Vec<u8>)>,
    pub hint_kicker: Option<(u32, u32, Vec<u8>)>,
}

/// Hover / press indices into `layout.items` + animated blends (0..1).
#[derive(Debug, Clone, Default)]
pub struct MenuChromeState {
    pub hover: Option<usize>,
    pub press: Option<usize>,
    /// Animated intensity for current hover target.
    pub hover_t: f32,
    /// Animated intensity while press is held.
    pub press_t: f32,
    /// Closing path (settings handoff still tags this).
    pub closing: bool,
    /// Optional speech from the cat (launch / fail).
    pub say: Option<&'static str>,
    /// Windows client-area animation off → fade only, no scale.
    pub reduced_motion: bool,
    /// Long-press reorder / feed-to-delete overlay.
    pub drag: Option<MenuDragChrome>,
    /// Draft mode for the cached card layer: shift rows around the dragged one
    /// but omit the moving ghost (composited per frame by [`present_menu_drag`]).
    pub drag_draft: bool,
    /// Blank every shortcut row (static card layer during drag). Rows are
    /// pre-rendered separately and blitted per frame at shifted positions.
    pub rows_blank: bool,
}

/// Popover grow start. Never `0` — nothing appears from nothing.
pub const MENU_GROW_FROM: f32 = 0.95;
/// Fade runs slightly ahead of scale (material before silhouette).
const MENU_FADE_LEAD: f32 = 1.22;

/// Card scale for visual open amount `t` (already ease-out from the clock).
pub fn menu_visual_scale(t: f32) -> f32 {
    MENU_GROW_FROM + (1.0 - MENU_GROW_FROM) * t.clamp(0.0, 1.0)
}

/// Card fade for visual open amount `t`. Leads scale so glass reads first.
pub fn menu_visual_fade(t: f32) -> f32 {
    (t.clamp(0.0, 1.0) * MENU_FADE_LEAD).min(1.0)
}

/// Device-pixel scale helper. Layout is logical; drawing is physical.
#[derive(Clone, Copy)]
struct Dpi {
    dpr: f32,
}

impl Dpi {
    fn new(dpr: f32) -> Self {
        // Snap near-integer DPR for pixel-perfect UI (1.0 / 1.25 / 1.5 / 2.0…)
        let d = dpr.clamp(1.0, 3.0);
        let snapped = if (d - d.round()).abs() < 0.08 {
            d.round()
        } else {
            d
        };
        Self { dpr: snapped }
    }
    #[inline]
    fn s(self, v: f32) -> f32 {
        v * self.dpr
    }
    #[inline]
    fn su(self, v: u32) -> u32 {
        ((v as f32) * self.dpr).round().max(1.0) as u32
    }
    #[inline]
    fn px(self, logical_pt: f32) -> f32 {
        (logical_pt * self.dpr).max(8.0)
    }
}

/// Vertical stride between shortcut row slots.
pub fn row_stride() -> f32 {
    ROW_H + ROW_GAP
}

/// Slot y (window-local logical) for the shortcut row at `orig` during a drag,
/// after removing the dragged row and inserting it at `insert_at`.
pub fn drag_slot_y(layout: &RadialLayout, orig: usize, from: usize, insert_at: usize) -> f32 {
    let remaining_index = if orig > from { orig - 1 } else { orig };
    let visual_index = if remaining_index >= insert_at {
        remaining_index + 1
    } else {
        remaining_index
    };
    layout.list_top + (visual_index as f32 - layout.list_scroll as f32) * row_stride()
}

// ── Public: launcher ──────────────────────────────────────────────────────

pub fn compose_menu_frame(
    pet_rgba: &[u8],
    pet_w: u32,
    pet_h: u32,
    layout: &RadialLayout,
    dpr: f32,
    chrome: MenuChromeState,
) -> (u32, u32, Vec<u8>) {
    let dpi = Dpi::new(dpr);
    let w = dpi.su(layout.window_w);
    let h = dpi.su(layout.window_h);
    let mut out = vec![0u8; (w * h * 4) as usize];

    // `open_t` is already ease-out visual 0..1 (pet::tick_menu_anim).
    let t_vis = layout.open_t.clamp(0.0, 1.0);
    let t_fade = menu_visual_fade(t_vis);
    let scale = if chrome.reduced_motion {
        1.0
    } else {
        menu_visual_scale(t_vis)
    };
    // The pet rect never participates in the card's grow scale — the cat must
    // stay exactly where it rests while the card scales around it.
    let pet_rect = (layout.pet_x, layout.pet_y, layout.pet_w, layout.pet_h);
    let layout_scaled;
    let layout = if (scale - 1.0).abs() < 0.001 {
        layout
    } else {
        let pivot_x = layout.pet_x + layout.pet_w * 0.5;
        let pivot_y = layout.pet_y + layout.pet_h * 0.5;
        layout_scaled = scale_layout_from_pivot(layout, pivot_x, pivot_y, scale);
        &layout_scaled
    };

    paint_menu_card(&mut out, w, h, dpi, layout, &chrome, t_fade);

    // Pet always full opacity, free-standing (no glass tray behind it).
    draw_avatar(
        &mut out,
        w,
        dpi,
        pet_rect.0,
        pet_rect.1,
        pet_rect.2,
        pet_rect.3,
        pet_rgba,
        pet_w,
        pet_h,
    );
    if let Some(line) = chrome.say {
        draw_say_bubble(
            &mut out,
            w,
            h,
            dpi.dpr,
            Some(layout),
            None,
            line,
            t_fade.max(0.85),
        );
    }
    // The delete bowl after the pet so its hint never sits under the silhouette.
    if let Some(drag) = chrome.drag.as_ref() {
        draw_bowl_state(&mut out, w, h, dpi, layout, drag, t_fade.max(0.85));
    }

    (w, h, out)
}

/// Rest-state card layer (no pet). Used as the open/close bitmap cache.
pub fn compose_menu_card_layer(
    layout: &RadialLayout,
    dpr: f32,
    chrome: MenuChromeState,
) -> (u32, u32, Vec<u8>) {
    let dpi = Dpi::new(dpr);
    let w = dpi.su(layout.window_w);
    let h = dpi.su(layout.window_h);
    let mut out = vec![0u8; (w * h * 4) as usize];
    paint_menu_card(&mut out, w, h, dpi, layout, &chrome, 1.0);
    (w, h, out)
}

/// Pet silhouette only — first present after resize, before the card cache exists.
pub fn compose_menu_pet_only(
    pet_rgba: &[u8],
    pet_w: u32,
    pet_h: u32,
    layout: &RadialLayout,
    dpr: f32,
) -> (u32, u32, Vec<u8>) {
    let dpi = Dpi::new(dpr);
    let w = dpi.su(layout.window_w);
    let h = dpi.su(layout.window_h);
    let mut out = vec![0u8; (w * h * 4) as usize];
    draw_avatar(
        &mut out,
        w,
        dpi,
        layout.pet_x,
        layout.pet_y,
        layout.pet_w,
        layout.pet_h,
        pet_rgba,
        pet_w,
        pet_h,
    );
    (w, h, out)
}

/// Settings live-size preview: the pet drawn inside its fixed layout rect,
/// scaled by `scale_ratio`. The rect grows right/down from the layout rect's
/// top-left — the same anchor the real pet window resize uses — so the
/// previewed desk position matches where the pet rests after commit.
pub fn compose_menu_pet_preview(
    pet_rgba: &[u8],
    pet_w: u32,
    pet_h: u32,
    layout: &RadialLayout,
    dpr: f32,
    scale_ratio: f32,
) -> (u32, u32, Vec<u8>) {
    let dpi = Dpi::new(dpr);
    let w = dpi.su(layout.window_w);
    let h = dpi.su(layout.window_h);
    let mut out = vec![0u8; (w * h * 4) as usize];
    let k = scale_ratio.clamp(0.001, 4.0);
    let pw = layout.pet_w * k;
    let ph = layout.pet_h * k;
    let px = layout.pet_x;
    let py = layout.pet_y;
    draw_avatar(
        &mut out,
        w,
        dpi,
        px,
        py,
        pw,
        ph,
        pet_rgba,
        pet_w,
        pet_h,
    );
    (w, h, out)
}

/// Cheap anim frame: scale+fade a rest card around the pet, then blit the pet.
pub fn present_menu_cached(
    dest: &mut [u8],
    dw: u32,
    dh: u32,
    card: &[u8],
    cw: u32,
    ch: u32,
    pet_rgba: &[u8],
    pet_src_w: u32,
    pet_src_h: u32,
    layout: &RadialLayout,
    dpr: f32,
    scale: f32,
    fade: f32,
) {
    dest.fill(0);
    if dw == 0 || dh == 0 || dest.len() < (dw * dh * 4) as usize {
        return;
    }
    let dpi = Dpi::new(dpr);
    if fade > 0.01 {
        blit_card_scaled_around_pet(dest, dw, dh, card, cw, ch, layout, dpi, scale, fade);
    }
    draw_avatar(
        dest,
        dw,
        dpi,
        layout.pet_x,
        layout.pet_y,
        layout.pet_w,
        layout.pet_h,
        pet_rgba,
        pet_src_w,
        pet_src_h,
    );
}

/// Base-state list row (icon tile + name + chevron) as a standalone bitmap, so
/// the drag fast path can blit rows without re-rasterizing text per frame.
pub fn prerender_list_row(
    name: &str,
    valid: bool,
    icon: Option<&IconRgba>,
    dpr: f32,
    row_w: f32,
    row_h: f32,
) -> (u32, u32, Vec<u8>) {
    let dpi = Dpi::new(dpr);
    let w = dpi.s(row_w).round().max(1.0) as u32;
    let h = dpi.s(row_h).round().max(1.0) as u32;
    let mut img = vec![0u8; (w * h * 4) as usize];
    draw_list_row(
        &mut img,
        w,
        h,
        dpi,
        0.0,
        0.0,
        w as f32,
        h as f32,
        dpi.s(14.0),
        name,
        valid,
        icon,
        0.0,
        0.0,
        1.0,
    );
    (w, h, img)
}

/// Rows for the current scroll window, in `layout.items` shortcut order.
pub fn prerender_list_rows(layout: &RadialLayout, dpr: f32) -> Vec<(u32, u32, Vec<u8>)> {
    let mut rows = Vec::new();
    for item in &layout.items {
        let MenuEntry::Shortcut { name, valid, icon, .. } = &item.entry else {
            continue;
        };
        rows.push(prerender_list_row(
            name, *valid, icon.as_deref(), dpr, item.w, item.h,
        ));
    }
    rows
}

/// Per-frame drag composition: blit the static card layer (rows blanked) + the
/// pre-rendered rows at their shifted slots + the moving parts (pet, lifted
/// row, delete bowl). No text rasterization / icon rescale on the hot path.
pub fn present_menu_drag(
    dest: &mut [u8],
    dw: u32,
    dh: u32,
    base: &[u8],
    rows: &[(u32, u32, Vec<u8>)],
    pet_rgba: &[u8],
    pet_src_w: u32,
    pet_src_h: u32,
    layout: &RadialLayout,
    dpr: f32,
    say: Option<&'static str>,
    drag: Option<&MenuDragChrome>,
) {
    let need = (dw * dh * 4) as usize;
    if need == 0 || dest.len() < need || base.len() < need {
        return;
    }
    dest[..need].copy_from_slice(&base[..need]);
    let dpi = Dpi::new(dpr);
    draw_avatar(
        dest,
        dw,
        dpi,
        layout.pet_x,
        layout.pet_y,
        layout.pet_w,
        layout.pet_h,
        pet_rgba,
        pet_src_w,
        pet_src_h,
    );
    if let Some(line) = say {
        draw_say_bubble(dest, dw, dh, dpi.dpr, Some(layout), None, line, 1.0);
    }
    let Some(drag) = drag else {
        return;
    };

    // Pre-rendered rows at their visual (shifted) slots. Index k counts every
    // shortcut in layout order — the dragged row's bitmap is skipped, not removed.
    let mut k = 0usize;
    for item in &layout.items {
        let MenuEntry::Shortcut { id, .. } = &item.entry else {
            continue;
        };
        let Some((iw, ih, img)) = rows.get(k) else {
            break;
        };
        if *id != drag.id {
            let orig = layout.list_scroll + k;
            let logical_y = drag_slot_y(layout, orig, drag.from, drag.insert_at);
            if logical_y + item.h >= layout.list_top - 2.0
                && logical_y <= layout.list_bottom + 2.0
            {
                blit(
                    dest,
                    dw,
                    dh,
                    img,
                    *iw,
                    *ih,
                    dpi.s(item.x).round().max(0.0) as u32,
                    dpi.s(logical_y).round().max(0.0) as u32,
                );
            }
        }
        k += 1;
    }

    // Lifted row: pre-rendered bitmap at the pointer.
    if let Some((iw, ih, img)) = &drag.ghost_img {
        let lift = GHOST_LIFT;
        let bw = dpi.s(drag.row_w) * lift;
        let bh = dpi.s(drag.row_h) * lift;
        let x = dpi.s(drag.pointer_x - drag.grab_dx) - (bw - dpi.s(drag.row_w)) * 0.5;
        let y = dpi.s(drag.pointer_y - drag.grab_dy) - (bh - dpi.s(drag.row_h)) * 0.5;
        let (gpad_x, gpad_y) = ghost_pad(dpi);
        blit(
            dest,
            dw,
            dh,
            img,
            *iw,
            *ih,
            (x - gpad_x).round().max(0.0) as u32,
            (y - gpad_y).round().max(0.0) as u32,
        );
    } else {
        draw_drag_ghost(dest, dw, dh, dpi, drag, 1.0);
    }

    // Delete bowl + hint, from pre-rendered state images.
    draw_bowl_state(dest, dw, dh, dpi, layout, drag, 1.0);
}

fn paint_menu_card(
    mut out: &mut [u8],
    w: u32,
    h: u32,
    dpi: Dpi,
    layout: &RadialLayout,
    chrome: &MenuChromeState,
    t_fade: f32,
) {
    // Elevated glass card (rest of union window stays transparent for pin-pet).
    let cx0 = dpi.s(layout.card_x);
    let cy0 = dpi.s(layout.card_y);
    let cx1 = dpi.s(layout.card_x + layout.card_w);
    let cy1 = dpi.s(layout.card_y + layout.card_h);
    let crad = dpi.s(22.0);

    if t_fade > 0.01 {
        // No outer drop shadow: the card sits flush on the desktop, with a
        // single 1 px border line (deeper layers like a shadow here only read
        // as a second line along the bottom / right edge).
        fill_rrect_aa(
            &mut out,
            w,
            h,
            cx0,
            cy0,
            cx1,
            cy1,
            crad,
            with_alpha(CARD, t_fade),
        );
        // Top highlight: clipped to the card outline, soft vertical fade so the
        // rounded corners and the highlight fully merge (no contour line).
        fill_top_sheen(
            &mut out,
            w,
            h,
            cx0,
            cy0,
            cx1,
            cy1,
            crad,
            1.5,
            26.0,
            with_alpha(INNER_HL, t_fade),
        );
        stroke_rrect_aa(
            &mut out,
            w,
            h,
            cx0 + 0.5,
            cy0 + 0.5,
            cx1 - 0.5,
            cy1 - 0.5,
            crad,
            with_alpha(BORDER, t_fade),
            1.0,
        );
        draw_card_tail(&mut out, w, h, dpi, &layout, t_fade);
    }

    let content_x = dpi.s(content_x_from(&layout));
    let content_w = dpi.s(content_w_from(&layout));

    // Title: soft fade with card (no extra vertical jump).
    let title_a = t_fade;
    if title_a > 0.02 {
        let gear_left = layout
            .items
            .iter()
            .find(|i| matches!(i.entry, MenuEntry::Manage))
            .map(|i| dpi.s(i.x) - dpi.s(8.0));
        let title_max = gear_left
            .map(|gx| (gx - content_x).max(dpi.s(80.0)))
            .unwrap_or_else(|| (content_w - dpi.s(36.0)).max(dpi.s(80.0)))
            as u32;
        blit_text(
            &mut out,
            w,
            h,
            TITLE,
            content_x,
            dpi.s(layout.card_y + 15.0),
            title_max,
            dpi.px(17.0),
            with_alpha(LABEL, title_a),
        );
        blit_text(
            &mut out,
            w,
            h,
            SUBTITLE,
            content_x,
            dpi.s(layout.card_y + 36.0),
            title_max,
            dpi.px(12.5),
            with_alpha(PAW_KICKER, title_a),
        );
    }

    if title_a > 0.02 && layout.recent_box_w > 1.0 {
        blit_text(
            &mut out,
            w,
            h,
            RECENT_CAPTION,
            dpi.s(layout.recent_box_x),
            dpi.s(layout.recent_label_y),
            dpi.s(layout.recent_box_w).max(dpi.s(80.0)) as u32,
            dpi.px(11.5),
            with_alpha(SECONDARY, title_a),
        );
        let bx0 = dpi.s(layout.recent_box_x);
        let by0 = dpi.s(layout.recent_box_y);
        let bx1 = bx0 + dpi.s(layout.recent_box_w);
        let by1 = by0 + dpi.s(layout.recent_box_h);
        let br = dpi.s(14.0);
        fill_rrect_aa(
            &mut out,
            w,
            h,
            bx0,
            by0,
            bx1,
            by1,
            br,
            with_alpha(GROUPED_BG, title_a),
        );
        stroke_rrect_aa(
            &mut out,
            w,
            h,
            bx0 + 0.5,
            by0 + 0.5,
            bx1 - 0.5,
            by1 - 0.5,
            br,
            with_alpha(SOFT_BORDER, title_a * 0.85),
            1.0,
        );
        blit_text(
            &mut out,
            w,
            h,
            LIST_CAPTION,
            dpi.s(layout.recent_box_x),
            dpi.s(layout.list_label_y),
            dpi.s(layout.recent_box_w).max(dpi.s(80.0)) as u32,
            dpi.px(11.5),
            with_alpha(SECONDARY, title_a),
        );
    }

    // Tens/day: items fade with the card. No stagger, no extra y.
    let mut saw_shortcut = false;
    let mut vis_shortcut = 0usize;
    for (i, item) in layout.items.iter().enumerate() {
        let reveal = t_fade;
        if reveal <= 0.01 {
            continue;
        }
        let item_dy = 0.0;
        let hover_w = if chrome.hover == Some(i) {
            chrome.hover_t.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let press_w = if chrome.press == Some(i) {
            chrome.press_t.clamp(0.0, 1.0)
        } else {
            0.0
        };
        // Press: scale 0.97 + +1px y (Appica active)
        let pscale = 1.0 - 0.03 * press_w;
        let mut x = dpi.s(item.x);
        let mut y = dpi.s(item.y) + item_dy + dpi.s(1.0) * press_w;
        let mut bw = dpi.s(item.w);
        let mut bh = dpi.s(item.h);
        let cx = x + bw * 0.5;
        let cy = y + bh * 0.5;
        bw *= pscale;
        bh *= pscale;
        x = cx - bw * 0.5;
        y = cy - bh * 0.5;
        let radius = match &item.entry {
            MenuEntry::Shortcut { .. } => dpi.s(14.0),
            MenuEntry::Recent { .. } => dpi.s(12.0),
            _ => dpi.s(11.0),
        };

        match &item.entry {
            MenuEntry::AddShortcut => {
                draw_fetch_btn(
                    &mut out,
                    w,
                    h,
                    dpi,
                    x,
                    y,
                    bw,
                    bh,
                    hover_w,
                    press_w,
                    reveal,
                );
            }
            MenuEntry::Manage => {
                draw_settings_btn(
                    &mut out,
                    w,
                    h,
                    dpi,
                    x,
                    y,
                    bw,
                    bh,
                    hover_w,
                    press_w,
                    reveal,
                );
            }
            MenuEntry::Recent { name, valid, icon, .. } => {
                draw_recent_icon(
                    &mut out,
                    w,
                    h,
                    dpi,
                    x,
                    y,
                    bw,
                    bh,
                    name,
                    *valid,
                    icon.as_deref(),
                    hover_w,
                    press_w,
                    reveal,
                );
            }
            MenuEntry::Shortcut { name, valid, icon, id } => {
                if chrome.rows_blank {
                    continue;
                }
                saw_shortcut = true;
                let orig = layout.list_scroll + vis_shortcut;
                vis_shortcut += 1;
                if chrome.drag.as_ref().is_some_and(|d| d.id == *id) {
                    continue;
                }
                if let Some(drag) = chrome.drag.as_ref() {
                    let logical_y = drag_slot_y(layout, orig, drag.from, drag.insert_at);
                    if logical_y + item.h < layout.list_top - 2.0
                        || logical_y > layout.list_bottom + 2.0
                    {
                        continue;
                    }
                    x = dpi.s(item.x);
                    y = dpi.s(logical_y);
                    bw = dpi.s(item.w);
                    bh = dpi.s(item.h);
                }
                draw_list_row(
                    &mut out,
                    w,
                    h,
                    dpi,
                    x,
                    y,
                    bw,
                    bh,
                    radius,
                    name,
                    *valid,
                    icon.as_deref(),
                    hover_w,
                    press_w,
                    reveal,
                );
            }
        }
    }

    if let Some(drag) = chrome.drag.as_ref() {
        saw_shortcut = true;
        if !chrome.drag_draft {
            draw_drag_ghost(&mut out, w, h, dpi, drag, t_fade);
        }
    }

    if !saw_shortcut && !chrome.rows_blank {
        draw_empty_state(
            &mut out,
            w,
            h,
            content_x,
            content_w,
            &layout,
            dpi,
            t_fade,
        );
    }

    // Scroll affordance when more apps exist than the viewport.
    if t_fade > 0.5 && (layout.list_can_scroll_up || layout.list_can_scroll_down) {
        let more = layout.list_total.saturating_sub(
            crate::ui::radial_menu::LIST_VISIBLE_ROWS.min(layout.list_total),
        );
        let hint = if layout.list_can_scroll_up && layout.list_can_scroll_down {
            format!("滚轮查看 · 共 {} 个", layout.list_total)
        } else if layout.list_can_scroll_down {
            format!("↓ 滚轮查看更多 · 共 {} 个", layout.list_total)
        } else {
            format!("↑ 滚轮回到顶部 · 共 {} 个", layout.list_total)
        };
        let _ = more;
        let hy = dpi.s(
            layout.card_y + layout.card_h
                - crate::ui::radial_menu::CARD_MARGIN
                - crate::ui::radial_menu::SCROLL_HINT_H
                + 2.0,
        );
        blit_text(
            &mut out,
            w,
            h,
            &hint,
            content_x,
            hy,
            content_w.max(dpi.s(80.0)) as u32,
            dpi.px(11.0),
            with_alpha(TERTIARY, t_fade * 0.95),
        );
    }
}

fn blit_card_scaled_around_pet(
    dest: &mut [u8],
    dw: u32,
    dh: u32,
    card: &[u8],
    cw: u32,
    ch: u32,
    layout: &RadialLayout,
    dpi: Dpi,
    scale: f32,
    fade: f32,
) {
    if cw == 0 || ch == 0 || card.len() < (cw * ch * 4) as usize {
        return;
    }
    let scale = scale.clamp(0.5, 1.5);
    let fade = fade.clamp(0.0, 1.0);
    let pivot_x = dpi.s(layout.pet_x + layout.pet_w * 0.5);
    let pivot_y = dpi.s(layout.pet_y + layout.pet_h * 0.5);
    let pad = dpi.s(16.0);
    let sx0 = dpi.s(layout.card_x) - pad;
    let sy0 = dpi.s(layout.card_y) - pad;
    let sx1 = dpi.s(layout.card_x + layout.card_w) + pad;
    let sy1 = dpi.s(layout.card_y + layout.card_h) + pad;

    let map = |x: f32, y: f32| (pivot_x + (x - pivot_x) * scale, pivot_y + (y - pivot_y) * scale);
    let corners = [
        map(sx0, sy0),
        map(sx1, sy0),
        map(sx0, sy1),
        map(sx1, sy1),
    ];
    let min_x = corners
        .iter()
        .map(|c| c.0.floor() as i32)
        .min()
        .unwrap_or(0)
        .max(0);
    let min_y = corners
        .iter()
        .map(|c| c.1.floor() as i32)
        .min()
        .unwrap_or(0)
        .max(0);
    let max_x = corners
        .iter()
        .map(|c| c.0.ceil() as i32)
        .max()
        .unwrap_or(0)
        .min(dw as i32);
    let max_y = corners
        .iter()
        .map(|c| c.1.ceil() as i32)
        .max()
        .unwrap_or(0)
        .min(dh as i32);
    if max_x <= min_x || max_y <= min_y {
        return;
    }
    let inv = 1.0 / scale as f64;
    let px = pivot_x as f64;
    let py = pivot_y as f64;
    for dy in min_y..max_y {
        for dx in min_x..max_x {
            let sx = px + (dx as f64 + 0.5 - px) * inv;
            let sy = py + (dy as f64 + 0.5 - py) * inv;
            let mut c = sample_rgba_bilinear(card, cw, ch, sx, sy);
            if fade < 1.0 {
                c[3] = (c[3] as f32 * fade).round() as u8;
            }
            if c[3] > 0 {
                put(dest, dw, dx, dy, c);
            }
        }
    }
}

fn with_alpha(c: [u8; 4], a: f32) -> [u8; 4] {
    let mut o = c;
    o[3] = ((c[3] as f32) * a.clamp(0.0, 1.0)).round().clamp(0.0, 255.0) as u8;
    o
}

fn lerp_rgba(a: [u8; 4], b: [u8; 4], t: f32) -> [u8; 4] {
    let t = t.clamp(0.0, 1.0);
    [
        lerp(a[0] as f32, b[0] as f32, t).round() as u8,
        lerp(a[1] as f32, b[1] as f32, t).round() as u8,
        lerp(a[2] as f32, b[2] as f32, t).round() as u8,
        lerp(a[3] as f32, b[3] as f32, t).round() as u8,
    ]
}

fn draw_fetch_btn(
    out: &mut [u8],
    w: u32,
    h: u32,
    dpi: Dpi,
    x: f32,
    y: f32,
    bw: f32,
    bh: f32,
    hover_w: f32,
    press_w: f32,
    reveal: f32,
) {
    let radius = bh * 0.5;
    let fill = lerp_rgba(
        lerp_rgba([0xFF, 0xFF, 0xFF, 0x8C], WHITE, hover_w),
        [0xFF, 0xEC, 0xF2, 0xE0],
        press_w,
    );
    fill_rrect_aa(out, w, h, x, y, x + bw, y + bh, radius, with_alpha(fill, reveal));
    stroke_rrect_aa(
        out,
        w,
        h,
        x + 0.5,
        y + 0.5,
        x + bw - 0.5,
        y + bh - 0.5,
        radius,
        with_alpha(ACCENT_PINK, reveal * 0.85),
        1.4,
    );
    blit_text_centered(
        out,
        w,
        h,
        ADD_LABEL,
        x,
        y,
        bw,
        bh,
        dpi.px(14.0),
        with_alpha(PAW_INK, reveal),
        dpi,
    );
}

fn attach_side(layout: &RadialLayout) -> ExpandDir {
    let pcx = layout.pet_x + layout.pet_w * 0.5;
    let pcy = layout.pet_y + layout.pet_h * 0.5;
    let ccx = layout.card_x + layout.card_w * 0.5;
    let ccy = layout.card_y + layout.card_h * 0.5;
    let dx = ccx - pcx;
    let dy = ccy - pcy;
    if dx.abs() >= dy.abs() {
        if dx >= 0.0 {
            ExpandDir::Right
        } else {
            ExpandDir::Left
        }
    } else if dy >= 0.0 {
        ExpandDir::Down
    } else {
        ExpandDir::Up
    }
}

fn draw_card_tail(
    out: &mut [u8],
    w: u32,
    h: u32,
    dpi: Dpi,
    layout: &RadialLayout,
    fade: f32,
) {
    let fade = fade.clamp(0.0, 1.0);
    if fade < 0.02 {
        return;
    }
    let cream = with_alpha(CARD, fade);
    let edge = with_alpha(BORDER, fade);
    let len = dpi.s(14.0);
    let half = dpi.s(9.0);
    let (ax, ay, bx, by, tip_x, tip_y) = match attach_side(layout) {
        ExpandDir::Right => {
            let x = dpi.s(layout.card_x) + 1.0;
            let y = dpi.s(layout.card_y + layout.card_h * 0.62);
            (x, y - half, x, y + half, x - len, y + dpi.s(2.0))
        }
        ExpandDir::Left => {
            let x = dpi.s(layout.card_x + layout.card_w) - 1.0;
            let y = dpi.s(layout.card_y + layout.card_h * 0.62);
            (x, y - half, x, y + half, x + len, y + dpi.s(2.0))
        }
        ExpandDir::Down => {
            let x = dpi.s(layout.card_x + layout.card_w * 0.5);
            let y = dpi.s(layout.card_y) + 1.0;
            (x - half, y, x + half, y, x + dpi.s(2.0), y - len)
        }
        ExpandDir::Up => {
            let x = dpi.s(layout.card_x + layout.card_w * 0.5);
            let y = dpi.s(layout.card_y + layout.card_h) - 1.0;
            (x - half, y, x + half, y, x + dpi.s(2.0), y + len)
        }
    };
    fill_triangle(out, w, h, ax, ay, bx, by, tip_x, tip_y, cream);
    // Soft pink bloom at the tail root — the card “grows out of” the cat.
    fill_soft_disc(
        out,
        w,
        h,
        ((ax + bx) * 0.5) as i32,
        ((ay + by) * 0.5) as i32,
        dpi.s(10.0) as i32,
        0.85,
        with_alpha(ACCENT_PINK, fade * 0.22),
        with_alpha(ACCENT_PINK, 0.0),
    );
    let _ = edge;
}

/// Comic say-bubble next to the pet (or at `fallback_xy` logical px).
pub fn draw_say_bubble(
    out: &mut [u8],
    w: u32,
    h: u32,
    dpr: f32,
    layout: Option<&RadialLayout>,
    fallback_xy: Option<(f32, f32)>,
    line: &str,
    fade: f32,
) {
    let fade = fade.clamp(0.0, 1.0);
    if fade < 0.05 {
        return;
    }
    let dpi = Dpi::new(dpr);
    let max_w = dpi.su(200);
    let Some((tw, th, tbuf)) =
        rasterize_text(line, max_w, dpi.px(12.0), with_alpha(PAW_INK, fade))
    else {
        return;
    };
    let pad_x = dpi.s(10.0);
    let pad_y = dpi.s(7.0);
    let bw = tw as f32 + pad_x * 2.0;
    let bh = th as f32 + pad_y * 2.0;
    let tail_w = dpi.s(7.0);
    let (mut x, mut y, toward_card) = if let Some(layout) = layout {
        let toward_card =
            if layout.card_x + layout.card_w * 0.5 >= layout.pet_x + layout.pet_w * 0.5 {
                1.0
            } else {
                -1.0
            };
        // Sit beside the pet, not over the face: start past the silhouette
        // (plus a small gap for the tail). Vertically pin to the head band.
        let gap = dpi.s(8.0);
        let x = if toward_card > 0.0 {
            dpi.s(layout.pet_x + layout.pet_w) + gap
        } else {
            dpi.s(layout.pet_x) - bw - gap
        };
        (x, dpi.s(layout.pet_y + layout.pet_h * 0.06), toward_card)
    } else if let Some((lx, ly)) = fallback_xy {
        (dpi.s(lx), dpi.s(ly), 1.0)
    } else {
        (dpi.s(16.0), dpi.s(16.0), 1.0)
    };
    x = x.clamp(dpi.s(2.0) + tail_w, (w as f32 - bw - 2.0).max(0.0));
    y = y.clamp(dpi.s(2.0), (h as f32 - bh - 2.0).max(0.0));
    fill_rrect_aa(
        out,
        w,
        h,
        x + dpi.s(1.5),
        y + dpi.s(2.0),
        x + bw + dpi.s(1.5),
        y + bh + dpi.s(2.0),
        dpi.s(10.0),
        with_alpha(SHADOW_B, fade),
    );
    fill_rrect_aa(out, w, h, x, y, x + bw, y + bh, dpi.s(10.0), with_alpha(WHITE, fade));
    // Tail pointing at the cat (or up at the pause chip on the standalone card).
    let ty = y + bh * 0.62;
    let white = with_alpha(WHITE, fade);
    if toward_card > 0.0 {
        fill_triangle(
            out,
            w,
            h,
            x - tail_w,
            ty,
            x + 1.0,
            ty - dpi.s(6.0),
            x + 1.0,
            ty + dpi.s(6.0),
            white,
        );
    } else {
        fill_triangle(
            out,
            w,
            h,
            x + bw + tail_w,
            ty,
            x + bw - 1.0,
            ty - dpi.s(6.0),
            x + bw - 1.0,
            ty + dpi.s(6.0),
            white,
        );
    }
    stroke_rrect_aa(
        out,
        w,
        h,
        x + 0.5,
        y + 0.5,
        x + bw - 0.5,
        y + bh - 0.5,
        dpi.s(10.0),
        with_alpha(ACCENT_PINK, fade * 0.55),
        1.0,
    );
    blit(out, w, h, &tbuf, tw, th, (x + pad_x) as u32, (y + pad_y) as u32);
}

fn fill_triangle(
    out: &mut [u8],
    w: u32,
    h: u32,
    ax: f32,
    ay: f32,
    bx: f32,
    by: f32,
    cx: f32,
    cy: f32,
    color: [u8; 4],
) {
    let area = (bx - ax) * (cy - ay) - (cx - ax) * (by - ay);
    if area.abs() < 0.01 {
        return;
    }
    let min_x = ax.min(bx).min(cx).floor().max(0.0) as i32;
    let min_y = ay.min(by).min(cy).floor().max(0.0) as i32;
    let max_x = ax.max(bx).max(cx).ceil().min(w as f32) as i32;
    let max_y = ay.max(by).max(cy).ceil().min(h as f32) as i32;
    for py in min_y..max_y {
        for px in min_x..max_x {
            let x = px as f32 + 0.5;
            let y = py as f32 + 0.5;
            let w0 = ((bx - x) * (cy - y) - (cx - x) * (by - y)) / area;
            let w1 = ((cx - x) * (ay - y) - (ax - x) * (cy - y)) / area;
            let w2 = 1.0 - w0 - w1;
            if w0 >= -0.03 && w1 >= -0.03 && w2 >= -0.03 {
                let edge = w0.min(w1).min(w2);
                let a = (edge * 10.0 + 0.55).clamp(0.0, 1.0);
                if a > 0.0 {
                    let mut c = color;
                    c[3] = ((color[3] as f32) * a) as u8;
                    if c[3] > 0 {
                        put(out, w, px, py, c);
                    }
                }
            }
        }
    }
}

#[allow(dead_code)]
fn draw_primary_btn(
    out: &mut [u8],
    w: u32,
    h: u32,
    dpi: Dpi,
    x: f32,
    y: f32,
    bw: f32,
    bh: f32,
    radius: f32,
    label: &str,
    hover_w: f32,
    press_w: f32,
    reveal: f32,
) {
    // Flat solid primary — no drop-shadow underlay, no top sheen.
    // Those layered fills read as "two shadow bars" on slate buttons.
    let _ = dpi;
    let base = lerp_rgba(PRIMARY, PRIMARY_HOVER, hover_w);
    let btn = lerp_rgba(base, PRIMARY_PRESS, press_w);
    let btn = with_alpha(btn, reveal);
    fill_rrect_aa(out, w, h, x, y, x + bw, y + bh, radius, btn);
    // Hairline border slightly darker than fill for edge definition only.
    let border = with_alpha(PRIMARY_PRESS, reveal * 0.55);
    stroke_rrect_aa(
        out,
        w,
        h,
        x + 0.5,
        y + 0.5,
        x + bw - 0.5,
        y + bh - 0.5,
        radius,
        border,
        1.0,
    );
    blit_text_centered(
        out,
        w,
        h,
        label,
        x,
        y,
        bw,
        bh,
        dpi.px(14.5),
        with_alpha(WHITE, reveal),
        dpi,
    );
}

fn draw_settings_btn(
    out: &mut [u8],
    w: u32,
    h: u32,
    dpi: Dpi,
    x: f32,
    y: f32,
    bw: f32,
    bh: f32,
    hover_w: f32,
    press_w: f32,
    reveal: f32,
) {
    let _ = (dpi, press_w);
    let cx = x + bw * 0.5;
    let cy = y + bh * 0.5;
    let s = bw.min(bh);
    if hover_w > 0.02 {
        let blush = with_alpha([0xFF, 0xF4, 0xEE, 0xFF], reveal * hover_w * 0.55);
        fill_soft_disc(
            out,
            w,
            h,
            cx.round() as i32,
            cy.round() as i32,
            (s * 0.52).round() as i32,
            2.4,
            blush,
            with_alpha(blush, 0.0),
        );
    }
    if blit_settings_paw_asset(out, w, h, cx, cy, s, reveal) {
        return;
    }
    draw_paw_badge(out, w, h, cx, cy, s, reveal, hover_w);
}

fn settings_paw_asset() -> Option<&'static (u32, u32, Vec<u8>)> {
    static CELL: OnceLock<Option<(u32, u32, Vec<u8>)>> = OnceLock::new();
    CELL.get_or_init(load_settings_paw).as_ref()
}

fn load_settings_paw() -> Option<(u32, u32, Vec<u8>)> {
    for dir in settings_paw_dirs() {
        let path = dir.join("ui").join("settings_paw.png");
        let Ok(img) = image::open(&path) else {
            continue;
        };
        let rgba = img.to_rgba8();
        let (iw, ih) = rgba.dimensions();
        if iw == 0 || ih == 0 {
            continue;
        }
        return Some((iw, ih, rgba.into_raw()));
    }
    None
}

fn settings_paw_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            dirs.push(parent.join("assets"));
        }
    }
    dirs.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets"));
    dirs.push(PathBuf::from("assets"));
    dirs
}

fn blit_settings_paw_asset(
    out: &mut [u8],
    w: u32,
    h: u32,
    cx: f32,
    cy: f32,
    s: f32,
    reveal: f32,
) -> bool {
    let Some((sw, sh, src)) = settings_paw_asset() else {
        return false;
    };
    let dest = s.round().max(12.0) as u32;
    let (dw, dh, mut scaled) = scale_rgba_fit(src, *sw, *sh, dest, dest);
    let fade = reveal.clamp(0.0, 1.0);
    if fade < 0.999 {
        for px in scaled.chunks_exact_mut(4) {
            px[3] = (px[3] as f32 * fade).round() as u8;
        }
    }
    let dx = (cx - dw as f32 * 0.5).round() as u32;
    let dy = (cy - dh as f32 * 0.5).round() as u32;
    blit(out, w, h, &scaled, dw, dh, dx, dy);
    true
}

/// Fallback if the reference PNG is missing: cream plate + 4 toes + heart pad.
fn draw_paw_badge(
    out: &mut [u8],
    w: u32,
    h: u32,
    cx: f32,
    cy: f32,
    s: f32,
    reveal: f32,
    hover_w: f32,
) {
    let plate = with_alpha([0xF6, 0xF0, 0xEA, 0xFF], reveal);
    let plate_hi = with_alpha([0xFF, 0xFB, 0xF7, 0xFF], reveal);
    fill_soft_disc(
        out,
        w,
        h,
        cx.round() as i32,
        cy.round() as i32,
        (s * 0.48).round() as i32,
        1.6,
        plate_hi,
        plate,
    );

    let fur = with_alpha([0xFF, 0xF8, 0xF4, 0xFF], reveal);
    // Palm
    fill_soft_ellipse(
        out,
        w,
        h,
        cx.round() as i32,
        (cy + 0.06 * s).round() as i32,
        (0.28 * s).round().max(4.0) as i32,
        (0.24 * s).round().max(3.0) as i32,
        fur,
    );
    // Four white toe lobes
    let lobes = [
        (cx - 0.20 * s, cy - 0.10 * s, 0.12 * s),
        (cx - 0.08 * s, cy - 0.20 * s, 0.125 * s),
        (cx + 0.08 * s, cy - 0.20 * s, 0.125 * s),
        (cx + 0.20 * s, cy - 0.10 * s, 0.12 * s),
    ];
    for (lx, ly, r) in lobes {
        fill_soft_disc(
            out,
            w,
            h,
            lx.round() as i32,
            ly.round() as i32,
            r.round().max(3.0) as i32,
            1.2,
            fur,
            fur,
        );
    }

    let pink = with_alpha(
        lerp_rgba([0xFF, 0xB3, 0xC2, 0xFF], [0xFF, 0x9A, 0xB4, 0xFF], hover_w),
        reveal,
    );
    let toes = [
        (cx - 0.18 * s, cy - 0.08 * s, 0.070 * s, 0.088 * s),
        (cx - 0.07 * s, cy - 0.18 * s, 0.072 * s, 0.095 * s),
        (cx + 0.07 * s, cy - 0.18 * s, 0.072 * s, 0.095 * s),
        (cx + 0.18 * s, cy - 0.08 * s, 0.070 * s, 0.088 * s),
    ];
    for (tx, ty, rx, ry) in toes {
        fill_soft_ellipse(
            out,
            w,
            h,
            tx.round() as i32,
            ty.round() as i32,
            rx.round().max(2.0) as i32,
            ry.round().max(2.0) as i32,
            pink,
        );
        let shine = with_alpha(WHITE, reveal * 0.55);
        fill_soft_disc(
            out,
            w,
            h,
            (tx - rx * 0.25).round() as i32,
            (ty - ry * 0.28).round() as i32,
            (rx * 0.35).round().max(1.0) as i32,
            0.8,
            shine,
            with_alpha(shine, 0.0),
        );
    }

    // Heart-shaped main pad
    fill_soft_ellipse(
        out,
        w,
        h,
        cx.round() as i32,
        (cy + 0.10 * s).round() as i32,
        (0.16 * s).round().max(3.0) as i32,
        (0.14 * s).round().max(3.0) as i32,
        pink,
    );
    let lobe = (0.10 * s).round().max(2.0) as i32;
    fill_soft_disc(
        out,
        w,
        h,
        (cx - 0.06 * s).round() as i32,
        (cy + 0.04 * s).round() as i32,
        lobe,
        1.0,
        pink,
        pink,
    );
    fill_soft_disc(
        out,
        w,
        h,
        (cx + 0.06 * s).round() as i32,
        (cy + 0.04 * s).round() as i32,
        lobe,
        1.0,
        pink,
        pink,
    );
    let shine = with_alpha(WHITE, reveal * 0.50);
    fill_soft_disc(
        out,
        w,
        h,
        (cx - 0.05 * s).round() as i32,
        (cy + 0.02 * s).round() as i32,
        (0.055 * s).round().max(1.0) as i32,
        1.0,
        shine,
        with_alpha(shine, 0.0),
    );
}

fn draw_list_row(
    out: &mut [u8],
    w: u32,
    h: u32,
    dpi: Dpi,
    x: f32,
    y: f32,
    bw: f32,
    bh: f32,
    radius: f32,
    name: &str,
    valid: bool,
    icon: Option<&IconRgba>,
    hover_w: f32,
    press_w: f32,
    reveal: f32,
) {
    let bg = if !valid {
        lerp_rgba(INVALID_BG, INVALID_BG_HOVER, (hover_w + press_w).min(1.0))
    } else {
        lerp_rgba(
            lerp_rgba(GROUPED_BG, GROUPED_HOVER, hover_w),
            GROUPED_PRESS,
            press_w,
        )
    };
    let bg = with_alpha(bg, reveal);
    // Hover elevation
    if hover_w > 0.02 && valid {
        fill_rrect_aa(
            out,
            w,
            h,
            x,
            y + dpi.s(1.5),
            x + bw,
            y + bh + dpi.s(2.0),
            radius,
            with_alpha(SHADOW_A, reveal * hover_w),
        );
    }
    fill_rrect_aa(out, w, h, x, y, x + bw, y + bh, radius, bg);
    stroke_rrect_aa(
        out,
        w,
        h,
        x + 0.5,
        y + 0.5,
        x + bw - 0.5,
        y + bh - 0.5,
        radius,
        with_alpha(SOFT_BORDER, reveal * 0.85),
        1.0,
    );

    let icx = (x + dpi.s(22.0)) as i32;
    let icy = (y + bh * 0.5) as i32;
    // Shared slot for icons: round icons get a dark tile container, square
    // icons stand alone — both end up with the same visual footprint.
    let tile_r = dpi.s(13.0).round() as i32;
    let tile_corner = dpi.s(7.0).round().max(3.0) as i32;
    draw_app_icon(
        out,
        w,
        h,
        dpi,
        icx,
        icy,
        tile_r,
        tile_corner,
        name,
        valid,
        icon,
        reveal,
    );

    let max_tw = (bw - dpi.s(88.0)).max(8.0) as u32;
    // 15pt logical · dual-font + 2× SS (text.rs) for clean Latin names.
    if valid {
        if let Some((tw, th, tbuf)) =
            rasterize_text(name, max_tw, dpi.px(15.0), with_alpha(LABEL, reveal))
        {
            let ty = (y + (bh - th as f32) * 0.5).round().max(0.0) as u32;
            blit(out, w, h, &tbuf, tw, th, (x + dpi.s(42.0)) as u32, ty);
        }
    } else {
        if let Some((tw, th, tbuf)) =
            rasterize_text(name, max_tw, dpi.px(14.0), with_alpha(ORANGE, reveal))
        {
            let ty = (y + dpi.s(8.0)).round().max(0.0) as u32;
            blit(out, w, h, &tbuf, tw, th, (x + dpi.s(42.0)) as u32, ty);
        }
        if let Some((tw, th, tbuf)) = rasterize_text(
            "无法找到程序 · 点此修复",
            max_tw,
            dpi.px(11.0),
            with_alpha(ORANGE, reveal * 0.9),
        ) {
            let ty = (y + dpi.s(26.0)).round().max(0.0) as u32;
            blit(out, w, h, &tbuf, tw, th, (x + dpi.s(42.0)) as u32, ty);
        }
    }

    draw_chevron(
        out,
        w,
        (x + bw - dpi.s(16.0)) as i32,
        (y + bh * 0.5) as i32,
        dpi,
        with_alpha(if valid { PAW_KICKER } else { TERTIARY }, reveal),
    );
}

/// Lift factor for the dragged row (visual pop).
const GHOST_LIFT: f32 = 1.04;
/// Extra pixels around the pre-rendered ghost (none — the row has no drop shadow).
fn ghost_pad(_dpi: Dpi) -> (f32, f32) {
    (0.0, 0.0)
}

/// Pre-render every per-frame drag overlay (lifted row, delete bowl, hint) so
/// the drag hot path never touches GDI text or rescales bitmaps. Call once when
/// a drag begins; `present_menu_drag` then only blits the cached images.
pub fn prerender_drag_images(drag: &mut MenuDragChrome, dpr: f32) {
    let dpi = Dpi::new(dpr);

    // Lifted row only — no offset drop shadow under the card.
    let bw = dpi.s(drag.row_w) * GHOST_LIFT;
    let bh = dpi.s(drag.row_h) * GHOST_LIFT;
    let (pad_x, pad_y) = ghost_pad(dpi);
    let gw = (bw + pad_x * 2.0).ceil().max(1.0) as u32;
    let gh = (bh + pad_y * 2.0).ceil().max(1.0) as u32;
    let mut img = vec![0u8; (gw * gh * 4) as usize];
    draw_list_row(
        &mut img,
        gw,
        gh,
        dpi,
        pad_x,
        pad_y,
        bw,
        bh,
        dpi.s(14.0),
        &drag.name,
        drag.valid,
        drag.icon.as_deref(),
        1.0,
        0.0,
        1.0,
    );
    drag.ghost_img = Some((gw, gh, img));

    // Delete bowl at rest and at the 1.08× over-bowl size.
    if let Some((sw, sh, src)) = empty_bowl_asset() {
        let rest = dpi.s(BOWL_SIZE).round().max(24.0) as u32;
        let over = (dpi.s(BOWL_SIZE) * 1.08).round().max(24.0) as u32;
        let (ow, oh, img) = scale_rgba_fit(src, *sw, *sh, rest, rest);
        drag.bowl_img = Some((ow, oh, img));
        let (ow2, oh2, img2) = scale_rgba_fit(src, *sw, *sh, over, over);
        drag.bowl_over_img = Some((ow2, oh2, img2));
    }

    //「喂给我删除」glyphs in both states.
    if let Some((tw, th, t)) =
        rasterize_text(DELETE_HINT, dpi.su(140), dpi.px(11.0), with_alpha(PAW_INK, 1.0))
    {
        drag.hint_ink = Some((tw, th, t));
    }
    if let Some((tw, th, t)) =
        rasterize_text(DELETE_HINT, dpi.su(140), dpi.px(11.0), with_alpha(PAW_KICKER, 1.0))
    {
        drag.hint_kicker = Some((tw, th, t));
    }
}

fn draw_drag_ghost(
    out: &mut [u8],
    w: u32,
    h: u32,
    dpi: Dpi,
    drag: &MenuDragChrome,
    reveal: f32,
) {
    let bw = dpi.s(drag.row_w) * GHOST_LIFT;
    let bh = dpi.s(drag.row_h) * GHOST_LIFT;
    let x = dpi.s(drag.pointer_x - drag.grab_dx) - (bw - dpi.s(drag.row_w)) * 0.5;
    let y = dpi.s(drag.pointer_y - drag.grab_dy) - (bh - dpi.s(drag.row_h)) * 0.5;
    draw_list_row(
        out,
        w,
        h,
        dpi,
        x,
        y,
        bw,
        bh,
        dpi.s(14.0),
        &drag.name,
        drag.valid,
        drag.icon.as_deref(),
        1.0,
        0.0,
        reveal,
    );
}

/// Delete bowl +「喂给我删除」hint. Uses the pre-rendered state images when
/// available; the fallback path (missing PNG) re-rasterizes per frame.
fn draw_bowl_state(
    out: &mut [u8],
    w: u32,
    h: u32,
    dpi: Dpi,
    layout: &RadialLayout,
    drag: &MenuDragChrome,
    reveal: f32,
) {
    let (bx, by, bw, bh) = bowl_rect(
        layout.pet_x,
        layout.pet_y,
        layout.pet_w,
        layout.pet_h,
        layout.window_w as f32,
        layout.window_h as f32,
    );
    let scale = if drag.over_bowl { 1.08 } else { 1.0 };
    let cx = bx + bw * 0.5;
    let cy = by + bh * 0.5;
    let dw = bw * scale;
    let dh = bh * scale;
    let x = cx - dw * 0.5;
    let y = cy - dh * 0.5;

    let img = if drag.over_bowl {
        drag.bowl_over_img.as_ref()
    } else {
        drag.bowl_img.as_ref()
    };
    if let Some((iw, ih, src)) = img {
        let dx = (dpi.s(cx) - *iw as f32 * 0.5).round().max(0.0) as u32;
        let dy = (dpi.s(cy) - *ih as f32 * 0.5).round().max(0.0) as u32;
        blit(out, w, h, src, *iw, *ih, dx, dy);
    } else {
        blit_empty_bowl(out, w, h, dpi.s(x), dpi.s(y), dpi.s(dw), dpi.s(dh), reveal);
    }

    // Hint sits under the bowl so it never overlaps the pet silhouette.
    let bowl_dev = (dpi.s(x), dpi.s(y), dpi.s(dw), dpi.s(dh));
    let hint = if drag.over_bowl {
        drag.hint_kicker.as_ref()
    } else {
        drag.hint_ink.as_ref()
    };
    if let Some((tw, th, t)) = hint {
        blit_hint_glyphs(out, w, h, dpi, layout, bowl_dev, cx, cy, t, *tw, *th);
    } else {
        let color = with_alpha(if drag.over_bowl { PAW_KICKER } else { PAW_INK }, reveal);
        if let Some((tw, th, t)) =
            rasterize_text(DELETE_HINT, dpi.su(140), dpi.px(11.0), color)
        {
            blit_hint_glyphs(out, w, h, dpi, layout, bowl_dev, cx, cy, &t, tw, th);
        }
    }
}

/// Position and blit the delete hint under (or beside) the bowl.
fn blit_hint_glyphs(
    out: &mut [u8],
    w: u32,
    h: u32,
    dpi: Dpi,
    layout: &RadialLayout,
    bowl_dev: (f32, f32, f32, f32),
    cx: f32,
    cy: f32,
    t: &[u8],
    tw: u32,
    th: u32,
) {
    let (sx, sy, sdw, sdh) = bowl_dev;
    let mut tx = dpi.s(cx) - tw as f32 * 0.5;
    let mut ty = sy + sdh + dpi.s(2.0);
    if ty + th as f32 > h as f32 - 2.0 {
        // No room below: tuck to the side facing the dock card.
        let toward_card = if layout.card_x + layout.card_w * 0.5 >= layout.pet_x + layout.pet_w * 0.5
        {
            1.0
        } else {
            -1.0
        };
        tx = if toward_card > 0.0 {
            sx + sdw + dpi.s(4.0)
        } else {
            sx - tw as f32 - dpi.s(4.0)
        };
        ty = dpi.s(cy) - th as f32 * 0.5;
    }
    let tx = tx.round().clamp(2.0, (w as f32 - tw as f32 - 2.0).max(2.0)) as u32;
    let ty = ty.round().clamp(2.0, (h as f32 - th as f32 - 2.0).max(2.0)) as u32;
    blit(out, w, h, t, tw, th, tx, ty);
}

fn empty_bowl_asset() -> Option<&'static (u32, u32, Vec<u8>)> {
    static CELL: OnceLock<Option<(u32, u32, Vec<u8>)>> = OnceLock::new();
    CELL.get_or_init(load_empty_bowl).as_ref()
}

fn load_empty_bowl() -> Option<(u32, u32, Vec<u8>)> {
    for dir in settings_paw_dirs() {
        let path = dir.join("ui").join("empty_feed_bowl.png");
        let Ok(img) = image::open(&path) else {
            continue;
        };
        let rgba = img.to_rgba8();
        let (iw, ih) = rgba.dimensions();
        if iw == 0 || ih == 0 {
            continue;
        }
        return Some((iw, ih, rgba.into_raw()));
    }
    None
}

fn blit_empty_bowl(
    out: &mut [u8],
    w: u32,
    h: u32,
    x: f32,
    y: f32,
    dw: f32,
    dh: f32,
    reveal: f32,
) {
    let dest = dw.min(dh).round().max(24.0) as u32;
    if let Some((sw, sh, src)) = empty_bowl_asset() {
        let (ow, oh, mut scaled) = scale_rgba_fit(src, *sw, *sh, dest, dest);
        let fade = reveal.clamp(0.0, 1.0);
        if fade < 0.999 {
            for px in scaled.chunks_exact_mut(4) {
                px[3] = (px[3] as f32 * fade).round() as u8;
            }
        }
        let dx = (x + (dw - ow as f32) * 0.5).round().max(0.0) as u32;
        let dy = (y + (dh - oh as f32) * 0.5).round().max(0.0) as u32;
        blit(out, w, h, &scaled, ow, oh, dx, dy);
        return;
    }
    // Fallback: empty cream dish if the PNG is missing.
    fill_rrect_aa(
        out,
        w,
        h,
        x,
        y,
        x + dw,
        y + dh,
        dw.min(dh) * 0.45,
        with_alpha([0xF6, 0xEC, 0xD8, 0xFF], reveal),
    );
    stroke_rrect_aa(
        out,
        w,
        h,
        x + 0.5,
        y + 0.5,
        x + dw - 0.5,
        y + dh - 0.5,
        dw.min(dh) * 0.45,
        with_alpha(PAW_INK, reveal * 0.7),
        1.5,
    );
}

fn draw_recent_icon(
    out: &mut [u8],
    w: u32,
    h: u32,
    dpi: Dpi,
    x: f32,
    y: f32,
    bw: f32,
    bh: f32,
    name: &str,
    valid: bool,
    icon: Option<&IconRgba>,
    hover_w: f32,
    press_w: f32,
    reveal: f32,
) {
    let _ = press_w;
    let hover_scale = 1.0 + 0.06 * hover_w;
    if hover_w > 0.02 {
        fill_rrect_aa(
            out,
            w,
            h,
            x,
            y,
            x + bw,
            y + bh,
            dpi.s(12.0),
            with_alpha(GROUPED_HOVER, reveal * hover_w),
        );
    }
    let cx = (x + bw * 0.5) as i32;
    let cy = (y + bh * 0.5) as i32;
    let tile_r = (dpi.s(RECENT_ICON * 0.5) * hover_scale).round().max(8.0) as i32;
    let tile_corner = dpi.s(8.0).round().max(3.0) as i32;
    draw_app_icon(
        out,
        w,
        h,
        dpi,
        cx,
        cy,
        tile_r,
        tile_corner,
        name,
        valid,
        icon,
        reveal,
    );
}

/// Shared slot for list-row and frequent-strip icons.
fn draw_app_icon(
    out: &mut [u8],
    w: u32,
    h: u32,
    dpi: Dpi,
    icx: i32,
    icy: i32,
    tile_r: i32,
    tile_corner: i32,
    name: &str,
    valid: bool,
    icon: Option<&IconRgba>,
    reveal: f32,
) {
    let (disc, disc_d) = if valid {
        (PRIMARY, PRIMARY_HOVER)
    } else {
        (ORANGE, ORANGE)
    };

    if valid {
        if let Some(icon) = icon {
            match icon.shape {
                IconShape::Round => {
                    fill_soft_tile(
                        out,
                        w,
                        h,
                        icx,
                        icy,
                        tile_r,
                        tile_corner,
                        0.7,
                        with_alpha(disc, reveal),
                        with_alpha(disc_d, reveal),
                    );
                    let icon_d = (tile_r as f32 * 2.0 * 0.80).max(8.0) as u32;
                    let (iw, ih, scaled) =
                        scale_icon_rgba(&icon.rgba, icon.w, icon.h, icon_d, icon_d);
                    blit_icon_tile(out, w, &scaled, iw, ih, icx, icy, tile_r, tile_corner);
                }
                IconShape::Square => {
                    let icon_d = (tile_r as f32 * 2.0 * 0.92).max(8.0) as u32;
                    let (iw, ih, scaled) =
                        scale_icon_rgba(&icon.rgba, icon.w, icon.h, icon_d, icon_d);
                    let dx = (icx - iw as i32 / 2).max(0) as u32;
                    let dy = (icy - ih as i32 / 2).max(0) as u32;
                    blit(out, w, h, &scaled, iw, ih, dx, dy);
                }
            }
        } else {
            fill_soft_tile(
                out,
                w,
                h,
                icx,
                icy,
                tile_r,
                tile_corner,
                0.7,
                with_alpha(disc, reveal),
                with_alpha(disc_d, reveal),
            );
            let ch = name.chars().next().unwrap_or('A').to_string();
            if let Some((tw, th, tbuf)) =
                rasterize_text(&ch, dpi.su(24), dpi.px(12.5), with_alpha(WHITE, reveal))
            {
                let (tx, ty) = center_in_rect(
                    icx as f32 - tile_r as f32,
                    icy as f32 - tile_r as f32,
                    tile_r as f32 * 2.0,
                    tile_r as f32 * 2.0,
                    tw,
                    th,
                    dpi.s(0.5),
                );
                blit(out, w, h, &tbuf, tw, th, tx, ty);
            }
        }
    } else {
        fill_soft_tile(
            out,
            w,
            h,
            icx,
            icy,
            tile_r,
            tile_corner,
            0.7,
            with_alpha(disc, reveal),
            with_alpha(disc_d, reveal),
        );
        if let Some((tw, th, tbuf)) =
            rasterize_text("!", dpi.su(24), dpi.px(12.5), with_alpha(WHITE, reveal))
        {
            let (tx, ty) = center_in_rect(
                icx as f32 - tile_r as f32,
                icy as f32 - tile_r as f32,
                tile_r as f32 * 2.0,
                tile_r as f32 * 2.0,
                tw,
                th,
                dpi.s(0.5),
            );
            blit(out, w, h, &tbuf, tw, th, tx, ty);
        }
    }
}

/// Scale card + items around pivot; pet rect stays fixed (pin-pet).
fn scale_layout_from_pivot(
    layout: &RadialLayout,
    pivot_x: f32,
    pivot_y: f32,
    scale: f32,
) -> RadialLayout {
    let s = scale.max(0.01);
    let map = |x: f32, y: f32| (pivot_x + (x - pivot_x) * s, pivot_y + (y - pivot_y) * s);

    let (cx, cy) = map(layout.card_x, layout.card_y);
    // Size scales; top-left of scaled rect relative to pivot
    let card_w = layout.card_w * s;
    let card_h = layout.card_h * s;
    // Re-map card corners properly: scale center of card
    let ccx = layout.card_x + layout.card_w * 0.5;
    let ccy = layout.card_y + layout.card_h * 0.5;
    let (nccx, nccy) = map(ccx, ccy);
    let card_x = nccx - card_w * 0.5;
    let card_y = nccy - card_h * 0.5;

    let items = layout
        .items
        .iter()
        .map(|it| {
            let icx = it.x + it.w * 0.5;
            let icy = it.y + it.h * 0.5;
            let (nx, ny) = map(icx, icy);
            let nw = it.w * s;
            let nh = it.h * s;
            crate::ui::radial_menu::ItemGeom {
                entry: it.entry.clone(),
                x: nx - nw * 0.5,
                y: ny - nh * 0.5,
                w: nw,
                h: nh,
                cx: nx,
                cy: ny,
                radius: it.radius * s,
            }
        })
        .collect();

    let _ = cx;
    let _ = cy;
    let (list_top_x, list_top_y) = map(layout.card_x, layout.list_top);
    let (_, list_bot_y) = map(layout.card_x, layout.list_bottom);
    let _ = list_top_x;
    RadialLayout {
        items,
        open_t: layout.open_t,
        window: layout.window,
        window_w: layout.window_w,
        window_h: layout.window_h,
        pet_x: layout.pet_x,
        pet_y: layout.pet_y,
        pet_w: layout.pet_w,
        pet_h: layout.pet_h,
        card_x,
        card_y,
        card_w,
        card_h,
        list_total: layout.list_total,
        list_scroll: layout.list_scroll,
        list_can_scroll_up: layout.list_can_scroll_up,
        list_can_scroll_down: layout.list_can_scroll_down,
        list_top: list_top_y,
        list_bottom: list_bot_y,
        recent_label_y: map(layout.recent_box_x, layout.recent_label_y).1,
        recent_box_x: {
            let bcx = layout.recent_box_x + layout.recent_box_w * 0.5;
            let (nx, _) = map(bcx, layout.recent_box_y);
            nx - layout.recent_box_w * s * 0.5
        },
        recent_box_y: {
            let bcy = layout.recent_box_y + layout.recent_box_h * 0.5;
            let (_, ny) = map(layout.recent_box_x, bcy);
            ny - layout.recent_box_h * s * 0.5
        },
        recent_box_w: layout.recent_box_w * s,
        recent_box_h: layout.recent_box_h * s,
        list_label_y: map(layout.recent_box_x, layout.list_label_y).1,
    }
}

fn blit_text(
    out: &mut [u8],
    w: u32,
    h: u32,
    text: &str,
    x: f32,
    y: f32,
    max_w: u32,
    px: f32,
    color: [u8; 4],
) {
    if let Some((tw, th, t)) = rasterize_text(text, max_w, px, color) {
        blit(
            out,
            w,
            h,
            &t,
            tw,
            th,
            x.round().max(0.0) as u32,
            y.round().max(0.0) as u32,
        );
    }
}

fn blit_text_centered(
    out: &mut [u8],
    w: u32,
    h: u32,
    text: &str,
    x: f32,
    y: f32,
    bw: f32,
    bh: f32,
    px: f32,
    color: [u8; 4],
    dpi: Dpi,
) {
    let max_w = (bw - dpi.s(16.0)).max(8.0) as u32;
    if let Some((tw, th, t)) = rasterize_text(text, max_w, px, color) {
        let (tx, ty) = center_in_rect(x, y, bw, bh, tw, th, dpi.s(1.0));
        blit(out, w, h, &t, tw, th, tx, ty);
    }
}

fn draw_empty_state(
    out: &mut [u8],
    w: u32,
    h: u32,
    content_x: f32,
    content_w: f32,
    layout: &RadialLayout,
    dpi: Dpi,
    reveal: f32,
) {
    let reveal = reveal.clamp(0.0, 1.0);
    if reveal < 0.05 {
        return;
    }
    let ey = dpi.s(empty_y(layout) as f32) + (1.0 - reveal) * dpi.s(6.0);
    let gx = (content_x + content_w * 0.5) as i32;
    let gy = (ey + dpi.s(14.0)) as i32;
    let r = dpi.s(20.0).round() as i32;

    fill_soft_disc(
        out,
        w,
        h,
        gx,
        gy,
        r,
        0.9,
        with_alpha(FILL_OPAQUE, reveal),
        with_alpha(GROUPED_BG, reveal),
    );
    // Soft pink ring accent
    stroke_rrect_aa(
        out,
        w,
        h,
        gx as f32 - r as f32,
        gy as f32 - r as f32,
        gx as f32 + r as f32,
        gy as f32 + r as f32,
        r as f32,
        with_alpha(ACCENT_PINK, reveal * 0.45),
        1.5,
    );
    let t = dpi.s(1.5).max(1.0);
    let arm = dpi.s(6.0);
    fill_rect_f(
        out,
        w,
        h,
        gx as f32 - t * 0.5,
        gy as f32 - arm,
        t,
        arm * 2.0,
        with_alpha(SECONDARY, reveal),
    );
    fill_rect_f(
        out,
        w,
        h,
        gx as f32 - arm,
        gy as f32 - t * 0.5,
        arm * 2.0,
        t,
        with_alpha(SECONDARY, reveal),
    );

    if let Some((tw, th, tbuf)) =
        rasterize_text(EMPTY_TITLE, dpi.su(220), dpi.px(14.0), with_alpha(LABEL, reveal))
    {
        let tx = (content_x + (content_w - tw as f32) * 0.5).round().max(0.0) as u32;
        blit(out, w, h, &tbuf, tw, th, tx, (ey + dpi.s(44.0)) as u32);
    }
    if let Some((tw, th, tbuf)) = rasterize_text(
        EMPTY_HINT,
        dpi.su(300),
        dpi.px(12.5),
        with_alpha(SECONDARY, reveal),
    ) {
        let tx = (content_x + (content_w - tw as f32) * 0.5).round().max(0.0) as u32;
        blit(out, w, h, &tbuf, tw, th, tx, (ey + dpi.s(64.0)) as u32);
    }
}

fn content_x_from(layout: &RadialLayout) -> f32 {
    layout
        .items
        .iter()
        .find(|i| matches!(i.entry, MenuEntry::AddShortcut | MenuEntry::Shortcut { .. }))
        .map(|i| i.x)
        .unwrap_or(layout.card_x + 16.0)
}

fn content_w_from(layout: &RadialLayout) -> f32 {
    layout
        .items
        .iter()
        .find(|i| matches!(i.entry, MenuEntry::AddShortcut | MenuEntry::Shortcut { .. }))
        .map(|i| i.w)
        .unwrap_or((layout.card_w - 32.0).max(80.0))
}

fn empty_y(layout: &RadialLayout) -> u32 {
    layout.list_top.round().max(0.0) as u32
}

fn draw_avatar(
    out: &mut [u8],
    w: u32,
    dpi: Dpi,
    pet_x: f32,
    pet_y: f32,
    pet_w: f32,
    pet_h: f32,
    pet_rgba: &[u8],
    src_w: u32,
    src_h: u32,
) {
    if src_w == 0 || src_h == 0 || pet_rgba.len() < (src_w * src_h * 4) as usize {
        return;
    }
    // Signed dest: a settings size preview may sit partly outside the
    // (frozen) overlay; `put` clips instead of wrapping a negative u32.
    let px = dpi.s(pet_x).round() as i32;
    let py = dpi.s(pet_y).round() as i32;
    let dw = dpi.s(pet_w).round().max(1.0) as i32;
    let dh = dpi.s(pet_h).round().max(1.0) as i32;
    // Exact replica of the idle present (app::scale_rgba_centered, scale 1):
    // same aspect fit, same safety margins, bottom-aligned. Opening the
    // launcher must never move or resize the cat, so this keeps the pet
    // pixel-identical to its resting state.
    let fit = (dw as f64 / src_w as f64).min(dh as f64 / src_h as f64);
    let mut fw = (src_w as f64 * fit).round().max(1.0) as u32;
    let mut fh = (src_h as f64 * fit).round().max(1.0) as u32;
    let margin_x = 2u32.min(fw / 16);
    let margin_top = 2u32.min(fh / 16);
    let margin_bot = 4u32.min(fh / 12).max(3);
    fw = fw.saturating_sub(margin_x * 2).max(1);
    fh = fh.saturating_sub(margin_top + margin_bot).max(1);
    let ox = px + (dw - fw as i32) / 2;
    let oy = py + dh - fh as i32 - margin_bot as i32;
    let scale_x = src_w as f64 / fw as f64;
    let scale_y = src_h as f64 / fh as f64;
    for dy in 0..fh {
        for dx in 0..fw {
            let sx = (dx as f64 + 0.5) * scale_x - 0.5;
            let sy = (dy as f64 + 0.5) * scale_y - 0.5;
            let col = sample_rgba_bilinear(pet_rgba, src_w, src_h, sx, sy);
            put(out, w, ox + dx as i32, oy + dy as i32, col);
        }
    }
}

fn draw_chevron(out: &mut [u8], w: u32, cx: i32, cy: i32, dpi: Dpi, c: [u8; 4]) {
    let n = dpi.s(6.0).round().max(5.0) as i32;
    for i in 0..n {
        put(out, w, cx - 2 + i, cy - (n - 1) + i, c);
        put(out, w, cx - 2 + i, cy + (n - 1) - i, c);
        put(out, w, cx - 1 + i, cy - (n - 1) + i, c);
        put(out, w, cx - 1 + i, cy + (n - 1) - i, c);
    }
}

// ── Settings / Manager (M5: reminder + shortcuts) ─────────────────────────

pub const SETTINGS_W: u32 = 420;
pub const SETTINGS_H: u32 = 320;
const REMINDER_CARD_TOP: f32 = 72.0;
const REMINDER_CARD_H: f32 = 120.0;
const PET_CARD_TOP: f32 = 204.0;
const PET_CARD_H: f32 = 72.0;

#[derive(Debug, Clone, Copy)]
pub enum SettingsHit {
    /// Top-right 「完成」: commit the pending pet-size preview and close.
    Done,
    ToggleEnabled,
    IntervalDec,
    IntervalInc,
    TogglePause,
    PetScaleDec,
    PetScaleInc,
}

/// `reminder`: (enabled, interval_minutes, paused)
/// `pet_scale`: relative size vs 128px baseline (e.g. 0.6)
pub fn compose_settings_frame(
    reminder: (bool, u32, bool),
    pet_scale: f32,
    dpr: f32,
    say: Option<&str>,
) -> (u32, u32, Vec<u8>) {
    let (enabled, interval_min, _paused) = reminder;
    let dpi = Dpi::new(dpr);
    let w = dpi.su(SETTINGS_W);
    let h = dpi.su(SETTINGS_H);
    let mut out = vec![0u8; (w * h * 4) as usize];
    let wf = w as f32;
    let hf = h as f32;

    fill_rrect_aa(
        &mut out,
        w,
        h,
        dpi.s(8.0),
        dpi.s(12.0),
        wf - dpi.s(6.0),
        hf - dpi.s(6.0),
        dpi.s(26.0),
        SHADOW_B,
    );
    fill_rrect_aa(&mut out, w, h, 0.0, 0.0, wf, hf, dpi.s(18.0), CARD);
    stroke_rrect_aa(
        &mut out,
        w,
        h,
        0.5,
        0.5,
        wf - 0.5,
        hf - 0.5,
        dpi.s(18.0),
        BORDER,
        1.0,
    );

    blit_text(
        &mut out,
        w,
        h,
        "设置",
        dpi.s(24.0),
        dpi.s(20.0),
        dpi.su(120),
        dpi.px(20.0),
        LABEL,
    );
    blit_text(
        &mut out,
        w,
        h,
        "提醒与外观",
        dpi.s(24.0),
        dpi.s(46.0),
        dpi.su(240),
        dpi.px(13.5),
        SECONDARY,
    );

    if let Some((tw, th, t)) = rasterize_text("完成", dpi.su(56), dpi.px(16.0), BLUE) {
        blit(
            &mut out,
            w,
            h,
            &t,
            tw,
            th,
            w - tw - dpi.su(24),
            dpi.s(24.0) as u32,
        );
    }

    // ── Reminder card ──
    let rc_top = dpi.s(REMINDER_CARD_TOP);
    let rc_h = dpi.s(REMINDER_CARD_H);
    fill_rrect_aa(
        &mut out,
        w,
        h,
        dpi.s(20.0),
        rc_top,
        wf - dpi.s(20.0),
        rc_top + rc_h,
        dpi.s(12.0),
        GROUPED_BG,
    );
    stroke_rrect_aa(
        &mut out,
        w,
        h,
        dpi.s(20.0) + 0.5,
        rc_top + 0.5,
        wf - dpi.s(20.0) - 0.5,
        rc_top + rc_h - 0.5,
        dpi.s(12.0),
        SOFT_BORDER,
        1.0,
    );
    blit_text(
        &mut out,
        w,
        h,
        "健康提醒",
        dpi.s(36.0),
        dpi.s(REMINDER_CARD_TOP + 12.0),
        dpi.su(160),
        dpi.px(13.5),
        SECONDARY,
    );

    // Enable row
    let en_mark = if enabled { "●  启用提醒" } else { "○  启用提醒" };
    let en_color = if enabled { BLUE } else { LABEL };
    blit_text(
        &mut out,
        w,
        h,
        en_mark,
        dpi.s(36.0),
        dpi.s(REMINDER_CARD_TOP + 36.0),
        dpi.su(200),
        dpi.px(15.0),
        en_color,
    );

    // Interval steppers
    blit_text(
        &mut out,
        w,
        h,
        "间隔",
        dpi.s(36.0),
        dpi.s(REMINDER_CARD_TOP + 68.0),
        dpi.su(48),
        dpi.px(14.0),
        LABEL,
    );
    // [−] chip
    fill_rrect_aa(
        &mut out,
        w,
        h,
        dpi.s(100.0),
        dpi.s(REMINDER_CARD_TOP + 64.0),
        dpi.s(132.0),
        dpi.s(REMINDER_CARD_TOP + 92.0),
        dpi.s(8.0),
        FILL_OPAQUE,
    );
    blit_text_centered(
        &mut out,
        w,
        h,
        "−",
        dpi.s(100.0),
        dpi.s(REMINDER_CARD_TOP + 64.0),
        dpi.s(32.0),
        dpi.s(28.0),
        dpi.px(18.0),
        LABEL,
        dpi,
    );
    let interval_label = format!("{interval_min} 分钟");
    blit_text(
        &mut out,
        w,
        h,
        &interval_label,
        dpi.s(144.0),
        dpi.s(REMINDER_CARD_TOP + 70.0),
        dpi.su(100),
        dpi.px(15.0),
        LABEL,
    );
    // [+] chip
    fill_rrect_aa(
        &mut out,
        w,
        h,
        dpi.s(248.0),
        dpi.s(REMINDER_CARD_TOP + 64.0),
        dpi.s(280.0),
        dpi.s(REMINDER_CARD_TOP + 92.0),
        dpi.s(8.0),
        FILL_OPAQUE,
    );
    blit_text_centered(
        &mut out,
        w,
        h,
        "+",
        dpi.s(248.0),
        dpi.s(REMINDER_CARD_TOP + 64.0),
        dpi.s(32.0),
        dpi.s(28.0),
        dpi.px(18.0),
        LABEL,
        dpi,
    );

    let status = if !enabled { "已关闭" } else { "运行中" };
    blit_text(
        &mut out,
        w,
        h,
        status,
        dpi.s(300.0),
        dpi.s(REMINDER_CARD_TOP + 70.0),
        dpi.su(90),
        dpi.px(13.5),
        if !enabled { ORANGE } else { BLUE },
    );
    // Pause chip — kept as a button, no longer toggles a real pause.
    let pause_label = "暂停";
    fill_rrect_aa(
        &mut out,
        w,
        h,
        dpi.s(320.0),
        dpi.s(REMINDER_CARD_TOP + 32.0),
        dpi.s(388.0),
        dpi.s(REMINDER_CARD_TOP + 58.0),
        dpi.s(8.0),
        FILL_OPAQUE,
    );
    blit_text_centered(
        &mut out,
        w,
        h,
        pause_label,
        dpi.s(320.0),
        dpi.s(REMINDER_CARD_TOP + 32.0),
        dpi.s(68.0),
        dpi.s(26.0),
        dpi.px(13.5),
        BLUE,
        dpi,
    );

    // ── Pet size card ──
    let pc_top = dpi.s(PET_CARD_TOP);
    let pc_h = dpi.s(PET_CARD_H);
    fill_rrect_aa(
        &mut out,
        w,
        h,
        dpi.s(20.0),
        pc_top,
        wf - dpi.s(20.0),
        pc_top + pc_h,
        dpi.s(12.0),
        GROUPED_BG,
    );
    stroke_rrect_aa(
        &mut out,
        w,
        h,
        dpi.s(20.0) + 0.5,
        pc_top + 0.5,
        wf - dpi.s(20.0) - 0.5,
        pc_top + pc_h - 0.5,
        dpi.s(12.0),
        SOFT_BORDER,
        1.0,
    );
    blit_text(
        &mut out,
        w,
        h,
        "宠物大小",
        dpi.s(36.0),
        dpi.s(PET_CARD_TOP + 12.0),
        dpi.su(160),
        dpi.px(13.5),
        SECONDARY,
    );
    // [−]
    fill_rrect_aa(
        &mut out,
        w,
        h,
        dpi.s(36.0),
        dpi.s(PET_CARD_TOP + 36.0),
        dpi.s(68.0),
        dpi.s(PET_CARD_TOP + 64.0),
        dpi.s(8.0),
        FILL_OPAQUE,
    );
    blit_text_centered(
        &mut out,
        w,
        h,
        "−",
        dpi.s(36.0),
        dpi.s(PET_CARD_TOP + 36.0),
        dpi.s(32.0),
        dpi.s(28.0),
        dpi.px(18.0),
        LABEL,
        dpi,
    );
    let pct = ((pet_scale * 100.0).round() as i32).clamp(50, 150);
    let scale_label = format!("{pct}%");
    blit_text(
        &mut out,
        w,
        h,
        &scale_label,
        dpi.s(88.0),
        dpi.s(PET_CARD_TOP + 42.0),
        dpi.su(72),
        dpi.px(16.0),
        LABEL,
    );
    // [+]
    fill_rrect_aa(
        &mut out,
        w,
        h,
        dpi.s(168.0),
        dpi.s(PET_CARD_TOP + 36.0),
        dpi.s(200.0),
        dpi.s(PET_CARD_TOP + 64.0),
        dpi.s(8.0),
        FILL_OPAQUE,
    );
    blit_text_centered(
        &mut out,
        w,
        h,
        "+",
        dpi.s(168.0),
        dpi.s(PET_CARD_TOP + 36.0),
        dpi.s(32.0),
        dpi.s(28.0),
        dpi.px(18.0),
        LABEL,
        dpi,
    );
    blit_text(
        &mut out,
        w,
        h,
        "相对默认尺寸 · 托盘也可调",
        dpi.s(220.0),
        dpi.s(PET_CARD_TOP + 44.0),
        dpi.su(180),
        dpi.px(12.5),
        TERTIARY,
    );

    if let Some(line) = say {
        draw_say_bubble(
            &mut out,
            w,
            h,
            dpr,
            None,
            Some((20.0, REMINDER_CARD_TOP + 100.0)),
            line,
            1.0,
        );
    }

    (w, h, out)
}

/// Compact settings laid out for the launcher card (same size as the dock).
#[derive(Debug, Clone, Copy)]
pub struct SettingsCardMetrics {
    pub w: f32,
    pub h: f32,
    pub reminder_y: f32,
    pub enable_y0: f32,
    pub enable_y1: f32,
    pub interval_dec: (f32, f32, f32, f32),
    pub interval_inc: (f32, f32, f32, f32),
    pub pause: (f32, f32, f32, f32),
    /// Top of the pet-size group card (styled like the reminder card).
    pub pet_y: f32,
    pub pet_dec: (f32, f32, f32, f32),
    pub pet_inc: (f32, f32, f32, f32),
}

pub fn settings_card_metrics(card_w: f32, card_h: f32) -> SettingsCardMetrics {
    let w = card_w.max(200.0);
    let h = card_h.max(240.0);
    let reminder_y = 46.0;
    let reminder_h = 86.0;
    let pet_y = reminder_y + reminder_h + 6.0;
    SettingsCardMetrics {
        w,
        h,
        reminder_y,
        enable_y0: reminder_y + 28.0,
        enable_y1: reminder_y + 50.0,
        interval_dec: (16.0, reminder_y + 52.0, 44.0, 26.0),
        interval_inc: (112.0, reminder_y + 52.0, 44.0, 26.0),
        pause: (w - 86.0, reminder_y + 28.0, 70.0, 24.0),
        pet_y,
        pet_dec: (16.0, pet_y + 36.0, 44.0, 26.0),
        pet_inc: (112.0, pet_y + 36.0, 44.0, 26.0),
    }
}

pub fn compose_settings_card(
    reminder: (bool, u32, bool),
    pet_scale: f32,
    dpr: f32,
    card_w: f32,
    card_h: f32,
) -> (u32, u32, Vec<u8>) {
    let (enabled, interval_min, _paused) = reminder;
    let m = settings_card_metrics(card_w, card_h);
    let dpi = Dpi::new(dpr);
    let w = dpi.su(m.w.round() as u32);
    let h = dpi.su(m.h.round() as u32);
    let mut out = vec![0u8; (w * h * 4) as usize];
    let wf = w as f32;
    let hf = h as f32;
    let _ = hf;

    fill_rrect_aa(
        &mut out,
        w,
        h,
        dpi.s(8.0),
        dpi.s(10.0),
        wf - dpi.s(4.0),
        hf - dpi.s(4.0),
        dpi.s(22.0),
        with_alpha(SHADOW_B, 0.9),
    );
    fill_rrect_aa(&mut out, w, h, 0.0, 0.0, wf, hf, dpi.s(22.0), CARD);
    stroke_rrect_aa(
        &mut out,
        w,
        h,
        0.5,
        0.5,
        wf - 0.5,
        hf - 0.5,
        dpi.s(22.0),
        BORDER,
        1.0,
    );

    blit_text(
        &mut out,
        w,
        h,
        "设置",
        dpi.s(16.0),
        dpi.s(12.0),
        dpi.su(120),
        dpi.px(17.0),
        LABEL,
    );
    if let Some((tw, th, t)) = rasterize_text("完成", dpi.su(56), dpi.px(14.0), BLUE) {
        blit(
            &mut out,
            w,
            h,
            &t,
            tw,
            th,
            w.saturating_sub(tw + dpi.su(16)),
            dpi.s(14.0) as u32,
        );
    }

    let rx0 = dpi.s(12.0);
    let rx1 = wf - dpi.s(12.0);
    fill_rrect_aa(
        &mut out,
        w,
        h,
        rx0,
        dpi.s(m.reminder_y),
        rx1,
        dpi.s(m.reminder_y + 86.0),
        dpi.s(12.0),
        GROUPED_BG,
    );
    stroke_rrect_aa(
        &mut out,
        w,
        h,
        rx0 + 0.5,
        dpi.s(m.reminder_y) + 0.5,
        rx1 - 0.5,
        dpi.s(m.reminder_y + 86.0) - 0.5,
        dpi.s(12.0),
        SOFT_BORDER,
        1.0,
    );
    blit_text(
        &mut out,
        w,
        h,
        "健康提醒",
        dpi.s(20.0),
        dpi.s(m.reminder_y + 6.0),
        dpi.su(140),
        dpi.px(12.0),
        SECONDARY,
    );
    let en_mark = if enabled { "●  启用" } else { "○  启用" };
    blit_text(
        &mut out,
        w,
        h,
        en_mark,
        dpi.s(20.0),
        dpi.s(m.enable_y0),
        dpi.su(120),
        dpi.px(13.5),
        if enabled { BLUE } else { LABEL },
    );
    let (px, py, pw, ph) = m.pause;
    fill_rrect_aa(
        &mut out,
        w,
        h,
        dpi.s(px),
        dpi.s(py),
        dpi.s(px + pw),
        dpi.s(py + ph),
        dpi.s(8.0),
        FILL_OPAQUE,
    );
    blit_text_centered(
        &mut out,
        w,
        h,
        "暂停",
        dpi.s(px),
        dpi.s(py),
        dpi.s(pw),
        dpi.s(ph),
        dpi.px(12.0),
        SECONDARY,
        dpi,
    );

    let (dx, dy, dw, dh) = m.interval_dec;
    fill_rrect_aa(
        &mut out,
        w,
        h,
        dpi.s(dx),
        dpi.s(dy),
        dpi.s(dx + dw),
        dpi.s(dy + dh),
        dpi.s(8.0),
        FILL_OPAQUE,
    );
    blit_text_centered(
        &mut out,
        w,
        h,
        "−",
        dpi.s(dx),
        dpi.s(dy),
        dpi.s(dw),
        dpi.s(dh),
        dpi.px(16.0),
        LABEL,
        dpi,
    );
    let interval_label = format!("{interval_min} 分");
    blit_text_centered(
        &mut out,
        w,
        h,
        &interval_label,
        dpi.s(dx),
        dpi.s(dy),
        dpi.s(m.interval_inc.0 + m.interval_inc.2 - dx),
        dpi.s(dh),
        dpi.px(13.0),
        LABEL,
        dpi,
    );
    let (ix, iy, iw, ih) = m.interval_inc;
    fill_rrect_aa(
        &mut out,
        w,
        h,
        dpi.s(ix),
        dpi.s(iy),
        dpi.s(ix + iw),
        dpi.s(iy + ih),
        dpi.s(8.0),
        FILL_OPAQUE,
    );
    blit_text_centered(
        &mut out,
        w,
        h,
        "+",
        dpi.s(ix),
        dpi.s(iy),
        dpi.s(iw),
        dpi.s(ih),
        dpi.px(16.0),
        LABEL,
        dpi,
    );

    // ── Pet size group card, styled like the reminder card above ──
    fill_rrect_aa(
        &mut out,
        w,
        h,
        rx0,
        dpi.s(m.pet_y),
        rx1,
        dpi.s(m.pet_y + 72.0),
        dpi.s(12.0),
        GROUPED_BG,
    );
    stroke_rrect_aa(
        &mut out,
        w,
        h,
        rx0 + 0.5,
        dpi.s(m.pet_y) + 0.5,
        rx1 - 0.5,
        dpi.s(m.pet_y + 72.0) - 0.5,
        dpi.s(12.0),
        SOFT_BORDER,
        1.0,
    );
    blit_text(
        &mut out,
        w,
        h,
        "宠物大小",
        dpi.s(20.0),
        dpi.s(m.pet_y + 6.0),
        dpi.su(140),
        dpi.px(12.0),
        SECONDARY,
    );

    let (pdx, pdy, pdw, pdh) = m.pet_dec;
    fill_rrect_aa(
        &mut out,
        w,
        h,
        dpi.s(pdx),
        dpi.s(pdy),
        dpi.s(pdx + pdw),
        dpi.s(pdy + pdh),
        dpi.s(8.0),
        FILL_OPAQUE,
    );
    blit_text_centered(
        &mut out,
        w,
        h,
        "−",
        dpi.s(pdx),
        dpi.s(pdy),
        dpi.s(pdw),
        dpi.s(pdh),
        dpi.px(16.0),
        LABEL,
        dpi,
    );
    let pct = ((pet_scale * 100.0).round() as i32).clamp(50, 150);
    blit_text_centered(
        &mut out,
        w,
        h,
        &format!("{pct}%"),
        dpi.s(pdx),
        dpi.s(pdy),
        dpi.s(m.pet_inc.0 + m.pet_inc.2 - pdx),
        dpi.s(pdh),
        dpi.px(14.0),
        LABEL,
        dpi,
    );
    let (pix, piy, piw, pih) = m.pet_inc;
    fill_rrect_aa(
        &mut out,
        w,
        h,
        dpi.s(pix),
        dpi.s(piy),
        dpi.s(pix + piw),
        dpi.s(piy + pih),
        dpi.s(8.0),
        FILL_OPAQUE,
    );
    blit_text_centered(
        &mut out,
        w,
        h,
        "+",
        dpi.s(pix),
        dpi.s(piy),
        dpi.s(piw),
        dpi.s(pih),
        dpi.px(16.0),
        LABEL,
        dpi,
    );
    // Hint, same wording/style as the standalone settings window.
    blit_text(
        &mut out,
        w,
        h,
        "相对默认尺寸 · 托盘也可调",
        dpi.s(pix + piw + 8.0),
        dpi.s(piy + 8.0),
        dpi.su(190),
        dpi.px(11.5),
        TERTIARY,
    );

    (w, h, out)
}

pub fn hit_settings_card(
    local_x: f32,
    local_y: f32,
    card_w: f32,
    card_h: f32,
) -> Option<SettingsHit> {
    let m = settings_card_metrics(card_w, card_h);
    if local_x >= m.w - 64.0 && local_y <= 42.0 {
        return Some(SettingsHit::Done);
    }
    if (16.0..=160.0).contains(&local_x) && (m.enable_y0..=m.enable_y1).contains(&local_y) {
        return Some(SettingsHit::ToggleEnabled);
    }
    if in_rect(local_x, local_y, m.pause) {
        return Some(SettingsHit::TogglePause);
    }
    if in_rect(local_x, local_y, m.interval_dec) {
        return Some(SettingsHit::IntervalDec);
    }
    if in_rect(local_x, local_y, m.interval_inc) {
        return Some(SettingsHit::IntervalInc);
    }
    if in_rect(local_x, local_y, m.pet_dec) {
        return Some(SettingsHit::PetScaleDec);
    }
    if in_rect(local_x, local_y, m.pet_inc) {
        return Some(SettingsHit::PetScaleInc);
    }
    None
}

fn in_rect(x: f32, y: f32, r: (f32, f32, f32, f32)) -> bool {
    x >= r.0 && x <= r.0 + r.2 && y >= r.1 && y <= r.1 + r.3
}

#[cfg(test)]
mod settings_card {
    use super::*;

    #[test]
    fn compact_card_fits_launcher() {
        let m = settings_card_metrics(360.0, 360.0);
        assert!(m.pet_inc.1 + m.pet_inc.3 < 360.0);
        let (w, h, out) = compose_settings_card((true, 45, false), 1.0, 1.0, 360.0, 360.0);
        assert_eq!(out.len(), (w * h * 4) as usize);
        assert!(w > 200 && h > 200);
        assert!(matches!(
            hit_settings_card(330.0, 20.0, 360.0, 360.0),
            Some(SettingsHit::Done)
        ));
        assert!(hit_settings_card(180.0, 300.0, 360.0, 360.0).is_none());
    }

    #[test]
    fn pause_chip_stays_a_button() {
        let m = settings_card_metrics(360.0, 360.0);
        assert!(matches!(
            hit_settings_card(m.pause.0 + 8.0, m.pause.1 + 8.0, 360.0, 360.0),
            Some(SettingsHit::TogglePause)
        ));
        // Even if the leftover paused flag is true, the chip is still 「暂停」
        // (same label as the running state) — no 「已暂停」/「恢复」.
        let (w, h, running) = compose_settings_card((true, 45, false), 1.0, 1.0, 360.0, 360.0);
        let (_, _, leftover) = compose_settings_card((true, 45, true), 1.0, 1.0, 360.0, 360.0);
        assert_eq!(running.len(), leftover.len());
        let _ = (w, h);
    }

    #[test]
    fn say_no_pause_bubble_paints() {
        if !cfg!(windows) {
            return;
        }
        let mut buf = vec![0u8; 400 * 200 * 4];
        draw_say_bubble(
            &mut buf,
            400,
            200,
            1.0,
            None,
            Some((20.0, 20.0)),
            SAY_NO_PAUSE,
            1.0,
        );
        let inked = buf.chunks_exact(4).filter(|p| p[3] > 20).count();
        assert!(inked > 80, "refusal bubble must paint, inked={inked}");
    }

    #[test]
    fn pet_size_section_matches_reminder_style() {
        let (w, _h, out) = compose_settings_card((true, 45, false), 1.0, 1.0, 360.0, 360.0);
        let m = settings_card_metrics(360.0, 360.0);
        let rgba = |x: u32, y: u32| -> [u8; 4] {
            let i = ((y * w + x) * 4) as usize;
            [out[i], out[i + 1], out[i + 2], out[i + 3]]
        };
        // Both sections sit on the identical grouped-card background (no text
        // or controls at the sample points) → styles are aligned.
        let reminder_bg = rgba(300, (m.reminder_y + 60.0) as u32);
        let pet_bg = rgba(300, (m.pet_y + 66.0) as u32);
        assert_eq!(pet_bg, reminder_bg, "pet group card must match reminder style");
        // The + chip is clickable at its new row position.
        assert!(matches!(
            hit_settings_card(m.pet_inc.0 + 10.0, m.pet_inc.1 + 10.0, 360.0, 360.0),
            Some(SettingsHit::PetScaleInc)
        ));
        // Hint text renders right of the + chip.
        let hx0 = (m.pet_inc.0 + m.pet_inc.2 + 8.0) as u32;
        let hy0 = (m.pet_inc.1 + 6.0) as u32;
        let mut inked = 0usize;
        for y in hy0..hy0 + 18 {
            for x in hx0..hx0 + 190 {
                if out[((y * w + x) * 4 + 3) as usize] > 0 {
                    inked += 1;
                }
            }
        }
        assert!(inked > 40, "pet-size hint text must render, inked={inked}");
    }

    #[test]
    fn standalone_done_hit_region() {
        // Top-right 「完成」 commits; card-control rows do not.
        assert!(matches!(hit_settings(400.0, 20.0), Some(SettingsHit::Done)));
        assert!(hit_settings(60.0, 60.0).is_none());
    }

    #[test]
    fn settings_top_corners_have_no_ghost_contour() {
        let (w, _h, out) = compose_settings_card((true, 45, false), 1.0, 2.0, 360.0, 360.0);
        let g = |x: u32, y: u32| out[((y * w + x) * 4 + 1) as usize] as i32;
        // No highlight/sheen ring in the transparent wedge outside the r=22 arc.
        assert_eq!(out[3], 0, "pixel (0,0) is outside the rounded card");
        assert_eq!(g(2, 2), 0, "outside the top-left arc");
        assert_eq!(g(w - 3, 2), 0, "outside the top-right arc");
        // Top band is the card fill, not a second inset highlight.
        let mid = g(w / 2, 16);
        assert_eq!(mid, CARD[1] as i32, "top band must not carry a sheen ring");
    }

    #[test]
    #[ignore]
    fn dump_settings_card_preview() {
        let (w, h, out) = compose_settings_card((true, 45, false), 1.0, 2.0, 360.0, 360.0);
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/_settings_card.png");
        image::save_buffer(&path, &out, w, h, image::ColorType::Rgba8).expect("write settings card");
        assert!(path.is_file());
    }
}

pub fn hit_settings(local_x: f32, local_y: f32) -> Option<SettingsHit> {
    // local coords are logical (caller maps physical → logical)
    let w = SETTINGS_W as f32;
    if local_x >= w - 72.0 && local_y <= 52.0 {
        return Some(SettingsHit::Done);
    }
    // Enable toggle row
    if (24.0..=300.0).contains(&local_x)
        && (REMINDER_CARD_TOP + 30.0..=REMINDER_CARD_TOP + 58.0).contains(&local_y)
    {
        return Some(SettingsHit::ToggleEnabled);
    }
    // Pause chip
    if (316.0..=392.0).contains(&local_x)
        && (REMINDER_CARD_TOP + 30.0..=REMINDER_CARD_TOP + 60.0).contains(&local_y)
    {
        return Some(SettingsHit::TogglePause);
    }
    // Interval −
    if (96.0..=136.0).contains(&local_x)
        && (REMINDER_CARD_TOP + 62.0..=REMINDER_CARD_TOP + 94.0).contains(&local_y)
    {
        return Some(SettingsHit::IntervalDec);
    }
    // Interval +
    if (244.0..=284.0).contains(&local_x)
        && (REMINDER_CARD_TOP + 62.0..=REMINDER_CARD_TOP + 94.0).contains(&local_y)
    {
        return Some(SettingsHit::IntervalInc);
    }
    // Pet scale −
    if (32.0..=72.0).contains(&local_x)
        && (PET_CARD_TOP + 34.0..=PET_CARD_TOP + 66.0).contains(&local_y)
    {
        return Some(SettingsHit::PetScaleDec);
    }
    // Pet scale +
    if (164.0..=204.0).contains(&local_x)
        && (PET_CARD_TOP + 34.0..=PET_CARD_TOP + 66.0).contains(&local_y)
    {
        return Some(SettingsHit::PetScaleInc);
    }
    None
}

// ── Drawing primitives (AA SDF) ───────────────────────────────────────────

fn fill_rrect_aa(
    out: &mut [u8],
    w: u32,
    h: u32,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    radius: f32,
    c: [u8; 4],
) {
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let r = radius
        .min((x1 - x0) * 0.5)
        .min((y1 - y0) * 0.5)
        .max(0.0);
    let min_x = x0.floor().max(0.0) as i32;
    let min_y = y0.floor().max(0.0) as i32;
    let max_x = x1.ceil().min(w as f32) as i32;
    let max_y = y1.ceil().min(h as f32) as i32;
    for py in min_y..max_y {
        for px in min_x..max_x {
            let d = sd_round_rect(px as f32 + 0.5, py as f32 + 0.5, x0, y0, x1, y1, r);
            // Slightly tighter AA (~0.6px) for crisper corners on HiDPI
            let a = (0.6 - d).clamp(0.0, 1.0);
            if a <= 0.0 {
                continue;
            }
            let mut col = c;
            col[3] = ((c[3] as f32) * a) as u8;
            if col[3] > 0 {
                put(out, w, px, py, col);
            }
        }
    }
}

/// Top highlight clipped to the card outline with a soft vertical fade, so the
/// highlight follows the card's rounded corners and fades out before its own
/// bottom edge (no hard contour line on the corner arcs).
fn fill_top_sheen(
    out: &mut [u8],
    w: u32,
    h: u32,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    radius: f32,
    inset: f32,
    height: f32,
    c: [u8; 4],
) {
    let r = radius
        .min((x1 - x0) * 0.5)
        .min((y1 - y0) * 0.5)
        .max(0.0);
    if height <= 0.0 {
        return;
    }
    let top = y0 + inset;
    let bottom = top + height;
    let min_x = (x0 + inset - 1.0).floor().max(0.0) as i32;
    let min_y = top.floor().max(0.0) as i32;
    let max_x = (x1 - inset + 1.0).ceil().min(w as f32) as i32;
    let max_y = bottom.ceil().min(h as f32) as i32;
    for py in min_y..max_y {
        let t = ((py as f32 + 0.5) - top) / height;
        let ydim = {
            let d = 1.0 - t.clamp(0.0, 1.0);
            d * d
        };
        if ydim <= 0.0 {
            continue;
        }
        for px in min_x..max_x {
            let cx = px as f32 + 0.5;
            let cy = py as f32 + 0.5;
            // Clip to the card outline: coverage uses the same AA curve as the
            // card fill, so the highlight never escapes the rounded corners.
            let d = sd_round_rect(cx, cy, x0, y0, x1, y1, r);
            let cov = (0.6 - d).clamp(0.0, 1.0);
            if cov <= 0.0 {
                continue;
            }
            let a = c[3] as f32 * ydim * cov;
            if a <= 0.5 {
                continue;
            }
            let mut col = c;
            col[3] = a.round() as u8;
            put(out, w, px, py, col);
        }
    }
}

fn stroke_rrect_aa(
    out: &mut [u8],
    w: u32,
    h: u32,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    radius: f32,
    c: [u8; 4],
    thickness: f32,
) {
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let r = radius
        .min((x1 - x0) * 0.5)
        .min((y1 - y0) * 0.5)
        .max(0.0);
    let half = thickness * 0.5;
    let min_x = (x0 - 1.0).floor().max(0.0) as i32;
    let min_y = (y0 - 1.0).floor().max(0.0) as i32;
    let max_x = (x1 + 1.0).ceil().min(w as f32) as i32;
    let max_y = (y1 + 1.0).ceil().min(h as f32) as i32;
    for py in min_y..max_y {
        for px in min_x..max_x {
            let d = sd_round_rect(px as f32 + 0.5, py as f32 + 0.5, x0, y0, x1, y1, r).abs();
            let a = (half + 0.5 - d).clamp(0.0, 1.0);
            if a <= 0.0 {
                continue;
            }
            let mut col = c;
            col[3] = ((c[3] as f32) * a) as u8;
            if col[3] > 0 {
                put(out, w, px, py, col);
            }
        }
    }
}

fn sd_round_rect(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> f32 {
    let cx = (x0 + x1) * 0.5;
    let cy = (y0 + y1) * 0.5;
    let hx = (x1 - x0) * 0.5;
    let hy = (y1 - y0) * 0.5;
    let dx = (px - cx).abs() - (hx - r);
    let dy = (py - cy).abs() - (hy - r);
    let ax = dx.max(0.0);
    let ay = dy.max(0.0);
    (ax * ax + ay * ay).sqrt() + dx.min(0.0).max(dy.min(0.0)) - r
}

fn fill_soft_disc(
    out: &mut [u8],
    w: u32,
    _h: u32,
    cx: i32,
    cy: i32,
    radius: i32,
    feather: f32,
    inner: [u8; 4],
    outer: [u8; 4],
) {
    let r = radius as f32;
    let extent = (radius as f32 + feather + 1.0) as i32;
    for y in (cy - extent)..=(cy + extent) {
        for x in (cx - extent)..=(cx + extent) {
            if x < 0 || y < 0 || x >= w as i32 {
                continue;
            }
            let dx = x as f32 - cx as f32;
            let dy = y as f32 - cy as f32;
            let d = (dx * dx + dy * dy).sqrt();
            if d > r + feather {
                continue;
            }
            let edge = if d <= r - 0.5 {
                1.0
            } else {
                (1.0 - (d - (r - 0.5)) / (feather + 0.5)).clamp(0.0, 1.0)
            };
            let shade = ((dy / r.max(1.0)) * 0.28 + 0.72).clamp(0.0, 1.0);
            let mut c = [0u8; 4];
            for k in 0..3 {
                c[k] = (inner[k] as f32 * (1.0 - shade) + outer[k] as f32 * shade) as u8;
            }
            c[3] = (inner[3] as f32 * edge) as u8;
            put(out, w, x, y, c);
        }
    }
}

/// Rounded-square tile with the same soft feather + top-light gradient as
/// `fill_soft_disc`, matching the square shape of app icons.
fn fill_soft_tile(
    out: &mut [u8],
    w: u32,
    _h: u32,
    cx: i32,
    cy: i32,
    half: i32,
    corner: i32,
    feather: f32,
    inner: [u8; 4],
    outer: [u8; 4],
) {
    let hx = half as f32;
    let hr = corner.max(1) as f32;
    let extent = (half as f32 + feather + 1.0) as i32;
    for y in (cy - extent)..=(cy + extent) {
        for x in (cx - extent)..=(cx + extent) {
            if x < 0 || y < 0 || x >= w as i32 {
                continue;
            }
            let px = x as f32 - cx as f32;
            let py = y as f32 - cy as f32;
            // Signed distance to the rounded rect (negative inside).
            let qx = px.abs() - (hx - hr);
            let qy = py.abs() - (hx - hr);
            let d = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt()
                + qx.max(qy).min(0.0)
                - hr;
            if d > feather {
                continue;
            }
            let edge = if d <= -0.5 {
                1.0
            } else {
                (1.0 - (d + 0.5) / (feather + 0.5)).clamp(0.0, 1.0)
            };
            let shade = ((py / hx.max(1.0)) * 0.28 + 0.72).clamp(0.0, 1.0);
            let mut c = [0u8; 4];
            for k in 0..3 {
                c[k] = (inner[k] as f32 * (1.0 - shade) + outer[k] as f32 * shade) as u8;
            }
            c[3] = (inner[3] as f32 * edge) as u8;
            put(out, w, x, y, c);
        }
    }
}

fn fill_soft_ellipse(
    out: &mut [u8],
    w: u32,
    _h: u32,
    cx: i32,
    cy: i32,
    rx: i32,
    ry: i32,
    c: [u8; 4],
) {
    let rx = rx.max(1) as f32;
    let ry = ry.max(1) as f32;
    let ex = rx as i32 + 2;
    let ey = ry as i32 + 2;
    for y in (cy - ey)..=(cy + ey) {
        for x in (cx - ex)..=(cx + ex) {
            if x < 0 || y < 0 || x >= w as i32 {
                continue;
            }
            let nx = (x as f32 - cx as f32) / rx;
            let ny = (y as f32 - cy as f32) / ry;
            let d = (nx * nx + ny * ny).sqrt();
            if d > 1.15 {
                continue;
            }
            let edge = if d <= 0.7 {
                1.0
            } else {
                (1.0 - (d - 0.7) / 0.45).clamp(0.0, 1.0)
            };
            let mut col = c;
            col[3] = (c[3] as f32 * edge) as u8;
            put(out, w, x, y, col);
        }
    }
}

fn fill_rect_f(out: &mut [u8], w: u32, h: u32, x: f32, y: f32, rw: f32, rh: f32, c: [u8; 4]) {
    let x0 = x.round().max(0.0) as u32;
    let y0 = y.round().max(0.0) as u32;
    let x1 = (x + rw).round().min(w as f32) as u32;
    let y1 = (y + rh).round().min(h as f32) as u32;
    for py in y0..y1 {
        for px in x0..x1 {
            put(out, w, px as i32, py as i32, c);
        }
    }
    let _ = h;
}

fn put(px: &mut [u8], w: u32, x: i32, y: i32, c: [u8; 4]) {
    if x < 0 || y < 0 || x >= w as i32 {
        return;
    }
    let h = (px.len() / 4) as u32 / w.max(1);
    if y >= h as i32 {
        return;
    }
    let i = ((y as u32 * w + x as u32) * 4) as usize;
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

fn blit(dest: &mut [u8], dw: u32, dh: u32, src: &[u8], sw: u32, sh: u32, dx: u32, dy: u32) {
    for y in 0..sh {
        for x in 0..sw {
            let tx = dx + x;
            let ty = dy + y;
            if tx >= dw || ty >= dh {
                continue;
            }
            let si = ((y * sw + x) * 4) as usize;
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

/// Alpha-composite one RGBA layer onto another at an integer offset.
///
/// Exposed so app-level overlay transitions can merge independently composed
/// launcher / settings buffers into one atomic layered-window frame.
pub fn blit_rgba(
    dest: &mut [u8],
    dw: u32,
    dh: u32,
    src: &[u8],
    sw: u32,
    sh: u32,
    dx: u32,
    dy: u32,
) {
    blit(dest, dw, dh, src, sw, sh, dx, dy);
}

/// Blit `src` so its (0,0) lands at `(dest_x, dest_y)`, clipped to `clip`.
pub fn blit_rgba_clipped(
    dest: &mut [u8],
    dw: u32,
    dh: u32,
    src: &[u8],
    sw: u32,
    sh: u32,
    dest_x: i32,
    dest_y: i32,
    clip: (i32, i32, i32, i32),
) {
    let (cx, cy, cw, ch) = clip;
    let clip_x1 = cx + cw;
    let clip_y1 = cy + ch;
    for y in 0..sh {
        let ty = dest_y + y as i32;
        if ty < cy || ty >= clip_y1 || ty < 0 || ty >= dh as i32 {
            continue;
        }
        for x in 0..sw {
            let tx = dest_x + x as i32;
            if tx < cx || tx >= clip_x1 || tx < 0 || tx >= dw as i32 {
                continue;
            }
            let si = ((y * sw + x) * 4) as usize;
            if si + 3 >= src.len() {
                continue;
            }
            let c = [src[si], src[si + 1], src[si + 2], src[si + 3]];
            if c[3] == 0 {
                continue;
            }
            put(dest, dw, tx, ty, c);
        }
    }
}

fn scale_rgba_fit(
    src: &[u8],
    sw: u32,
    sh: u32,
    max_w: u32,
    max_h: u32,
) -> (u32, u32, Vec<u8>) {
    if sw == 0 || sh == 0 || src.len() < (sw * sh * 4) as usize {
        return (1, 1, vec![0, 0, 0, 0]);
    }
    // Allow upscale so pet looks sharp on HiDPI plates
    let fit = (max_w as f32 / sw as f32).min(max_h as f32 / sh as f32);
    let dw = ((sw as f32) * fit).round().max(1.0) as u32;
    let dh = ((sh as f32) * fit).round().max(1.0) as u32;
    let mut out = vec![0u8; (dw * dh * 4) as usize];
    for y in 0..dh {
        for x in 0..dw {
            let fx = (x as f32 + 0.5) * sw as f32 / dw as f32 - 0.5;
            let fy = (y as f32 + 0.5) * sh as f32 / dh as f32 - 0.5;
            let x0 = fx.floor() as i32;
            let y0 = fy.floor() as i32;
            let tx = fx - x0 as f32;
            let ty = fy - y0 as f32;
            let mut acc = [0.0f32; 4];
            for (oy, wy) in [(0, 1.0 - ty), (1, ty)] {
                for (ox, wx) in [(0, 1.0 - tx), (1, tx)] {
                    let sx = (x0 + ox).clamp(0, sw as i32 - 1) as u32;
                    let sy = (y0 + oy).clamp(0, sh as i32 - 1) as u32;
                    let si = ((sy * sw + sx) * 4) as usize;
                    let wgt = wx * wy;
                    for k in 0..4 {
                        acc[k] += src[si + k] as f32 * wgt;
                    }
                }
            }
            let di = ((y * dw + x) * 4) as usize;
            for k in 0..4 {
                out[di + k] = acc[k].round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    (dw, dh, out)
}

/// Blit `src` centered on (cx, cy), clipped to a rounded-square tile
/// of half-size `half` and corner radius `corner`.
fn blit_icon_tile(
    out: &mut [u8],
    w: u32,
    src: &[u8],
    sw: u32,
    sh: u32,
    cx: i32,
    cy: i32,
    half: i32,
    corner: i32,
) {
    if sw == 0 || sh == 0 || src.len() < (sw * sh * 4) as usize {
        return;
    }
    let x0 = cx - half;
    let y0 = cy - half;
    let x1 = cx + half;
    let y1 = cy + half;
    let r = corner.max(1) as f32;
    let ox = cx - sw as i32 / 2;
    let oy = cy - sh as i32 / 2;
    for y in 0..sh as i32 {
        for x in 0..sw as i32 {
            let px = (ox + x) as f32;
            let py = (oy + y) as f32;
            // Rounded-rect containment: clamp to inner rect, test corner circle.
            let cxp = px.clamp(x0 as f32 + r, x1 as f32 - r);
            let cyp = py.clamp(y0 as f32 + r, y1 as f32 - r);
            let dx = px - cxp;
            let dy = py - cyp;
            if dx * dx + dy * dy > r * r {
                continue;
            }
            let si = ((y as u32 * sw + x as u32) * 4) as usize;
            let c = [src[si], src[si + 1], src[si + 2], src[si + 3]];
            if c[3] == 0 {
                continue;
            }
            put(out, w, ox + x, oy + y, c);
        }
    }
}

#[cfg(test)]
mod playful_dock {
    use super::*;
    use crate::ui::radial_menu::{layout_pinned, ExpandDir, MenuEntry};
    use uuid::Uuid;

    fn dummy_pet() -> (u32, u32, Vec<u8>) {
        let (w, h) = (32u32, 32u32);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for px in rgba.chunks_exact_mut(4) {
            px.copy_from_slice(&[0xF4, 0xF0, 0xEC, 0xFF]);
        }
        (w, h, rgba)
    }

    #[test]
    fn compose_drag_paints_bowl_and_ghost() {
        let entries = vec![
            MenuEntry::AddShortcut,
            MenuEntry::Manage,
            MenuEntry::Shortcut {
                id: Uuid::nil(),
                name: "Chrome".into(),
                valid: true,
                icon: None,
            },
        ];
        let lay = layout_pinned(
            &entries,
            500,
            480,
            (8.0, 80.0, 128.0, 128.0),
            (150.0, 20.0, 360.0, 360.0),
            ExpandDir::Right,
            1.0,
        );
        let (pw, ph, pet) = dummy_pet();
        let (w, h, out) = compose_menu_frame(
            &pet,
            pw,
            ph,
            &lay,
            1.0,
            MenuChromeState {
                drag: Some(MenuDragChrome {
                    id: Uuid::nil(),
                    name: "Chrome".into(),
                    valid: true,
                    icon: None,
                    pointer_x: 220.0,
                    pointer_y: 240.0,
                    grab_dx: 40.0,
                    grab_dy: 16.0,
                    from: 0,
                    insert_at: 0,
                    over_bowl: true,
                    row_w: 300.0,
                    row_h: 42.0,
                    ghost_img: None,
                    bowl_img: None,
                    bowl_over_img: None,
                    hint_ink: None,
                    hint_kicker: None,
                }),
                ..MenuChromeState::default()
            },
        );
        assert_eq!(out.len(), (w * h * 4) as usize);
        let opaque = out.chunks_exact(4).filter(|p| p[3] > 8).count();
        assert!(opaque > 200, "drag overlay should paint, got {opaque}");
    }

    #[test]
    fn compose_open_frame_is_nonempty() {
        let entries = vec![
            MenuEntry::AddShortcut,
            MenuEntry::Manage,
            MenuEntry::Shortcut {
                id: Uuid::nil(),
                name: "Chrome".into(),
                valid: true,
                icon: None,
            },
        ];
        let lay = layout_pinned(
            &entries,
            500,
            480,
            (8.0, 80.0, 128.0, 128.0),
            (150.0, 20.0, 360.0, 360.0),
            ExpandDir::Right,
            1.0,
        );
        let (pw, ph, pet) = dummy_pet();
        let (w, h, out) = compose_menu_frame(
            &pet,
            pw,
            ph,
            &lay,
            1.0,
            MenuChromeState {
                say: Some(SAY_LAUNCH),
                ..MenuChromeState::default()
            },
        );
        assert!(w > 100 && h > 100);
        assert_eq!(out.len(), (w * h * 4) as usize);
        let opaque = out.chunks_exact(4).filter(|p| p[3] > 8).count();
        assert!(opaque > 200, "card + tail + pet should paint, got {opaque}");
    }

    #[test]
    fn compose_grows_from_pet_not_card_center() {
        let entries = vec![MenuEntry::AddShortcut, MenuEntry::Manage];
        let mut lay = layout_pinned(
            &entries,
            500,
            480,
            (8.0, 160.0, 128.0, 128.0),
            (160.0, 20.0, 360.0, 360.0),
            ExpandDir::Right,
            0.0,
        );
        lay.open_t = 0.0;
        let closed_x = {
            let scale = MENU_GROW_FROM;
            let px = lay.pet_x + lay.pet_w * 0.5;
            px + (lay.card_x - px) * scale
        };
        lay.open_t = 1.0;
        let open_x = lay.card_x;
        assert!(
            closed_x < open_x,
            "at t=0 the card must sit closer to the pet ({closed_x} < {open_x})"
        );
    }

    #[test]
    fn cached_present_paints_card_and_pet() {
        let entries = vec![MenuEntry::AddShortcut, MenuEntry::Manage];
        let lay = layout_pinned(
            &entries,
            500,
            480,
            (8.0, 160.0, 128.0, 128.0),
            (160.0, 20.0, 360.0, 360.0),
            ExpandDir::Right,
            1.0,
        );
        let (cw, ch, card) = compose_menu_card_layer(&lay, 1.0, MenuChromeState::default());
        let (pw, ph, pet) = dummy_pet();
        let mut out = vec![0u8; (cw * ch * 4) as usize];
        present_menu_cached(
            &mut out,
            cw,
            ch,
            &card,
            cw,
            ch,
            &pet,
            pw,
            ph,
            &lay,
            1.0,
            MENU_GROW_FROM,
            0.6,
        );
        let opaque = out.chunks_exact(4).filter(|p| p[3] > 8).count();
        assert!(opaque > 100, "scaled card + pet should paint, got {opaque}");
    }

    fn drag_chrome(name: &str) -> MenuDragChrome {
        MenuDragChrome {
            id: Uuid::from_u128(3),
            name: name.into(),
            valid: true,
            icon: None,
            pointer_x: 220.0,
            pointer_y: 240.0,
            grab_dx: 40.0,
            grab_dy: 16.0,
            from: 2,
            insert_at: 1,
            over_bowl: false,
            row_w: 300.0,
            row_h: 42.0,
            ghost_img: None,
            bowl_img: None,
            bowl_over_img: None,
            hint_ink: None,
            hint_kicker: None,
        }
    }

    fn three_shortcut_layout() -> RadialLayout {
        let entries = vec![
            MenuEntry::AddShortcut,
            MenuEntry::Manage,
            MenuEntry::Shortcut {
                id: Uuid::from_u128(1),
                name: "App0".into(),
                valid: true,
                icon: None,
            },
            MenuEntry::Shortcut {
                id: Uuid::from_u128(2),
                name: "App1".into(),
                valid: true,
                icon: None,
            },
            MenuEntry::Shortcut {
                id: Uuid::from_u128(3),
                name: "App2".into(),
                valid: true,
                icon: None,
            },
        ];
        layout_pinned(
            &entries,
            500,
            480,
            (8.0, 80.0, 128.0, 128.0),
            (150.0, 20.0, 360.0, 360.0),
            ExpandDir::Right,
            1.0,
        )
    }

    #[test]
    fn say_bubble_stays_off_the_pet_face() {
        if !cfg!(windows) {
            return;
        }
        let lay = three_shortcut_layout();
        let w = 500u32;
        let h = 480u32;
        let mut buf = vec![0u8; (w * h * 4) as usize];
        draw_say_bubble(&mut buf, w, h, 1.0, Some(&lay), None, SAY_NO_PAUSE, 1.0);
        // Face lives in the upper-center of the pet slot; a bubble starting
        // at 0.55·pet_w used to paint here.
        let x0 = lay.pet_x as u32;
        let x1 = (lay.pet_x + lay.pet_w * 0.72) as u32;
        let y0 = lay.pet_y as u32;
        let y1 = (lay.pet_y + lay.pet_h * 0.55) as u32;
        let mut face_ink = 0usize;
        for y in y0..y1 {
            for x in x0..x1 {
                if buf[((y * w + x) * 4 + 3) as usize] > 20 {
                    face_ink += 1;
                }
            }
        }
        assert_eq!(face_ink, 0, "say bubble must not cover the pet face, ink={face_ink}");
        let total = buf.chunks_exact(4).filter(|p| p[3] > 20).count();
        assert!(total > 80, "bubble must still paint beside the pet");
    }

    #[test]
    fn menu_pet_geometry_matches_idle_present() {
        let lay = three_shortcut_layout();
        let (pw, ph, pet) = dummy_pet();
        let (w1, h1, frame) = compose_menu_frame(
            &pet,
            pw,
            ph,
            &lay,
            1.0,
            MenuChromeState::default(),
        );
        let (w2, h2, pet_only) = compose_menu_pet_only(&pet, pw, ph, &lay, 1.0);
        assert_eq!((w1, h1), (w2, h2));
        let alpha = |buf: &[u8], w: u32, x: u32, y: u32| buf[((y * w + x) * 4 + 3) as usize];
        // Pet rect (8,80,128,128) at dpr=1, 32×32 source: the idle-present
        // replica draws a 124×122 box at (10,82) — inside opaque, outside
        // transparent (no glass tray, exact same geometry as the rest state).
        assert!(alpha(&frame, w1, 60, 120) > 0, "pet must be inked");
        assert!(alpha(&frame, w1, 10, 82) > 0, "box top-left");
        assert!(alpha(&frame, w1, 133, 203) > 0, "box bottom-right");
        assert_eq!(alpha(&frame, w1, 9, 82), 0, "nothing left of the box");
        assert_eq!(alpha(&frame, w1, 10, 81), 0, "nothing above the box");
        assert_eq!(alpha(&frame, w1, 134, 82), 0, "nothing right of the box");
        assert_eq!(alpha(&frame, w1, 10, 204), 0, "nothing below the box");
        // The pet area must match the settings-view layer pixel for pixel.
        for y in 80..208 {
            for x in 8..136 {
                let fi = ((y * w1 + x) * 4) as usize;
                let pi = ((y * w2 + x) * 4) as usize;
                assert_eq!(
                    &frame[fi..fi + 4],
                    &pet_only[pi..pi + 4],
                    "pet pixel mismatch at ({x},{y})"
                );
            }
        }
    }

    #[test]
    fn pet_preview_scales_from_top_left_anchor() {
        let lay = three_shortcut_layout(); // pet rect (8, 80, 128, 128)
        let (pw, ph, pet) = dummy_pet();
        let (w, _h, base) = compose_menu_pet_preview(&pet, pw, ph, &lay, 1.0, 1.0);
        let (_w2, _h2, big) = compose_menu_pet_preview(&pet, pw, ph, &lay, 1.0, 1.5);
        let (_w3, _h3, small) = compose_menu_pet_preview(&pet, pw, ph, &lay, 1.0, 0.5);
        let ink = |buf: &[u8]| buf.chunks_exact(4).filter(|p| p[3] > 0).count();
        assert!(ink(&big) > ink(&base), "bigger ratio must draw more pet pixels");
        assert!(ink(&base) > ink(&small), "smaller ratio must draw fewer pet pixels");
        let any_ink = |buf: &[u8], y: u32| {
            (0..w).any(|x| buf[((y * w + x) * 4 + 3) as usize] > 0)
        };
        // Top-left anchored like the real window resize: top edge stays put,
        // the pet extends right/down (feet drop) as it grows.
        let top_row = |buf: &[u8]| {
            (0..480u32).find(|&y| any_ink(buf, y)).unwrap_or(u32::MAX)
        };
        let bottom_row = |buf: &[u8]| {
            (0..480u32).rev().find(|&y| any_ink(buf, y)).unwrap_or(0)
        };
        let t_base = top_row(&base);
        assert_eq!(top_row(&big), t_base, "top edge must not move when growing");
        assert_eq!(top_row(&small), t_base, "top edge must not move when shrinking");
        assert!(
            bottom_row(&big) > bottom_row(&base) && bottom_row(&base) > bottom_row(&small),
            "bottom edge must follow the growing pet (window top-left fixed)"
        );
    }

    #[test]
    fn prerender_drag_images_fills_expected_sizes() {
        let mut drag = drag_chrome("App2");
        prerender_drag_images(&mut drag, 2.0);

        // Ghost: row 300×42 lifted 1.04×, no shadow pad, at 2× dpr.
        let (gw, gh, img) = drag.ghost_img.as_ref().expect("ghost must pre-render");
        assert!((*gw as f32 - (300.0 * 1.04 * 2.0)).abs() <= 2.0, "gw={gw}");
        assert!((*gh as f32 - (42.0 * 1.04 * 2.0)).abs() <= 2.0, "gh={gh}");
        assert_eq!(img.len(), (*gw * *gh * 4) as usize);
        let opaque = img.chunks_exact(4).filter(|p| p[3] > 8).count();
        assert!(opaque > 100, "ghost row painted, got {opaque}");

        // Bowl from assets/ui/empty_feed_bowl.png: rest 96, over 104 at 2× dpr.
        if let Some((ow, oh, bowl)) = &drag.bowl_img {
            assert!(*ow <= 96 && *oh <= 96 && *ow > 0 && *oh > 0);
            assert!(bowl.chunks_exact(4).any(|p| p[3] > 8), "bowl painted");
        }
        if let Some((ow, oh, _)) = &drag.bowl_over_img {
            assert!(*ow <= 104 && *oh <= 104 && *ow > 0 && *oh > 0);
        }

        // Glyphs only exist where GDI text works (Windows).
        if cfg!(windows) {
            for (label, hint) in [
                ("ink", &drag.hint_ink),
                ("kicker", &drag.hint_kicker),
            ] {
                let (tw, th, t) = hint.as_ref().unwrap_or_else(|| panic!("{label} hint missing"));
                assert!(*tw > 0 && *th > 0 && t.len() == (*tw * *th * 4) as usize);
            }
        }
    }

    #[test]
    fn drag_slot_y_shifts_around_insert_slot() {
        let lay = three_shortcut_layout();
        let y0 = drag_slot_y(&lay, 0, 2, 1);
        let y1 = drag_slot_y(&lay, 1, 2, 1);
        // slot 1 is freed by the dragged row; rows land at 0 and 2
        assert_eq!(y1 - y0, row_stride() * 2.0);
        // inserting at the top pushes everything down one slot
        assert_eq!(drag_slot_y(&lay, 0, 2, 0), y0 + row_stride());
        // unaffected rows keep their natural slot
        assert_eq!(drag_slot_y(&lay, 1, 0, 0), lay.list_top + row_stride());
    }

    #[test]
    fn card_interior_not_tinted_by_shadow() {
        // The card is translucent (0xF0); with the old offset-rrect shadows the
        // interior pixels had shadow blended underneath. The new outer shadow
        // never touches the card, so interior RGB is exactly the card color.
        let lay = three_shortcut_layout();
        let (w, _h, rgba) = compose_menu_card_layer(&lay, 1.0, MenuChromeState::default());
        let px = |x: u32, y: u32| -> [u8; 4] {
            let i = ((y * w + x) * 4) as usize;
            [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
        };
        let interior = px(190, 140);
        assert_eq!(interior, CARD, "interior pixel must be pure card color");
    }

    #[test]
    fn card_has_no_outer_shadow() {
        let lay = three_shortcut_layout();
        let (w, _h, rgba) = compose_menu_card_layer(&lay, 1.0, MenuChromeState::default());
        let alpha = |x: u32, y: u32| -> u8 {
            rgba[((y * w + x) * 4 + 3) as usize]
        };
        // Nothing above, left of, below or right of the card — no drop shadow.
        assert_eq!(alpha(300, 10), 0, "nothing above the card");
        assert_eq!(alpha(100, 100), 0, "nothing left of the card");
        assert_eq!(alpha(155, 22), 0, "top-left notch stays transparent");
        assert_eq!(alpha(512, 22), 0, "top-right notch stays transparent");
        for y in 381..396 {
            assert_eq!(alpha(330, y), 0, "no shadow band below the card at y={y}");
        }
        for x in 511..526 {
            assert_eq!(alpha(x, 200), 0, "no shadow band right of the card at x={x}");
        }
    }

    #[test]
    fn top_sheen_follows_card_outline() {
        let lay = three_shortcut_layout();
        let (w, _h, rgba) = compose_menu_card_layer(&lay, 1.0, MenuChromeState::default());
        let px = |x: u32, y: u32| -> [u8; 4] {
            let i = ((y * w + x) * 4) as usize;
            [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
        };
        // The highlight brightens the top band …
        assert!(px(190, 24)[1] > CARD[1], "top band should be brightened");
        // … but never escapes the rounded corners into the transparent wedge.
        assert_eq!(px(155, 22)[3], 0, "sheen must not bleed past the corner");
        assert_eq!(px(508, 22)[3], 0, "sheen must not bleed past the corner");
    }

    #[test]
    #[ignore]
    fn dump_card_border_preview() {
        let lay = three_shortcut_layout();
        let (w, h, rgba) = compose_menu_card_layer(&lay, 2.0, MenuChromeState::default());
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/_card_border.png");
        image::save_buffer(&path, &rgba, w, h, image::ColorType::Rgba8).expect("write preview");
        // Zoomed crops of the corners + right/bottom edge for inspection.
        let zoom = |cx: u32, cy: u32, half: u32, name: &str, z: u32| {
            let x0 = cx.saturating_sub(half);
            let y0 = cy.saturating_sub(half);
            let mut dst = vec![0u8; (half * 2 * z * half * 2 * z * 4) as usize];
            for py in 0..(half * 2 * z) {
                for px in 0..(half * 2 * z) {
                    let sx = x0 + px / z;
                    let sy = y0 + py / z;
                    if sx >= w || sy >= h {
                        continue;
                    }
                    let si = ((sy * w + sx) * 4) as usize;
                    let di = ((py * half * 2 * z + px) * 4) as usize;
                    dst[di..di + 4].copy_from_slice(&rgba[si..si + 4]);
                }
            }
            let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join(format!("target/_card_{name}.png"));
            image::save_buffer(&p, &dst, half * 2 * z, half * 2 * z, image::ColorType::Rgba8)
                .expect("write corner crop");
        };
        zoom(151, 21, 8, "tl", 8); // top-left corner (device px, dpr = 2 → logical 20, 20)
        zoom(1023, 21, 8, "tr", 8); // top-right corner
        zoom(151, 765, 8, "bl", 8); // bottom-left
        zoom(1023, 765, 8, "br", 8); // bottom-right shadow corner
        zoom(500, 790, 8, "bottom_edge", 6);
        zoom(1050, 400, 8, "right_edge", 6);
        assert!(path.is_file());
    }

    #[test]
    fn present_drag_same_frame_outside_overlay_boxes() {
        // Blank layer + pre-rendered rows + present_menu_drag must reproduce
        // compose_menu_frame everywhere except the overlay regions (rows, the
        // lifted row, bowl, hint).
        let lay = three_shortcut_layout();
        let (pw, ph, pet) = dummy_pet();
        let drag = drag_chrome("App2");

        let chrome = MenuChromeState {
            drag: Some(drag.clone()),
            ..MenuChromeState::default()
        };
        let (w, h, full) = compose_menu_frame(&pet, pw, ph, &lay, 1.0, chrome.clone());

        let mut blank = chrome.clone();
        blank.drag = None;
        blank.drag_draft = false;
        blank.rows_blank = true;
        let (cw, ch, base) = compose_menu_card_layer(&lay, 1.0, blank);
        let rows = prerender_list_rows(&lay, 1.0);
        let mut out = vec![0u8; (cw * ch * 4) as usize];
        present_menu_drag(&mut out, cw, ch, &base, &rows, &pet, pw, ph, &lay, 1.0, None, Some(&drag));

        assert_eq!((w, h), (cw, ch));

        // Lifted row box: pointer − grab, lifted 1.04×, no shadow pad.
        let gx = 220.0f32 - 40.0 - (300.0f32 * 1.04 - 300.0) * 0.5;
        let gy = 240.0f32 - 16.0 - (42.0f32 * 1.04 - 42.0) * 0.5;
        let gbox = (
            (gx - 2.0).floor().max(0.0) as i32,
            (gy - 2.0).floor().max(0.0) as i32,
            (300.0f32 * 1.04 + 4.0).ceil() as i32,
            (42.0f32 * 1.04 + 4.0).ceil() as i32,
        );
        // List viewport box: shifted rows live inside it (2 px tolerance).
        let (row_x, row_w) = lay
            .items
            .iter()
            .find_map(|it| match &it.entry {
                MenuEntry::Shortcut { .. } => Some((it.x, it.w)),
                _ => None,
            })
            .unwrap_or((lay.card_x, lay.card_w));
        let vbox = (
            (row_x - 4.0).floor().max(0.0) as i32,
            (lay.list_top - 4.0).floor().max(0.0) as i32,
            (row_w + 8.0).ceil() as i32,
            ((lay.list_bottom - lay.list_top) + 8.0).ceil() as i32,
        );
        // Bowl + hint box (hint sits under the bowl on this tall window).
        let (bx, by, bw, bh) = bowl_rect(8.0, 80.0, 128.0, 128.0, 500.0, 480.0);
        let bbox = (
            (bx - 12.0).floor().max(0.0) as i32,
            (by - 12.0).floor().max(0.0) as i32,
            (bw + 24.0).ceil() as i32,
            (bh + 12.0 + 48.0).ceil() as i32,
        );
        let in_box = |x: i32, y: i32| {
            let (x0, y0, w0, h0) = gbox;
            let (x1, y1, w1, h1) = vbox;
            let (x2, y2, w2, h2) = bbox;
            (x >= x0 && x < x0 + w0 && y >= y0 && y < y0 + h0)
                || (x >= x1 && x < x1 + w1 && y >= y1 && y < y1 + h1)
                || (x >= x2 && x < x2 + w2 && y >= y2 && y < y2 + h2)
        };

        let mut outside = 0usize;
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                if full[i..i + 4] != out[i..i + 4] && !in_box(x as i32, y as i32) {
                    outside += 1;
                }
            }
        }
        assert_eq!(outside, 0, "diffs outside drag overlay boxes: {outside}");
    }
}

#[cfg(test)]
mod gear_preview {
    use super::*;
    use std::path::PathBuf;

    #[test]
    #[ignore]
    fn dump_settings_gear() {
        let dpr = 2.0;
        let logical = crate::ui::radial_menu::GEAR_SIZE;
        let size = (logical * dpr).round() as u32;
        let pad = 16u32;
        let w = size * 3 + pad * 4;
        let h = size + pad * 2;
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        let plates = [
            ([0xFF, 0xFB, 0xFA, 0xFF], pad, 0.0),
            ([0xFF, 0xFF, 0xFF, 0xFF], pad * 2 + size, 1.0),
            ([0x2A, 0x22, 0x1C, 0xFF], pad * 3 + size * 2, 0.0),
        ];
        for (bg, x0, hover) in plates {
            fill_rrect_aa(
                &mut rgba,
                w,
                h,
                x0 as f32,
                pad as f32,
                (x0 + size) as f32,
                (pad + size) as f32,
                8.0,
                bg,
            );
            draw_settings_btn(
                &mut rgba,
                w,
                h,
                Dpi::new(dpr),
                x0 as f32,
                pad as f32,
                size as f32,
                size as f32,
                hover,
                0.0,
                1.0,
            );
        }
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/_settings_paw.png");
        image::save_buffer(
            &path,
            &rgba,
            w,
            h,
            image::ColorType::Rgba8,
        )
        .expect("write settings mark preview");
        assert!(path.is_file());
    }
}
