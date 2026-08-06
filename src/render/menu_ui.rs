//! Apple HIG–inspired quick-launch dock + manager UI.
//!
//! Drawn at **device pixel ratio** so text/edges stay sharp on HiDPI
//! (no low-res compose → bilinear upscale blur).

use crate::render::text::{center_in_rect, rasterize_text};
use crate::ui::radial_menu::{MenuEntry, RadialLayout};

// ── Palette (warm glass · design §3 / §7 · no system Acrylic) ─────────────
/// Warm panel ~88% alpha (L4 glass 拟态).
const CARD: [u8; 4] = [0xFF, 0xF8, 0xF2, 0xE0];
/// Soft frosted rows.
const GROUPED_BG: [u8; 4] = [0xF5, 0xF0, 0xEB, 0xC8];
const GROUPED_HOVER: [u8; 4] = [0xFF, 0xFF, 0xFF, 0xD8];
const GROUPED_PRESS: [u8; 4] = [0xFF, 0xEE, 0xF5, 0xE8];
const INVALID_BG: [u8; 4] = [0xFF, 0xB0, 0x20, 0x38];
const INVALID_BG_HOVER: [u8; 4] = [0xFF, 0xB0, 0x20, 0x55];
const HIGHLIGHT_ROW: [u8; 4] = [0xFF, 0x9E, 0xC4, 0x40];
const SEPARATOR: [u8; 4] = [0xC6, 0xC6, 0xC8, 0x70];
const HAIRLINE: [u8; 4] = [0xFF, 0xFF, 0xFF, 0x55];
const INNER_HL: [u8; 4] = [0xFF, 0xFF, 0xFF, 0x48];
const LABEL: [u8; 4] = [0x3A, 0x35, 0x40, 0xFF];
const SECONDARY: [u8; 4] = [0x3C, 0x3C, 0x43, 0xB0];
const TERTIARY: [u8; 4] = [0x3C, 0x3C, 0x43, 0x70];
const BLUE: [u8; 4] = [0x00, 0x7A, 0xFF, 0xFF];
const BLUE_PRESS: [u8; 4] = [0x00, 0x64, 0xD6, 0xFF];
const FILL_OPAQUE: [u8; 4] = [0xE8, 0xE4, 0xE0, 0xE0];
const FILL_HOVER: [u8; 4] = [0xFF, 0xFF, 0xFF, 0xE8];
const ORANGE: [u8; 4] = [0xC4, 0x7A, 0x00, 0xFF];
const RED: [u8; 4] = [0xFF, 0x3B, 0x30, 0xFF];
const WHITE: [u8; 4] = [0xFF, 0xFF, 0xFF, 0xFF];
const SHADOW_A: [u8; 4] = [0x00, 0x00, 0x00, 0x0E];
const SHADOW_B: [u8; 4] = [0x00, 0x00, 0x00, 0x18];
const SHADOW_C: [u8; 4] = [0x00, 0x00, 0x00, 0x28];

/// Hover / press indices into `layout.items`.
#[derive(Debug, Clone, Copy, Default)]
pub struct MenuChromeState {
    pub hover: Option<usize>,
    pub press: Option<usize>,
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
    reminder_paused: bool,
    dpr: f32,
    chrome: MenuChromeState,
) -> (u32, u32, Vec<u8>) {
    let dpi = Dpi::new(dpr);
    let w = dpi.su(layout.window_w);
    let h = dpi.su(layout.window_h);
    let mut out = vec![0u8; (w * h * 4) as usize];

    // L3: scale card from pet center; opacity from open_t (0.92→1 visual scale).
    let t = layout.open_t.clamp(0.0, 1.0);
    let scale = 0.92 + 0.08 * t;
    let pivot_x = layout.pet_x + layout.pet_w * 0.5;
    let pivot_y = layout.pet_y + layout.pet_h * 0.5;
    let layout = scale_layout_from_pivot(layout, pivot_x, pivot_y, scale);

    // Glass card only (rest of union window stays transparent for pin-pet).
    let cx0 = dpi.s(layout.card_x);
    let cy0 = dpi.s(layout.card_y);
    let cx1 = dpi.s(layout.card_x + layout.card_w);
    let cy1 = dpi.s(layout.card_y + layout.card_h);
    let crad = dpi.s(22.0);

    fill_rrect_aa(
        &mut out,
        w,
        h,
        cx0 + dpi.s(8.0),
        cy0 + dpi.s(10.0),
        cx1 + dpi.s(6.0),
        cy1 + dpi.s(8.0),
        crad,
        SHADOW_A,
    );
    fill_rrect_aa(
        &mut out,
        w,
        h,
        cx0 + dpi.s(4.0),
        cy0 + dpi.s(5.0),
        cx1 + dpi.s(3.0),
        cy1 + dpi.s(4.0),
        crad,
        SHADOW_B,
    );
    fill_rrect_aa(
        &mut out,
        w,
        h,
        cx0 + dpi.s(1.5),
        cy0 + dpi.s(2.0),
        cx1 + dpi.s(1.5),
        cy1 + dpi.s(1.5),
        crad,
        SHADOW_C,
    );
    fill_rrect_aa(&mut out, w, h, cx0, cy0, cx1, cy1, crad, CARD);
    // Inner top glass highlight (L4)
    fill_rrect_aa(
        &mut out,
        w,
        h,
        cx0 + dpi.s(2.0),
        cy0 + dpi.s(2.0),
        cx1 - dpi.s(2.0),
        cy0 + dpi.s(18.0),
        dpi.s(12.0),
        INNER_HL,
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
        HAIRLINE,
        1.0,
    );

    let content_x = dpi.s(content_x_from(&layout));
    let content_w = dpi.s(content_w_from(&layout));

    blit_text(
        &mut out,
        w,
        h,
        "快捷启动",
        content_x,
        dpi.s(layout.card_y + 16.0),
        dpi.su(220),
        dpi.px(17.0),
        LABEL,
    );
    blit_text(
        &mut out,
        w,
        h,
        "打开常用应用",
        content_x,
        dpi.s(layout.card_y + 38.0),
        dpi.su(220),
        dpi.px(12.0),
        SECONDARY,
    );

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
    );

    let mut saw_shortcut = false;
    for (i, item) in layout.items.iter().enumerate() {
        let x = dpi.s(item.x);
        let y = dpi.s(item.y);
        let bw = dpi.s(item.w);
        let bh = dpi.s(item.h);
        let radius = dpi.s(12.0);
        let pressed = chrome.press == Some(i);
        let hovered = chrome.hover == Some(i);

        match &item.entry {
            MenuEntry::AddShortcut => {
                let btn = if pressed { BLUE_PRESS } else { BLUE };
                fill_rrect_aa(
                    &mut out,
                    w,
                    h,
                    x,
                    y + dpi.s(1.5),
                    x + bw,
                    y + bh + dpi.s(1.5),
                    radius,
                    SHADOW_A,
                );
                fill_rrect_aa(&mut out, w, h, x, y, x + bw, y + bh, radius, btn);
                fill_rrect_aa(
                    &mut out,
                    w,
                    h,
                    x + dpi.s(2.0),
                    y + dpi.s(1.0),
                    x + bw - dpi.s(2.0),
                    y + dpi.s(5.0),
                    dpi.s(3.0),
                    INNER_HL,
                );
                blit_text_centered(
                    &mut out, w, h, "添加应用", x, y, bw, bh, dpi.px(15.0), WHITE, dpi,
                );
            }
            MenuEntry::Manage => {
                let bg = if pressed {
                    GROUPED_PRESS
                } else if hovered {
                    FILL_HOVER
                } else {
                    FILL_OPAQUE
                };
                fill_rrect_aa(&mut out, w, h, x, y, x + bw, y + bh, radius, bg);
                blit_text_centered(&mut out, w, h, "管理", x, y, bw, bh, dpi.px(14.0), BLUE, dpi);
            }
            MenuEntry::PauseReminder => {
                let bg = if pressed {
                    GROUPED_PRESS
                } else if hovered {
                    FILL_HOVER
                } else {
                    FILL_OPAQUE
                };
                fill_rrect_aa(&mut out, w, h, x, y, x + bw, y + bh, radius, bg);
                let label = if reminder_paused {
                    "恢复提醒"
                } else {
                    "暂停提醒"
                };
                blit_text_centered(&mut out, w, h, label, x, y, bw, bh, dpi.px(14.0), LABEL, dpi);
            }
            MenuEntry::Shortcut { name, valid, .. } => {
                saw_shortcut = true;
                let bg = if !*valid {
                    if hovered || pressed {
                        INVALID_BG_HOVER
                    } else {
                        INVALID_BG
                    }
                } else if pressed {
                    GROUPED_PRESS
                } else if hovered {
                    GROUPED_HOVER
                } else {
                    GROUPED_BG
                };
                fill_rrect_aa(&mut out, w, h, x, y, x + bw, y + bh, radius, bg);

                let icx = (x + dpi.s(22.0)) as i32;
                let icy = (y + bh * 0.5) as i32;
                let disc_r = dpi.s(13.0).round() as i32;
                let disc = if *valid { BLUE } else { ORANGE };
                let disc_d = if *valid { BLUE_PRESS } else { ORANGE };
                fill_soft_disc(
                    &mut out,
                    w,
                    h,
                    icx,
                    icy,
                    disc_r,
                    0.7,
                    disc,
                    disc_d,
                );

                let ch = if *valid {
                    name.chars().next().unwrap_or('A').to_string()
                } else {
                    "!".to_string()
                };
                if let Some((tw, th, tbuf)) = rasterize_text(&ch, dpi.su(24), dpi.px(12.0), WHITE) {
                    let (tx, ty) = center_in_rect(
                        icx as f32 - disc_r as f32,
                        icy as f32 - disc_r as f32,
                        disc_r as f32 * 2.0,
                        disc_r as f32 * 2.0,
                        tw,
                        th,
                        dpi.s(0.5),
                    );
                    blit(&mut out, w, h, &tbuf, tw, th, tx, ty);
                }

                let max_tw = (bw - dpi.s(88.0)).max(8.0) as u32;
                if *valid {
                    if let Some((tw, th, tbuf)) =
                        rasterize_text(name, max_tw, dpi.px(15.0), LABEL)
                    {
                        let ty = (y + (bh - th as f32) * 0.5 + dpi.s(0.5))
                            .round()
                            .max(0.0) as u32;
                        blit(&mut out, w, h, &tbuf, tw, th, (x + dpi.s(42.0)) as u32, ty);
                    }
                } else {
                    // design §7.10: ⚠ name + 无法找到程序
                    if let Some((tw, th, tbuf)) =
                        rasterize_text(name, max_tw, dpi.px(14.0), ORANGE)
                    {
                        let ty = (y + dpi.s(8.0)).round().max(0.0) as u32;
                        blit(&mut out, w, h, &tbuf, tw, th, (x + dpi.s(42.0)) as u32, ty);
                    }
                    if let Some((tw, th, tbuf)) =
                        rasterize_text("无法找到程序 · 点此修复", max_tw, dpi.px(11.0), ORANGE)
                    {
                        let ty = (y + dpi.s(26.0)).round().max(0.0) as u32;
                        blit(&mut out, w, h, &tbuf, tw, th, (x + dpi.s(42.0)) as u32, ty);
                    }
                }

                draw_chevron(
                    &mut out,
                    w,
                    (x + bw - dpi.s(16.0)) as i32,
                    (y + bh * 0.5) as i32,
                    dpi,
                    TERTIARY,
                );
            }
        }
    }

    if !saw_shortcut {
        draw_empty_state(&mut out, w, h, content_x, content_w, &layout, dpi);
    }

    // Fade whole overlay with open_t (card + labels already scaled; pet stays put).
    apply_rgba_alpha(&mut out, t);

    (w, h, out)
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
    }
}

fn apply_rgba_alpha(buf: &mut [u8], a: f32) {
    let a = a.clamp(0.0, 1.0);
    if (a - 1.0).abs() < 0.001 {
        return;
    }
    for px in buf.chunks_exact_mut(4) {
        px[3] = ((px[3] as f32) * a).round().clamp(0.0, 255.0) as u8;
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
) {
    let ey = dpi.s(empty_y(layout) as f32);
    let gx = (content_x + content_w * 0.5) as i32;
    let gy = (ey + dpi.s(12.0)) as i32;
    let r = dpi.s(18.0).round() as i32;

    fill_soft_disc(out, w, h, gx, gy, r, 0.8, FILL_OPAQUE, GROUPED_BG);
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
        SECONDARY,
    );
    fill_rect_f(
        out,
        w,
        h,
        gx as f32 - arm,
        gy as f32 - t * 0.5,
        arm * 2.0,
        t,
        SECONDARY,
    );

    if let Some((tw, th, tbuf)) = rasterize_text("还没有常用应用", dpi.su(220), dpi.px(14.0), LABEL)
    {
        let tx = (content_x + (content_w - tw as f32) * 0.5).round().max(0.0) as u32;
        blit(out, w, h, &tbuf, tw, th, tx, (ey + dpi.s(40.0)) as u32);
    }
    if let Some((tw, th, tbuf)) =
        rasterize_text("点「添加应用」选 exe / 快捷方式", dpi.su(300), dpi.px(12.0), SECONDARY)
    {
        let tx = (content_x + (content_w - tw as f32) * 0.5).round().max(0.0) as u32;
        blit(out, w, h, &tbuf, tw, th, tx, (ey + dpi.s(60.0)) as u32);
    }
}

fn content_x_from(layout: &RadialLayout) -> f32 {
    layout
        .items
        .first()
        .map(|i| i.x)
        .unwrap_or(layout.card_x + 16.0)
}

fn content_w_from(layout: &RadialLayout) -> f32 {
    layout
        .items
        .first()
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
) {
    let px = dpi.s(pet_x);
    let py = dpi.s(pet_y);
    let pw = dpi.s(pet_w);
    let ph = dpi.s(pet_h);
    let rad = dpi.s(18.0);

    fill_rrect_aa(
        out,
        w,
        h,
        px + dpi.s(2.0),
        py + dpi.s(4.0),
        px + pw + dpi.s(2.0),
        py + ph + dpi.s(4.0),
        rad,
        SHADOW_B,
    );
    fill_rrect_aa(out, w, h, px, py, px + pw, py + ph, rad, GROUPED_BG);
    stroke_rrect_aa(
        out,
        w,
        h,
        px + 0.5,
        py + 0.5,
        px + pw - 0.5,
        py + ph - 0.5,
        rad,
        SEPARATOR,
        1.0,
    );

    let max = (pw.min(ph) - dpi.s(40.0)).max(dpi.s(48.0)) as u32;
    let (sw, sh, pet) = scale_rgba_fit(pet_rgba, src_w, src_h, max, max);
    let ox = (px + (pw - sw as f32) * 0.5).round() as u32;
    let oy = (py + (ph - dpi.s(28.0) - sh as f32) * 0.45 + dpi.s(10.0))
        .round()
        .max(py + dpi.s(10.0)) as u32;

    fill_soft_ellipse(
        out,
        w,
        h,
        (px + pw * 0.5) as i32,
        (py + ph - dpi.s(34.0)) as i32,
        dpi.s(30.0) as i32,
        dpi.s(8.0) as i32,
        SHADOW_B,
    );
    blit(out, w, h, &pet, sw, sh, ox, oy);

    if let Some((tw, th, t)) = rasterize_text("轻点关闭", dpi.su(110), dpi.px(11.0), TERTIARY) {
        let tx = (px + (pw - tw as f32) * 0.5).round() as u32;
        let ty = (py + ph - dpi.s(18.0) - th as f32 * 0.5).round() as u32;
        blit(out, w, h, &t, tw, th, tx, ty);
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
        HAIRLINE,
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
        dpi.px(13.0),
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
        dpi.s(14.0),
        GROUPED_BG,
    );
    blit_text(
        &mut out,
        w,
        h,
        "健康提醒",
        dpi.s(36.0),
        dpi.s(REMINDER_CARD_TOP + 12.0),
        dpi.su(160),
        dpi.px(13.0),
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
        dpi.px(13.0),
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
        dpi.px(13.0),
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
        dpi.s(14.0),
        GROUPED_BG,
    );
    blit_text(
        &mut out,
        w,
        h,
        "宠物大小",
        dpi.s(36.0),
        dpi.s(PET_CARD_TOP + 12.0),
        dpi.su(160),
        dpi.px(13.0),
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
        dpi.px(12.0),
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
        dpi.px(13.0),
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
        dpi.s(14.0),
        GROUPED_BG,
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
        if let Some((tw, th, t)) = rasterize_text(&label, dpi.su(200), dpi.px(14.0), color) {
            let ty = (y + (row_h - th as f32) * 0.5 + dpi.s(0.5)).round() as u32;
            blit(&mut out, w, h, &t, tw, th, dpi.s(36.0) as u32, ty);
        }
        let mid_y = (y + row_h * 0.5 - dpi.s(6.0)) as u32;
        trailing_btn(&mut out, w, h, w - dpi.su(168), mid_y, "上移", BLUE, dpi);
        trailing_btn(&mut out, w, h, w - dpi.su(120), mid_y, "下移", BLUE, dpi);
        trailing_btn(&mut out, w, h, w - dpi.su(72), mid_y, "启停", BLUE, dpi);
        trailing_btn(&mut out, w, h, w - dpi.su(32), mid_y, "删除", RED, dpi);
    }

    fill_rrect_aa(
        &mut out,
        w,
        h,
        dpi.s(24.0),
        hf - dpi.s(72.0),
        wf - dpi.s(24.0),
        hf - dpi.s(24.0),
        dpi.s(14.0),
        BLUE,
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
        dpi.px(16.0),
        WHITE,
        dpi,
    );

    (w, h, out)
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
        if local_x >= w - 32.0 - 20.0 {
            return Some(SettingsHit::RowDelete(i));
        }
        if local_x >= w - 72.0 - 16.0 {
            return Some(SettingsHit::RowToggle(i));
        }
        if local_x >= w - 120.0 - 16.0 {
            return Some(SettingsHit::RowDown(i));
        }
        if local_x >= w - 168.0 - 16.0 {
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
    if let Some((tw, th, t)) = rasterize_text(label, dpi.su(48), dpi.px(12.0), color) {
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
