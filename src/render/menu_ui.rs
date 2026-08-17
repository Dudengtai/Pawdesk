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
use crate::ui::radial_menu::{ExpandDir, MenuEntry, RadialLayout};

// ── Palette (playful warm glass · design §2 tokens) ───────────────────────
/// Cream card, a touch rosier than the old near-white.
const CARD: [u8; 4] = [0xFF, 0xF8, 0xF4, 0xF0];
/// Soft muted surfaces (rows / chips) — pink wash.
const GROUPED_BG: [u8; 4] = [0xFF, 0xEC, 0xF2, 0x8C];
const GROUPED_HOVER: [u8; 4] = [0xFF, 0xD6, 0xE2, 0xB8];
const GROUPED_PRESS: [u8; 4] = [0xF6, 0xD0, 0xDC, 0xD0];
const INVALID_BG: [u8; 4] = [0xFF, 0xB0, 0x20, 0x30];
const INVALID_BG_HOVER: [u8; 4] = [0xFF, 0xB0, 0x20, 0x48];
const HIGHLIGHT_ROW: [u8; 4] = [0xFF, 0x9E, 0xC4, 0x38];
const SEPARATOR: [u8; 4] = [0xE2, 0xE0, 0xDE, 0x90];
const BORDER: [u8; 4] = [0xFF, 0x9E, 0xC4, 0x48];
const HAIRLINE: [u8; 4] = [0xFF, 0xFF, 0xFF, 0x80];
const INNER_HL: [u8; 4] = [0xFF, 0xFF, 0xFF, 0x55];
const PAW_INK: [u8; 4] = [0x9A, 0x40, 0x68, 0xFF];
const PAW_KICKER: [u8; 4] = [0xFF, 0x7A, 0xAF, 0xFF];
const TITLE: &str = "给你叼来了";
const SUBTITLE: &str = "想开哪个？";
const ADD_LABEL: &str = "再叼一个";
const CLOSE_HINT: &str = "拍拍收起";
const EMPTY_TITLE: &str = "还没叼来应用";
const EMPTY_HINT: &str = "点「再叼一个」选 exe / 快捷方式";
pub const SAY_LAUNCH: &str = "收到，马上打开～";
pub const SAY_FAIL: &str = "这个应用好像搬家了…";
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
const RED: [u8; 4] = [0xE1, 0x1D, 0x48, 0xFF];
const WHITE: [u8; 4] = [0xFF, 0xFF, 0xFF, 0xFF];
const ACCENT_PINK: [u8; 4] = [0xFF, 0x9E, 0xC4, 0xFF];
const SHADOW_A: [u8; 4] = [0x0F, 0x17, 0x2B, 0x0A];
const SHADOW_B: [u8; 4] = [0x0F, 0x17, 0x2B, 0x12];
const SHADOW_C: [u8; 4] = [0x0F, 0x17, 0x2B, 0x1C];
const SHADOW_BTN: [u8; 4] = [0x0F, 0x17, 0x2B, 0x28];

/// Hover / press indices into `layout.items` + animated blends (0..1).
#[derive(Debug, Clone, Copy, Default)]
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
    let layout_scaled;
    let layout = if (scale - 1.0).abs() < 0.001 {
        layout
    } else {
        let pivot_x = layout.pet_x + layout.pet_w * 0.5;
        let pivot_y = layout.pet_y + layout.pet_h * 0.5;
        layout_scaled = scale_layout_from_pivot(layout, pivot_x, pivot_y, scale);
        &layout_scaled
    };

    paint_menu_card(&mut out, w, h, dpi, layout, chrome, t_fade);

    // Pet always full opacity. Plate / close hint fades with the card so the
    // silhouette does not flash into a glass tray on frame 0.
    draw_avatar(
        &mut out,
        w,
        h,
        dpi,
        layout.pet_x,
        layout.pet_y,
        layout.pet_w,
        layout.pet_h,
        pet_rgba,
        pet_w,
        pet_h,
        t_fade,
    );
    if let Some(line) = chrome.say {
        draw_say_bubble(&mut out, w, h, dpi, layout, line, t_fade.max(0.85));
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
    paint_menu_card(&mut out, w, h, dpi, layout, chrome, 1.0);
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
        h,
        dpi,
        layout.pet_x,
        layout.pet_y,
        layout.pet_w,
        layout.pet_h,
        pet_rgba,
        pet_w,
        pet_h,
        0.0,
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
        dh,
        dpi,
        layout.pet_x,
        layout.pet_y,
        layout.pet_w,
        layout.pet_h,
        pet_rgba,
        pet_src_w,
        pet_src_h,
        fade,
    );
}

fn paint_menu_card(
    mut out: &mut [u8],
    w: u32,
    h: u32,
    dpi: Dpi,
    layout: &RadialLayout,
    chrome: MenuChromeState,
    t_fade: f32,
) {
    let t_shadow = (t_fade * t_fade).clamp(0.0, 1.0);

    // Elevated glass card (rest of union window stays transparent for pin-pet).
    let cx0 = dpi.s(layout.card_x);
    let cy0 = dpi.s(layout.card_y);
    let cx1 = dpi.s(layout.card_x + layout.card_w);
    let cy1 = dpi.s(layout.card_y + layout.card_h);
    let crad = dpi.s(22.0);

    if t_fade > 0.01 {
        fill_rrect_aa(
            &mut out,
            w,
            h,
            cx0 + dpi.s(10.0),
            cy0 + dpi.s(14.0),
            cx1 + dpi.s(8.0),
            cy1 + dpi.s(12.0),
            crad,
            with_alpha(SHADOW_A, t_shadow),
        );
        fill_rrect_aa(
            &mut out,
            w,
            h,
            cx0 + dpi.s(5.0),
            cy0 + dpi.s(7.0),
            cx1 + dpi.s(4.0),
            cy1 + dpi.s(6.0),
            crad,
            with_alpha(SHADOW_B, t_shadow),
        );
        fill_rrect_aa(
            &mut out,
            w,
            h,
            cx0 + dpi.s(1.5),
            cy0 + dpi.s(2.5),
            cx1 + dpi.s(1.5),
            cy1 + dpi.s(2.0),
            crad,
            with_alpha(SHADOW_C, t_shadow * 0.9),
        );
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
        // Top sheen
        fill_rrect_aa(
            &mut out,
            w,
            h,
            cx0 + dpi.s(1.5),
            cy0 + dpi.s(1.5),
            cx1 - dpi.s(1.5),
            cy0 + dpi.s(20.0),
            dpi.s(14.0),
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
        stroke_rrect_aa(
            &mut out,
            w,
            h,
            cx0 + 1.0,
            cy0 + 1.0,
            cx1 - 1.0,
            cy1 - 1.0,
            crad - 0.5,
            with_alpha(HAIRLINE, t_fade),
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

    // Tens/day: items fade with the card. No stagger, no extra y.
    let mut saw_shortcut = false;
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
            MenuEntry::Shortcut { name, valid, icon, .. } => {
                saw_shortcut = true;
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

    if !saw_shortcut {
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

fn draw_say_bubble(
    out: &mut [u8],
    w: u32,
    h: u32,
    dpi: Dpi,
    layout: &RadialLayout,
    line: &str,
    fade: f32,
) {
    let fade = fade.clamp(0.0, 1.0);
    if fade < 0.05 {
        return;
    }
    let max_w = dpi.su(168);
    let Some((tw, th, tbuf)) =
        rasterize_text(line, max_w, dpi.px(12.0), with_alpha(PAW_INK, fade))
    else {
        return;
    };
    let pad_x = dpi.s(10.0);
    let pad_y = dpi.s(6.0);
    let bw = tw as f32 + pad_x * 2.0;
    let bh = th as f32 + pad_y * 2.0;
    let toward_card = if layout.card_x + layout.card_w * 0.5 >= layout.pet_x + layout.pet_w * 0.5 {
        1.0
    } else {
        -1.0
    };
    let x = if toward_card > 0.0 {
        dpi.s(layout.pet_x + layout.pet_w * 0.55)
    } else {
        dpi.s(layout.pet_x + layout.pet_w * 0.45) - bw
    };
    let y = dpi.s(layout.pet_y + 4.0);
    let x = x.clamp(dpi.s(2.0), (w as f32 - bw - 2.0).max(0.0));
    let y = y.clamp(dpi.s(2.0), (h as f32 - bh - 2.0).max(0.0));
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
    let (disc, disc_d) = if valid {
        (PRIMARY, PRIMARY_HOVER)
    } else {
        (ORANGE, ORANGE)
    };

    if valid {
        if let Some(icon) = icon {
            match icon.shape {
                IconShape::Round => {
                    // Dark tile container fills the empty corners, then the
                    // icon at ~80% (Windows-11-style inner inset).
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
                    // No container — the icon itself fills the slot (~92%).
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
    let y = layout
        .items
        .iter()
        .map(|i| i.y + i.h)
        .fold(layout.card_y + 96.0, f32::max)
        + 20.0;
    y as u32
}

fn draw_avatar(
    out: &mut [u8],
    w: u32,
    h: u32,
    dpi: Dpi,
    pet_x: f32,
    pet_y: f32,
    pet_w: f32,
    pet_h: f32,
    pet_rgba: &[u8],
    src_w: u32,
    src_h: u32,
    // 0 = free silhouette (idle look); 1 = full glass plate + hint.
    plate_a: f32,
) {
    let plate_a = plate_a.clamp(0.0, 1.0);
    let px = dpi.s(pet_x);
    let py = dpi.s(pet_y);
    let pw = dpi.s(pet_w);
    let ph = dpi.s(pet_h);
    let rad = dpi.s(18.0);

    if plate_a > 0.02 {
        fill_rrect_aa(
            out,
            w,
            h,
            px + dpi.s(3.0),
            py + dpi.s(5.0),
            px + pw + dpi.s(3.0),
            py + ph + dpi.s(5.0),
            rad,
            with_alpha(SHADOW_B, plate_a),
        );
        fill_rrect_aa(
            out,
            w,
            h,
            px,
            py,
            px + pw,
            py + ph,
            rad,
            with_alpha(CARD, plate_a),
        );
        stroke_rrect_aa(
            out,
            w,
            h,
            px + 0.5,
            py + 0.5,
            px + pw - 0.5,
            py + ph - 0.5,
            rad,
            with_alpha(SEPARATOR, plate_a),
            1.0,
        );
    }

    // Idle: fill almost the whole pin rect. Menu settled: leave room for label.
    let inset = lerp(dpi.s(4.0), dpi.s(40.0), plate_a);
    let max = (pw.min(ph) - inset).max(dpi.s(48.0)) as u32;
    let (sw, sh, pet) = scale_rgba_fit(pet_rgba, src_w, src_h, max, max);
    let label_pad = lerp(0.0, dpi.s(28.0), plate_a);
    let ox = (px + (pw - sw as f32) * 0.5).round() as u32;
    let oy = (py + (ph - label_pad - sh as f32) * 0.5 + dpi.s(2.0))
        .round()
        .max(py + dpi.s(2.0)) as u32;

    if plate_a > 0.02 {
        fill_soft_ellipse(
            out,
            w,
            h,
            (px + pw * 0.5) as i32,
            (py + ph - dpi.s(34.0)) as i32,
            dpi.s(30.0) as i32,
            dpi.s(8.0) as i32,
            with_alpha(SHADOW_B, plate_a),
        );
    }
    blit(out, w, h, &pet, sw, sh, ox, oy);

    if plate_a > 0.05 {
        if let Some((tw, th, t)) =
            rasterize_text(CLOSE_HINT, dpi.su(110), dpi.px(11.0), with_alpha(PAW_INK, plate_a * 0.85))
        {
            let tx = (px + (pw - tw as f32) * 0.5).round() as u32;
            let ty = (py + ph - dpi.s(18.0) - th as f32 * 0.5).round() as u32;
            blit(out, w, h, &t, tw, th, tx, ty);
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
pub const SETTINGS_H: u32 = 640;
pub const ROW_H: f32 = 48.0;
/// Shortcut list starts below reminder + pet-size cards.
pub const LIST_TOP: f32 = 300.0;
const REMINDER_CARD_TOP: f32 = 72.0;
const REMINDER_CARD_H: f32 = 120.0;
const PET_CARD_TOP: f32 = 204.0;
const PET_CARD_H: f32 = 72.0;

#[derive(Debug, Clone, Copy)]
pub enum SettingsHit {
    Close,
    ToggleEnabled,
    IntervalDec,
    IntervalInc,
    TogglePause,
    PetScaleDec,
    PetScaleInc,
    Add,
    RowToggle(usize),
    RowUp(usize),
    RowDown(usize),
    RowDelete(usize),
}

/// `reminder`: (enabled, interval_minutes, paused)
/// `pet_scale`: relative size vs 128px baseline (e.g. 0.6)
/// `highlight_row`: optional list index to emphasize (invalid shortcut from launcher).
pub fn compose_settings_frame(
    names: &[(String, bool, bool)],
    reminder: (bool, u32, bool),
    pet_scale: f32,
    dpr: f32,
    highlight_row: Option<usize>,
) -> (u32, u32, Vec<u8>) {
    let (enabled, interval_min, paused) = reminder;
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
    fill_rrect_aa(
        &mut out,
        w,
        h,
        dpi.s(1.5),
        dpi.s(1.5),
        wf - dpi.s(1.5),
        dpi.s(22.0),
        dpi.s(14.0),
        INNER_HL,
    );
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
        "提醒与常用应用",
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

    let status = if !enabled {
        "已关闭"
    } else if paused {
        "已暂停"
    } else {
        "运行中"
    };
    blit_text(
        &mut out,
        w,
        h,
        status,
        dpi.s(300.0),
        dpi.s(REMINDER_CARD_TOP + 70.0),
        dpi.su(90),
        dpi.px(13.5),
        if paused || !enabled { ORANGE } else { BLUE },
    );
    // Pause toggle chip
    let pause_label = if paused { "恢复" } else { "暂停" };
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

    // ── Shortcut list ──
    blit_text(
        &mut out,
        w,
        h,
        "常用应用",
        dpi.s(24.0),
        dpi.s(LIST_TOP - 22.0),
        dpi.su(160),
        dpi.px(13.5),
        SECONDARY,
    );

    let list_top = dpi.s(LIST_TOP);
    let list_bottom = hf - dpi.s(88.0);
    fill_rrect_aa(
        &mut out,
        w,
        h,
        dpi.s(20.0),
        list_top,
        wf - dpi.s(20.0),
        list_bottom,
        dpi.s(12.0),
        GROUPED_BG,
    );
    stroke_rrect_aa(
        &mut out,
        w,
        h,
        dpi.s(20.0) + 0.5,
        list_top + 0.5,
        wf - dpi.s(20.0) - 0.5,
        list_bottom - 0.5,
        dpi.s(12.0),
        SOFT_BORDER,
        1.0,
    );

    if names.is_empty() {
        if let Some((tw, th, t)) = rasterize_text("列表为空", dpi.su(160), dpi.px(15.0), LABEL) {
            blit(
                &mut out,
                w,
                h,
                &t,
                tw,
                th,
                (w - tw) / 2,
                (list_top + list_bottom) as u32 / 2 - 12,
            );
        }
    }

    let row_h = dpi.s(ROW_H);
    for (i, (name, sc_en, valid)) in names.iter().enumerate() {
        let y = list_top + i as f32 * row_h;
        if y + row_h > list_bottom - dpi.s(8.0) {
            break;
        }
        if highlight_row == Some(i) {
            fill_rrect_aa(
                &mut out,
                w,
                h,
                dpi.s(24.0),
                y + dpi.s(2.0),
                wf - dpi.s(24.0),
                y + row_h - dpi.s(2.0),
                dpi.s(10.0),
                HIGHLIGHT_ROW,
            );
        }
        if i > 0 && highlight_row != Some(i) && highlight_row != Some(i - 1) {
            fill_rect_f(
                &mut out,
                w,
                h,
                dpi.s(36.0),
                y,
                wf - dpi.s(72.0),
                1.0,
                SEPARATOR,
            );
        }
        let mark = if !valid {
            "!"
        } else if *sc_en {
            "●"
        } else {
            "○"
        };
        let color = if *valid { LABEL } else { ORANGE };
        let label = if *valid {
            format!("{mark}  {name}")
        } else {
            format!("{mark}  {name} · 无法找到程序")
        };
        if let Some((tw, th, t)) = rasterize_text(&label, dpi.su(180), dpi.px(14.0), color) {
            let ty = (y + (row_h - th as f32) * 0.5 + dpi.s(0.5)).round() as u32;
            blit(&mut out, w, h, &t, tw, th, dpi.s(36.0) as u32, ty);
        }
        let mid_y = (y + row_h * 0.5 - dpi.s(6.0)) as u32;
        trailing_btn(&mut out, w, h, w - dpi.su(184), mid_y, "上移", BLUE, dpi);
        trailing_btn(&mut out, w, h, w - dpi.su(136), mid_y, "下移", BLUE, dpi);
        trailing_btn(&mut out, w, h, w - dpi.su(88), mid_y, "启停", BLUE, dpi);
        trailing_btn(&mut out, w, h, w - dpi.su(48), mid_y, "删除", RED, dpi);
    }

    fill_rrect_aa(
        &mut out,
        w,
        h,
        dpi.s(24.0),
        hf - dpi.s(70.0),
        wf - dpi.s(24.0),
        hf - dpi.s(22.0),
        dpi.s(12.0),
        SHADOW_BTN,
    );
    fill_rrect_aa(
        &mut out,
        w,
        h,
        dpi.s(24.0),
        hf - dpi.s(72.0),
        wf - dpi.s(24.0),
        hf - dpi.s(24.0),
        dpi.s(12.0),
        PRIMARY,
    );
    blit_text_centered(
        &mut out,
        w,
        h,
        "添加应用",
        dpi.s(24.0),
        hf - dpi.s(72.0),
        wf - dpi.s(48.0),
        dpi.s(48.0),
        dpi.px(15.0),
        WHITE,
        dpi,
    );

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
    pub pet_dec: (f32, f32, f32, f32),
    pub pet_inc: (f32, f32, f32, f32),
    pub list_top: f32,
    pub list_bottom: f32,
    pub row_h: f32,
    pub add: (f32, f32, f32, f32),
}

pub fn settings_card_metrics(card_w: f32, card_h: f32) -> SettingsCardMetrics {
    let w = card_w.max(200.0);
    let h = card_h.max(240.0);
    let reminder_y = 46.0;
    let reminder_h = 86.0;
    let pet_y = reminder_y + reminder_h + 6.0;
    let pet_h = 42.0;
    let list_top = pet_y + pet_h + 22.0;
    let add_h = 34.0;
    let add_y = h - 12.0 - add_h;
    let list_bottom = (add_y - 8.0).max(list_top + 36.0);
    SettingsCardMetrics {
        w,
        h,
        reminder_y,
        enable_y0: reminder_y + 28.0,
        enable_y1: reminder_y + 50.0,
        interval_dec: (16.0, reminder_y + 52.0, 44.0, 26.0),
        interval_inc: (132.0, reminder_y + 52.0, 44.0, 26.0),
        pause: (w - 86.0, reminder_y + 28.0, 70.0, 24.0),
        pet_dec: (16.0, pet_y + 8.0, 36.0, 26.0),
        pet_inc: (120.0, pet_y + 8.0, 36.0, 26.0),
        list_top,
        list_bottom,
        row_h: 34.0,
        add: (16.0, add_y, w - 32.0, add_h),
    }
}

pub fn settings_card_visible_rows(m: &SettingsCardMetrics) -> usize {
    let span = (m.list_bottom - m.list_top).max(0.0);
    ((span / m.row_h).floor() as usize).max(1)
}

pub fn compose_settings_card(
    names: &[(String, bool, bool)],
    reminder: (bool, u32, bool),
    pet_scale: f32,
    dpr: f32,
    highlight_row: Option<usize>,
    card_w: f32,
    card_h: f32,
    list_scroll: usize,
) -> (u32, u32, Vec<u8>) {
    let (enabled, interval_min, paused) = reminder;
    let m = settings_card_metrics(card_w, card_h);
    let dpi = Dpi::new(dpr);
    let w = dpi.su(m.w.round() as u32);
    let h = dpi.su(m.h.round() as u32);
    let mut out = vec![0u8; (w * h * 4) as usize];
    let wf = w as f32;
    let hf = h as f32;
    let visible = settings_card_visible_rows(&m);
    let max_scroll = names.len().saturating_sub(visible);
    let scroll = list_scroll.min(max_scroll);

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
    fill_rrect_aa(
        &mut out,
        w,
        h,
        dpi.s(1.5),
        dpi.s(1.5),
        wf - dpi.s(1.5),
        dpi.s(18.0),
        dpi.s(14.0),
        INNER_HL,
    );
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
        if paused { "已暂停" } else { "暂停" },
        dpi.s(px),
        dpi.s(py),
        dpi.s(pw),
        dpi.s(ph),
        dpi.px(12.0),
        if paused { ORANGE } else { SECONDARY },
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
    blit_text(
        &mut out,
        w,
        h,
        &interval_label,
        dpi.s(dx + dw + 6.0),
        dpi.s(dy + 4.0),
        dpi.su(72),
        dpi.px(13.0),
        LABEL,
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
    blit_text(
        &mut out,
        w,
        h,
        &format!("{pct}%"),
        dpi.s(pdx + pdw + 8.0),
        dpi.s(pdy + 4.0),
        dpi.su(56),
        dpi.px(14.0),
        LABEL,
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

    blit_text(
        &mut out,
        w,
        h,
        "常用应用",
        dpi.s(16.0),
        dpi.s(m.list_top - 16.0),
        dpi.su(140),
        dpi.px(12.0),
        SECONDARY,
    );

    let vis_names: Vec<(usize, &(String, bool, bool))> = names
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible)
        .collect();
    for (slot, (i, (name, sc_en, valid))) in vis_names.iter().enumerate() {
        let y = dpi.s(m.list_top + slot as f32 * m.row_h);
        let bh = dpi.s(m.row_h);
        if highlight_row == Some(*i) {
            fill_rrect_aa(
                &mut out,
                w,
                h,
                dpi.s(12.0),
                y,
                wf - dpi.s(12.0),
                y + bh - dpi.s(2.0),
                dpi.s(8.0),
                HIGHLIGHT_ROW,
            );
        }
        let mark = if !*valid {
            "!"
        } else if *sc_en {
            "●"
        } else {
            "○"
        };
        let color = if *valid { LABEL } else { ORANGE };
        let label = format!("{mark}  {name}");
        if let Some((tw, th, t)) = rasterize_text(&label, dpi.su(150), dpi.px(12.5), color) {
            let ty = (y + (bh - th as f32) * 0.5).round() as u32;
            blit(&mut out, w, h, &t, tw, th, dpi.s(18.0) as u32, ty);
        }
        let mid_y = (y + bh * 0.5 - dpi.s(6.0)) as u32;
        trailing_btn(&mut out, w, h, w - dpi.su(132), mid_y, "上", BLUE, dpi);
        trailing_btn(&mut out, w, h, w - dpi.su(100), mid_y, "下", BLUE, dpi);
        trailing_btn(&mut out, w, h, w - dpi.su(68), mid_y, "开", BLUE, dpi);
        trailing_btn(&mut out, w, h, w - dpi.su(36), mid_y, "删", RED, dpi);
    }

    let (ax, ay, aw, ah) = m.add;
    fill_rrect_aa(
        &mut out,
        w,
        h,
        dpi.s(ax),
        dpi.s(ay),
        dpi.s(ax + aw),
        dpi.s(ay + ah),
        dpi.s(12.0),
        PRIMARY,
    );
    blit_text_centered(
        &mut out,
        w,
        h,
        "添加应用",
        dpi.s(ax),
        dpi.s(ay),
        dpi.s(aw),
        dpi.s(ah),
        dpi.px(14.0),
        WHITE,
        dpi,
    );

    (w, h, out)
}

pub fn hit_settings_card(
    local_x: f32,
    local_y: f32,
    card_w: f32,
    card_h: f32,
    row_count: usize,
    list_scroll: usize,
) -> Option<SettingsHit> {
    let m = settings_card_metrics(card_w, card_h);
    if local_x >= m.w - 64.0 && local_y <= 42.0 {
        return Some(SettingsHit::Close);
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
    if in_rect(local_x, local_y, m.add) {
        return Some(SettingsHit::Add);
    }
    let visible = settings_card_visible_rows(&m);
    for slot in 0..visible {
        let i = list_scroll + slot;
        if i >= row_count {
            break;
        }
        let y0 = m.list_top + slot as f32 * m.row_h;
        let y1 = y0 + m.row_h - 2.0;
        if local_y < y0 || local_y > y1 {
            continue;
        }
        if local_x >= m.w - 48.0 {
            return Some(SettingsHit::RowDelete(i));
        }
        if local_x >= m.w - 80.0 {
            return Some(SettingsHit::RowToggle(i));
        }
        if local_x >= m.w - 112.0 {
            return Some(SettingsHit::RowDown(i));
        }
        if local_x >= m.w - 144.0 {
            return Some(SettingsHit::RowUp(i));
        }
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
        assert!(m.list_bottom > m.list_top);
        assert!(m.add.1 + m.add.3 <= 360.0);
        assert!(settings_card_visible_rows(&m) >= 2);
        let (w, h, out) = compose_settings_card(
            &[("Chrome".into(), true, true)],
            (true, 45, false),
            1.0,
            1.0,
            None,
            360.0,
            360.0,
            0,
        );
        assert_eq!(out.len(), (w * h * 4) as usize);
        assert!(w > 200 && h > 200);
        assert!(matches!(
            hit_settings_card(330.0, 20.0, 360.0, 360.0, 1, 0),
            Some(SettingsHit::Close)
        ));
    }
}

pub fn hit_settings(local_x: f32, local_y: f32, row_count: usize) -> Option<SettingsHit> {
    // local coords are logical (caller maps physical → logical)
    let w = SETTINGS_W as f32;
    let h = SETTINGS_H as f32;
    if local_x >= w - 72.0 && local_y <= 52.0 {
        return Some(SettingsHit::Close);
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
    if (24.0..=w - 24.0).contains(&local_x) && (h - 72.0..=h - 24.0).contains(&local_y) {
        return Some(SettingsHit::Add);
    }
    for i in 0..row_count {
        let y = LIST_TOP + i as f32 * ROW_H;
        if local_y < y || local_y > y + ROW_H - 4.0 {
            continue;
        }
        if local_x >= w - 48.0 - 20.0 {
            return Some(SettingsHit::RowDelete(i));
        }
        if local_x >= w - 88.0 - 16.0 {
            return Some(SettingsHit::RowToggle(i));
        }
        if local_x >= w - 136.0 - 16.0 {
            return Some(SettingsHit::RowDown(i));
        }
        if local_x >= w - 184.0 - 16.0 {
            return Some(SettingsHit::RowUp(i));
        }
    }
    None
}

fn trailing_btn(
    out: &mut [u8],
    w: u32,
    h: u32,
    x: u32,
    y: u32,
    label: &str,
    color: [u8; 4],
    dpi: Dpi,
) {
    if let Some((tw, th, t)) = rasterize_text(label, dpi.su(48), dpi.px(12.5), color) {
        blit(out, w, h, &t, tw, th, x.saturating_sub(tw / 2), y);
    }
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
