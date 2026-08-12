//! Appica-inspired quick-launch dock + manager UI (warm glass · pin-pet).
//!
//! Drawn at **device pixel ratio** so text/edges stay sharp on HiDPI
//! (no low-res compose → bilinear upscale blur). No system Acrylic.

use crate::render::easing::{ease_out_cubic, ease_out_quint, lerp, stagger_t};
use crate::render::text::{center_in_rect, rasterize_text};
use crate::shortcut::{scale_icon_rgba, IconRgba, IconShape};
use crate::ui::radial_menu::{MenuEntry, RadialLayout};

// ── Palette (Appica-warm · design §2 · no system Acrylic) ─────────────────
/// Near-white card ~92% alpha with warm tint.
const CARD: [u8; 4] = [0xFF, 0xFB, 0xFA, 0xEB];
/// Soft muted surfaces (rows / chips).
const GROUPED_BG: [u8; 4] = [0xF4, 0xF2, 0xF0, 0xD0];
const GROUPED_HOVER: [u8; 4] = [0xFF, 0xFF, 0xFF, 0xE8];
const GROUPED_PRESS: [u8; 4] = [0xEE, 0xE8, 0xF0, 0xF0];
const INVALID_BG: [u8; 4] = [0xFF, 0xB0, 0x20, 0x30];
const INVALID_BG_HOVER: [u8; 4] = [0xFF, 0xB0, 0x20, 0x48];
const HIGHLIGHT_ROW: [u8; 4] = [0xFF, 0x9E, 0xC4, 0x38];
const SEPARATOR: [u8; 4] = [0xE2, 0xE0, 0xDE, 0x90];
const BORDER: [u8; 4] = [0x1E, 0x1B, 0x2E, 0x18];
const HAIRLINE: [u8; 4] = [0xFF, 0xFF, 0xFF, 0x70];
const INNER_HL: [u8; 4] = [0xFF, 0xFF, 0xFF, 0x55];
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

    // ── Silk motion (card only; pet stays pinned at full opacity) ──────────
    // open_t is a **linear** 0..1 clock from pet::tick_menu_anim.
    // Scale/fade curves are continuous for any path of t (open or close).
    let t_clock = layout.open_t.clamp(0.0, 1.0);
    let t_scale = ease_out_quint(t_clock);
    // Fade leads scale slightly so glass materializes before full size.
    let t_fade = ease_out_cubic((t_clock * 1.22).min(1.0));
    // Shadow softens in with a longer tail (no harsh blob on first frames).
    let t_shadow = (t_fade * t_fade).clamp(0.0, 1.0);

    // Grow from pet center — gentle start scale (0.90→1), no overshoot, no extra y.
    let scale = 0.90 + 0.10 * t_scale;
    let pivot_x = layout.pet_x + layout.pet_w * 0.5;
    let pivot_y = layout.pet_y + layout.pet_h * 0.5;
    let layout = scale_layout_from_pivot(layout, pivot_x, pivot_y, scale);

    // Elevated glass card (rest of union window stays transparent for pin-pet).
    let cx0 = dpi.s(layout.card_x);
    let cy0 = dpi.s(layout.card_y);
    let cx1 = dpi.s(layout.card_x + layout.card_w);
    let cy1 = dpi.s(layout.card_y + layout.card_h);
    let crad = dpi.s(18.0);

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
    }

    let content_x = dpi.s(content_x_from(&layout));
    let content_w = dpi.s(content_w_from(&layout));

    // Title: soft fade with card (no extra vertical jump).
    let title_a = t_fade;
    if title_a > 0.02 {
        blit_text(
            &mut out,
            w,
            h,
            "快捷启动",
            content_x,
            dpi.s(layout.card_y + 15.0),
            dpi.su(220),
            dpi.px(17.0),
            with_alpha(LABEL, title_a),
        );
        blit_text(
            &mut out,
            w,
            h,
            "打开常用应用",
            content_x,
            dpi.s(layout.card_y + 36.0),
            dpi.su(220),
            dpi.px(12.5),
            with_alpha(SECONDARY, title_a),
        );
    }

    // Pet always full opacity. Plate / "轻点关闭" fades in with the card so the
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

    // Controls cascade after card materializes (~12% delay), soft dy.
    let content_clock = ((t_clock - 0.10) / 0.90).clamp(0.0, 1.0);
    let mut saw_shortcut = false;
    for (i, item) in layout.items.iter().enumerate() {
        let reveal = stagger_t(content_clock, i, 0.028, 0.78) * t_fade;
        if reveal <= 0.01 {
            continue;
        }
        let item_dy = (1.0 - stagger_t(content_clock, i, 0.028, 0.78)) * dpi.s(5.0);
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
        let radius = dpi.s(11.0);

        match &item.entry {
            MenuEntry::AddShortcut => {
                draw_primary_btn(
                    &mut out,
                    w,
                    h,
                    dpi,
                    x,
                    y,
                    bw,
                    bh,
                    radius,
                    "添加应用",
                    hover_w,
                    press_w,
                    reveal,
                );
            }
            MenuEntry::Manage => {
                draw_soft_btn(
                    &mut out,
                    w,
                    h,
                    dpi,
                    x,
                    y,
                    bw,
                    bh,
                    radius,
                    "管理",
                    LABEL,
                    hover_w,
                    press_w,
                    reveal,
                );
            }
            MenuEntry::PauseReminder => {
                let label = if reminder_paused {
                    "恢复提醒"
                } else {
                    "暂停提醒"
                };
                draw_soft_btn(
                    &mut out,
                    w,
                    h,
                    dpi,
                    x,
                    y,
                    bw,
                    bh,
                    radius,
                    label,
                    LABEL,
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
            t_fade * ease_out_cubic(content_clock),
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
        let hy = dpi.s(layout.list_bottom + 4.0).max(dpi.s(layout.card_y + layout.card_h - 20.0));
        blit_text(
            &mut out,
            w,
            h,
            &hint,
            content_x,
            hy,
            dpi.su(280),
            dpi.px(11.0),
            with_alpha(TERTIARY, t_fade * 0.95),
        );
    }

    // No global alpha — pet stays solid; card/controls already use per-layer fade.

    (w, h, out)
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

fn draw_soft_btn(
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
    text: [u8; 4],
    hover_w: f32,
    press_w: f32,
    reveal: f32,
) {
    let bg0 = FILL_OPAQUE;
    let bg1 = FILL_HOVER;
    let bg2 = GROUPED_PRESS;
    let bg = with_alpha(
        lerp_rgba(lerp_rgba(bg0, bg1, hover_w), bg2, press_w),
        reveal,
    );
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
        with_alpha(SOFT_BORDER, reveal),
        1.0,
    );
    // Soft top highlight on hover
    if hover_w > 0.05 {
        fill_rrect_aa(
            out,
            w,
            h,
            x + dpi.s(2.0),
            y + dpi.s(1.0),
            x + bw - dpi.s(2.0),
            y + dpi.s(4.0),
            dpi.s(3.0),
            with_alpha(INNER_HL, reveal * hover_w * 0.7),
        );
    }
    blit_text_centered(
        out,
        w,
        h,
        label,
        x,
        y,
        bw,
        bh,
        dpi.px(13.5),
        with_alpha(text, reveal),
        dpi,
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
        with_alpha(TERTIARY, reveal),
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
        rasterize_text("还没有常用应用", dpi.su(220), dpi.px(14.0), with_alpha(LABEL, reveal))
    {
        let tx = (content_x + (content_w - tw as f32) * 0.5).round().max(0.0) as u32;
        blit(out, w, h, &tbuf, tw, th, tx, (ey + dpi.s(44.0)) as u32);
    }
    if let Some((tw, th, tbuf)) = rasterize_text(
        "点「添加应用」选 exe / 快捷方式",
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
            rasterize_text("轻点关闭", dpi.su(110), dpi.px(11.0), with_alpha(TERTIARY, plate_a))
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
