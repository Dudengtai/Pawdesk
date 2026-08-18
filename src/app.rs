//! Application lifecycle (M0–M4).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tracing::{debug, error, info, warn};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{
    ElementState, MouseButton as WinitMouseButton, MouseScrollDelta, StartCause, WindowEvent,
};
use winit::keyboard::{Key, NamedKey};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId, WindowLevel};

use crate::config::{AppConfig, ConfigRepository, DebouncedSaver, PET_SCALE_MAX};
use crate::error::AppError;
use crate::event::{AppEvent, Point, TrayCommand};
use crate::pet::{
    hop_arc_height, pet_logical_size, AnimationLibrary, PetController, PetState, ReminderStage,
    REMINDER_WINDOW_H, REMINDER_WINDOW_W,
};
use crate::platform;
use crate::reminder::{now_rfc3339, pick_message, ReminderScheduler};
use crate::render::easing::{ease_in_out_cubic, ease_out_cubic};
use crate::render::menu_ui::{
    blit_rgba, blit_rgba_clipped, compose_menu_card_layer, compose_menu_frame,
    compose_menu_pet_only, compose_pet_in_slot, compose_settings_card,
    compose_settings_frame, draw_say_bubble, hit_settings,
    hit_settings_card, menu_visual_fade, menu_visual_scale, prerender_drag_images,
    prerender_list_rows, present_menu_cached, present_menu_drag, MenuChromeState,
    MenuDragChrome, SettingsHit, SAY_EATEN, SAY_FAIL, SAY_NO_PAUSE, SETTINGS_H,
    SETTINGS_W,
};
use crate::render::sample_rgba_bilinear;
use crate::render::reminder_ui::{
    compose_reminder_card_frame, compose_reminder_frame, compose_reminder_overlay,
    food_button_layout, load_feed_bowl, load_reminder_card, FeedBowl, OverlayCardBlit,
    ReminderCard,
};
use crate::render::yawn_bubble::{compose_yawn_frame, place_yawn_bubble, YawnPlacement};
// Present path uses CPU + UpdateLayeredWindow only (no wgpu surface on the pet HWND).
// Attaching a DXGI/Vulkan swapchain to a WS_EX_LAYERED window breaks per-pixel alpha.
use crate::shortcut::{
    build_pick_context, extract_icon, launch, pick_executable, IconRgba, ShortcutItem,
    ShortcutRepository,
};
use crate::ui::launcher_place::{
    infer_attach_dir, logical_to_physical, overlay_pad_for_max_pet, physical_to_logical,
    physical_to_logical_u32, place_launcher, snap_dpr, DEFAULT_GAP, DEFAULT_MARGIN,
    WINDOW_PADDING,
};
use crate::ui::list_drag::{
    bowl_rect, edge_scroll_delta, insert_index_from_y, pointer_dist, reorder_ids, should_start_drag,
    ListDrag, SLOP_PX,
};
use crate::ui::reminder_place::{place_reminder_travel, ReminderPlacement};
use crate::ui::pet_window::DragState;
use crate::ui::radial_menu::{
    self, build_entries, clamp_list_scroll, count_shortcuts, hit_center, hit_test_index,
    layout_pinned_scroll, MenuEntry, RadialLayout, CARD_LOGICAL_H, CARD_LOGICAL_W, MENU_WINDOW_H,
    MENU_WINDOW_W, ROW_GAP, ROW_H,
};
use crate::ui::tray::TrayHandle;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum UserEvent {
    App(AppEvent),
    /// Async file-dialog result (never block UI thread with rfd).
    FilePicked(Option<PathBuf>),
}

/// Dock geometry as it was when settings opened — restored if the user
/// discards a pet-size preview (Esc) instead of committing with 「完成」.
#[derive(Debug, Clone)]
struct SettingsLayoutSnapshot {
    layout: RadialLayout,
    present_pos: (i32, i32),
    logical_size: (u32, u32),
}

/// In-card launcher ↔ settings slide (physical pixels stay put).
#[derive(Debug, Clone, Copy)]
struct SettingsTransition {
    started: Instant,
    duration: Duration,
    /// Linear 0..1 clock; advanced by `about_to_wait`, read by compose.
    t: f32,
    /// true = dock → settings (in from right); false = settings → dock.
    entering: bool,
}

/// Persistent dock HWND box. Size stays put across open/close so layered
/// present never has to `SetWindowPos`-resize (that wipe is the cat flash).
#[derive(Debug, Clone, Copy)]
struct DockHwnd {
    pos: (i32, i32),
    phys: (u32, u32),
    win_log_w: u32,
    win_log_h: u32,
    pet_x: f32,
    pet_y: f32,
    pet_w: f32,
    pet_h: f32,
}

/// Re-composition key for the cached drag layers — the static card (rows
/// blanked) + pre-rendered row bitmaps. They only change when the scroll window
/// (or the frozen chrome) changes; insertion-slot changes just re-blit rows.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DragLayersKey {
    scroll: usize,
    total: usize,
    hover: Option<usize>,
    press: Option<usize>,
    say: Option<&'static str>,
}

/// Rebuild the static drag layers (blank card + row bitmaps) only when the key
/// changed. Takes the layers field directly so callers can keep other borrows.
fn ensure_drag_layers(
    layers: &mut Option<(DragLayersKey, (u32, u32, Vec<u8>), Vec<(u32, u32, Vec<u8>)>)>,
    layout: &RadialLayout,
    dpr: f32,
    chrome: &MenuChromeState,
    key: DragLayersKey,
) {
    if layers.as_ref().map(|(k, _, _)| k) == Some(&key) {
        return;
    }
    let mut blank = chrome.clone();
    blank.drag = None;
    blank.drag_draft = false;
    blank.rows_blank = true;
    let (w, h, base) = compose_menu_card_layer(layout, dpr, blank);
    let rows = prerender_list_rows(layout, dpr);
    *layers = Some((key, (w, h, base), rows));
}

pub struct App {
    window: Option<Arc<Window>>,
    tray: Option<TrayHandle>,
    pet: Option<PetController>,
    config: AppConfig,
    saver: Option<DebouncedSaver>,
    scheduler: Option<ReminderScheduler>,
    drag: DragState,
    cursor_in_window: Point,
    assets_dir: PathBuf,
    sprite_logical: (u32, u32),
    visible: bool,
    exit_requested: bool,
    last_frame: Instant,
    click_through: bool,
    texture_dirty: bool,
    last_clip_name: String,
    scale_factor: f64,
    /// True while the reminder travel overlay is up (hop + card + return).
    reminder_ui_active: bool,
    /// Overlay geometry for the current reminder (physical px).
    reminder_place: Option<ReminderPlacement>,
    /// Overlay HWND top-left for atomic ULW (physical).
    reminder_present_pos: Option<(i32, i32)>,
    /// Pet slot top-left inside the overlay (physical).
    reminder_slot: Option<Point>,
    /// 0 = pet only, 1 = card only.
    reminder_card_t: f32,
    /// Card fade: (started, from, to, duration).
    reminder_card_anim: Option<(Instant, f32, f32, Duration)>,
    /// Whole-image reminder card (tishi.png mockup), None → composed fallback.
    reminder_card: Option<ReminderCard>,
    /// Kibble bowl used as the feed control.
    feed_bowl: Option<FeedBowl>,
    /// Feed completed this session cycle; persist once when return starts.
    feed_persist_pending: bool,
    /// M4 shortcuts.
    shortcuts: ShortcutRepository,
    /// Extracted app icons, keyed by target path (None = extraction failed once).
    shortcut_icons: HashMap<PathBuf, Option<Arc<IconRgba>>>,
    /// Expanded radial menu UI.
    menu_ui_active: bool,
    /// Low-level mouse hook while the launcher is open (outside click → close).
    menu_outside_guard: Option<platform::OutsideClickGuard>,
    /// Suppress re-opening launcher immediately after close (accidental double-click).
    menu_reopen_after: Option<Instant>,
    /// Settings overlay (reminder + pet size).
    settings_ui_active: bool,
    /// Last radial layout for hit-testing (menu-local **logical** coords).
    menu_layout: Option<RadialLayout>,
    /// Dynamic menu window size in logical px (hit mapping).
    menu_logical_size: (u32, u32),
    /// Menu item hover / press (layout.items index).
    menu_hover: Option<usize>,
    menu_press: Option<usize>,
    /// Animated 0..1 blends for Appica-style hover/press microinteractions.
    menu_hover_t: f32,
    menu_press_t: f32,
    /// Overlay window top-left (physical) for atomic layered present during menu open.
    menu_present_pos: Option<(i32, i32)>,
    /// Dock-sized HWND kept after close. Growing/shrinking a layered window
    /// discards the ULW bitmap; DWM then composites an empty frame (the flash).
    /// Open/close after the first grow only change the bitmap.
    dock_hwnd: Option<DockHwnd>,
    /// Rest-state card layer (physical, no pet) for open/close scale+fade.
    menu_card_cache: Option<(u32, u32, Vec<u8>)>,
    /// Shortcut list scroll (first visible row index). Supports many apps via wheel.
    menu_list_scroll: usize,
    /// Rare speech (launch failure). Success closes immediately.
    menu_say: Option<(Instant, &'static str)>,
    /// Long-press reorder / feed-to-delete on the dock list.
    list_drag: ListDrag,
    /// Snapshot used to paint the lifted row (name / icon).
    list_drag_visual: Option<MenuDragChrome>,
    /// Throttle auto-scroll while dragging near the list edge.
    list_drag_edge_at: Option<Instant>,
    /// Static drag layers: card (rows blanked) + pre-rendered rows for the
    /// current scroll window. Insertion-slot changes never rebuild these —
    /// [`present_menu_drag`] re-blits the rows at shifted positions.
    menu_drag_layers: Option<(DragLayersKey, (u32, u32, Vec<u8>), Vec<(u32, u32, Vec<u8>)>)>,
    /// Settings grows from the launcher's Manage button instead of snapping center.
    settings_transition: Option<SettingsTransition>,
    /// Overlay window top-left (physical) for atomic layered present during settings transition.
    settings_present_pos: Option<(i32, i32)>,
    /// Settings lives inside the dock card (launcher button), not a new window.
    settings_embed: bool,
    /// Card-sized settings snapshot for the slide.
    settings_card_cache: Option<(u32, u32, Vec<u8>)>,
    /// Uncommitted pet-size preview while settings is open (None = use config).
    pet_scale_draft: Option<f32>,
    /// Overlay + pet slot as of settings entry, so Esc can undo a live resize.
    settings_layout_snapshot: Option<SettingsLayoutSnapshot>,
    /// Expanded comic-bubble window while `idle_yawn` plays.
    yawn_ui_active: bool,
    yawn_place: Option<YawnPlacement>,
    yawn_present_pos: Option<(i32, i32)>,
    /// Pet position before menu/settings expand (window top-left, physical).
    overlay_origin: Option<Point>,
    /// Idle present lock: keep ULW at this screen pos + `pet_size()` until
    /// winit's async resize catches up. Prevents the one-frame jump when an
    /// overlay HWND is torn down (or the pet scale changes) and `inner_size`
    /// still belongs to the previous window.
    idle_present_pos: Option<(i32, i32)>,
    /// Current pet frame RGBA for alpha hit-testing (normal pet size).
    hit_rgba: Vec<u8>,
    hit_size: (u32, u32),
    _bg_rx: Option<std::sync::mpsc::Receiver<AppEvent>>,
    /// Proxy for background threads → UI (file picker, etc.).
    event_proxy: Option<EventLoopProxy<UserEvent>>,
    /// True while native file dialog is open on a worker thread.
    file_picker_busy: bool,
    /// If set, the next file pick repairs this shortcut instead of adding one.
    file_picker_repair: Option<Uuid>,
}

impl App {
    pub fn new(assets_dir: PathBuf, mut config: AppConfig, mut saver: DebouncedSaver) -> Self {
        let (_bg_tx, bg_rx) = std::sync::mpsc::channel();
        drop(_bg_tx);
        let now = Instant::now();
        // Pause was removed as a product feature; leftover configs must not
        // leave the reminder stuck silent.
        if config.reminder.paused {
            config.reminder.paused = false;
            saver.mark_dirty();
        }
        let interval = ReminderScheduler::resolve_interval(config.reminder.interval_minutes);
        let mut scheduler = ReminderScheduler::new(
            config.reminder.enabled,
            false,
            interval,
            now,
        );
        scheduler.apply_startup_catchup(config.reminder.last_completed_at.as_deref(), now);
        let had_disabled = config.shortcuts.iter().any(|s| !s.enabled);
        let shortcuts = ShortcutRepository::from_items(config.shortcuts.clone());
        if had_disabled {
            config.shortcuts = shortcuts.list_sorted();
            saver.mark_dirty();
        }
        let reminder_card = load_reminder_card(
            &assets_dir.join("ui/reminder_card.png"),
            REMINDER_WINDOW_W,
            REMINDER_WINDOW_H,
        );
        match &reminder_card {
            Some(_) => info!("reminder card loaded (tishi.png mockup)"),
            None => warn!("reminder card image missing; using composed reminder UI"),
        }
        let feed_bowl = load_feed_bowl(&assets_dir.join("ui/feed_bowl.png"));
        match &feed_bowl {
            Some(_) => info!("feed bowl loaded"),
            None => warn!("feed bowl missing; reminder will use a placeholder"),
        }

        Self {
            window: None,
            tray: None,
            pet: None,
            config,
            saver: Some(saver),
            scheduler: Some(scheduler),
            drag: DragState::default(),
            cursor_in_window: Point::new(0.0, 0.0),
            assets_dir,
            sprite_logical: (128, 128),
            visible: true,
            exit_requested: false,
            last_frame: Instant::now(),
            click_through: false,
            texture_dirty: true,
            last_clip_name: String::new(),
            scale_factor: 1.0,
            reminder_ui_active: false,
            reminder_place: None,
            reminder_present_pos: None,
            reminder_slot: None,
            reminder_card_t: 0.0,
            reminder_card_anim: None,
            reminder_card,
            feed_bowl,
            feed_persist_pending: false,
            shortcuts,
            shortcut_icons: HashMap::new(),
            menu_ui_active: false,
            menu_outside_guard: None,
            menu_reopen_after: None,
            settings_ui_active: false,
            menu_layout: None,
            menu_logical_size: (MENU_WINDOW_W, MENU_WINDOW_H),
            menu_hover: None,
            menu_press: None,
            menu_hover_t: 0.0,
            menu_press_t: 0.0,
            menu_present_pos: None,
            dock_hwnd: None,
            menu_card_cache: None,
            menu_list_scroll: 0,
            menu_say: None,
            list_drag: ListDrag::Idle,
            list_drag_visual: None,
            list_drag_edge_at: None,
            menu_drag_layers: None,
            settings_transition: None,
            settings_present_pos: None,
            settings_embed: false,
            settings_card_cache: None,
            pet_scale_draft: None,
            settings_layout_snapshot: None,
            yawn_ui_active: false,
            yawn_place: None,
            yawn_present_pos: None,
            overlay_origin: None,
            idle_present_pos: None,
            hit_rgba: Vec::new(),
            hit_size: (128, 128),
            _bg_rx: Some(bg_rx),
            event_proxy: None,
            file_picker_busy: false,
            file_picker_repair: None,
        }
    }

    pub fn run() -> Result<(), AppError> {
        init_logging()?;

        info!("PawDesk starting (M4: shortcuts + radial menu)");
        let assets_dir = asset_root();
        info!(path = %assets_dir.display(), "assets root");

        let repo = ConfigRepository::default_paths()?;
        info!(path = %repo.config_path().display(), "config path");
        let config = repo.load();
        info!(
            pet_scale = config.pet.scale,
            pet_logical = pet_logical_size(config.pet.scale),
            schema = config.schema_version,
            "pet display size from config"
        );
        // Persist migrated fields (e.g. schema v3 scale) immediately so a crash
        // or old process cannot leave disk on the pre-migration value.
        if let Err(e) = repo.save(&config) {
            warn!(error = %e, "failed to persist config after load/migrate");
        }
        let saver = DebouncedSaver::new(repo);

        if let Ok(monitors) = platform::list_monitors_approx() {
            for m in &monitors {
                info!(
                    name = %m.name,
                    work = ?m.work_area,
                    primary = m.is_primary,
                    "monitor"
                );
            }
        }

        let event_loop = EventLoop::<UserEvent>::with_user_event()
            .build()
            .map_err(|e| AppError::Platform(format!("create event loop: {e}")))?;

        let mut app = App::new(assets_dir, config, saver);
        app.event_proxy = Some(event_loop.create_proxy());
        event_loop
            .run_app(&mut app)
            .map_err(|e| AppError::Platform(format!("event loop error: {e}")))?;

        info!("PawDesk exited cleanly");
        Ok(())
    }

    /// Pet window edge in logical px (design 128 × config scale).
    fn pet_size(&self) -> u32 {
        pet_logical_size(self.config.pet.scale)
    }

    /// Desk-spot of the pet (physical), not the dock HWND origin.
    fn pet_desk_origin(&self) -> Option<Point> {
        if let Some(d) = self.dock_hwnd {
            let dpr = snap_dpr(self.scale_factor);
            let lx = (d.pet_x as f64 * dpr).round();
            let ly = (d.pet_y as f64 * dpr).round();
            return Some(Point::new(d.pos.0 as f64 + lx, d.pos.1 as f64 + ly));
        }
        self.window.as_ref().and_then(|w| {
            w.outer_position()
                .ok()
                .map(|p| Point::new(p.x as f64, p.y as f64))
        })
    }

    fn remember_dock_hwnd(&mut self, pos: (i32, i32), phys: (u32, u32), layout: &RadialLayout) {
        self.dock_hwnd = Some(DockHwnd {
            pos,
            phys,
            win_log_w: layout.window_w,
            win_log_h: layout.window_h,
            pet_x: layout.pet_x,
            pet_y: layout.pet_y,
            pet_w: layout.pet_w,
            pet_h: layout.pet_h,
        });
    }

    fn clear_dock_hwnd(&mut self) {
        self.dock_hwnd = None;
    }

    /// Copy the on-screen pet slot into `dock_hwnd`. The HWND box may grow
    /// to the current overlay (settings size preview) but never shrink —
    /// shrinking discards the layered bitmap and flashes on the next open.
    fn sync_dock_hwnd_slot_from_layout(&mut self) {
        let Some(layout) = self.menu_layout.as_ref() else {
            return;
        };
        let Some(dock) = self.dock_hwnd.as_mut() else {
            return;
        };
        if let Some(pos) = self.menu_present_pos {
            dock.pos = pos;
        }
        dock.pet_x = layout.pet_x;
        dock.pet_y = layout.pet_y;
        dock.pet_w = layout.pet_w;
        dock.pet_h = layout.pet_h;
        let dpr = snap_dpr(self.scale_factor);
        let new_phys = (
            logical_to_physical(layout.window_w, dpr).max(1) as u32,
            logical_to_physical(layout.window_h, dpr).max(1) as u32,
        );
        if new_phys.0 > dock.phys.0 || new_phys.1 > dock.phys.1 {
            dock.phys = (dock.phys.0.max(new_phys.0), dock.phys.1.max(new_phys.1));
            dock.win_log_w = dock.win_log_w.max(layout.window_w);
            dock.win_log_h = dock.win_log_h.max(layout.window_h);
        }
    }

    /// Grow the overlay so a 100% pet still fits against the locked card.
    /// Card and current pet **screen** rects stay put. Returns true if the
    /// canvas grew (callers should rebuild caches / sync the HWND).
    fn ensure_overlay_fits_max_pet(&mut self) -> bool {
        let Some(pos) = self.menu_present_pos else {
            return false;
        };
        let Some(layout) = self.menu_layout.as_ref() else {
            return false;
        };
        let dpr = snap_dpr(self.scale_factor);
        let window = platform::Rect {
            x: pos.0,
            y: pos.1,
            width: logical_to_physical(layout.window_w, dpr),
            height: logical_to_physical(layout.window_h, dpr),
        };
        let card = Self::layout_rect_on_screen(
            pos,
            layout.card_x,
            layout.card_y,
            layout.card_w,
            layout.card_h,
            dpr,
        );
        let pet = Self::layout_rect_on_screen(
            pos,
            layout.pet_x,
            layout.pet_y,
            layout.pet_w,
            layout.pet_h,
            dpr,
        );
        let dir = infer_attach_dir(pet, card);
        let max_pet = logical_to_physical(pet_logical_size(PET_SCALE_MAX), dpr);
        let pad = overlay_pad_for_max_pet(
            window,
            card,
            pet,
            dir,
            max_pet,
            DEFAULT_GAP,
            WINDOW_PADDING,
        );
        if pad.is_zero() {
            return false;
        }
        let dx_log = physical_to_logical(-pad.origin_dx, dpr);
        let dy_log = physical_to_logical(-pad.origin_dy, dpr);
        let new_phys_w = (window.width + pad.extra_w).max(1);
        let new_phys_h = (window.height + pad.extra_h).max(1);
        let new_log_w = physical_to_logical_u32(new_phys_w, dpr);
        let new_log_h = physical_to_logical_u32(new_phys_h, dpr);
        let new_pos = (pos.0 + pad.origin_dx, pos.1 + pad.origin_dy);
        let Some(layout) = self.menu_layout.as_mut() else {
            return false;
        };
        layout.translate(dx_log, dy_log);
        layout.window_w = new_log_w;
        layout.window_h = new_log_h;
        layout.window = new_log_w;
        self.menu_present_pos = Some(new_pos);
        self.menu_logical_size = (new_log_w, new_log_h);
        if let Some(g) = &self.menu_outside_guard {
            g.update_rect(platform::Rect {
                x: new_pos.0,
                y: new_pos.1,
                width: new_phys_w,
                height: new_phys_h,
            });
        }
        self.sync_dock_hwnd_slot_from_layout();
        true
    }

    /// Safety net after 「完成」: if the pet slot hangs off the overlay
    /// (pad was skipped), grow the canvas toward the overflow. Never shrinks.
    /// Returns true when the canvas grew.
    fn grow_overlay_to_contain_pet_slot(&mut self) -> bool {
        let Some(layout) = self.menu_layout.as_ref() else {
            return false;
        };
        let dpr = snap_dpr(self.scale_factor);
        let pad_l = (-layout.pet_x).max(0.0);
        let pad_t = (-layout.pet_y).max(0.0);
        let overflow_r = (layout.pet_x + layout.pet_w - layout.window_w as f32).max(0.0);
        let overflow_b = (layout.pet_y + layout.pet_h - layout.window_h as f32).max(0.0);
        if pad_l < 0.01 && pad_t < 0.01 && overflow_r < 0.01 && overflow_b < 0.01 {
            return false;
        }
        let Some(layout) = self.menu_layout.as_mut() else {
            return false;
        };
        layout.translate(pad_l, pad_t);
        let new_w = ((layout.window_w as f32) + pad_l + overflow_r)
            .round()
            .max(1.0) as u32;
        let new_h = ((layout.window_h as f32) + pad_t + overflow_b)
            .round()
            .max(1.0) as u32;
        layout.window_w = new_w;
        layout.window_h = new_h;
        layout.window = new_w;
        if let Some(pos) = self.menu_present_pos.as_mut() {
            // Do not use `logical_to_physical` here: it clamps 0 → 1.
            pos.0 -= (pad_l as f64 * dpr).round() as i32;
            pos.1 -= (pad_t as f64 * dpr).round() as i32;
        }
        self.menu_logical_size = (new_w, new_h);
        if let (Some(g), Some(pos)) = (&self.menu_outside_guard, self.menu_present_pos) {
            g.update_rect(platform::Rect {
                x: pos.0,
                y: pos.1,
                width: logical_to_physical(new_w, dpr),
                height: logical_to_physical(new_h, dpr),
            });
        }
        true
    }

    /// Scale shown/persisted right now: the settings preview draft when one is
    /// pending, otherwise the committed config value.
    fn effective_pet_scale(&self) -> f32 {
        self.pet_scale_draft.unwrap_or(self.config.pet.scale)
    }

    /// Grow/shrink the dock pet draw-rect in place. Does not move the overlay
    /// window or the card — that would jitter the cat on 「完成」.
    fn sync_menu_pet_slot_to_effective_scale(&mut self) {
        let s = pet_logical_size(self.effective_pet_scale()) as f32;
        let Some(lay) = self.menu_layout.as_mut() else {
            return;
        };
        if (lay.pet_w - s).abs() < 0.01 && (lay.pet_h - s).abs() < 0.01 {
            return;
        }
        lay.pet_w = s;
        lay.pet_h = s;
    }

    fn create_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), AppError> {
        let logical = self.pet_size();
        info!(
            pet_scale = self.config.pet.scale,
            logical,
            "creating pet window"
        );
        let attrs = Window::default_attributes()
            .with_title("PawDesk")
            .with_inner_size(LogicalSize::new(logical, logical))
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(false)
            .with_visible(true)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_active(false);

        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .map_err(|e| AppError::Platform(format!("create window: {e}")))?,
        );

        self.scale_factor = window.scale_factor();
        // Force physical size after create — some DPI paths ignore initial LogicalSize.
        self.window = Some(window.clone());
        self.resize_pet_window(logical, logical);
        let inner = window.inner_size();
        info!(
            dpr = self.scale_factor,
            logical,
            phys_w = inner.width,
            phys_h = inner.height,
            "pet window size applied"
        );

        // Critical on Windows: without DWM/layered setup the transparent pet window
        // composites as a solid white (or black) square even when textures have alpha.
        if let Err(e) = platform::enable_transparent_window(window.as_ref()) {
            warn!("enable_transparent_window: {e}");
        }

        let size = window.outer_size();
        let (default_x, default_y) = if let Ok(wa) = platform::primary_work_area() {
            (
                wa.x + wa.width - size.width as i32 - 48,
                wa.y + wa.height - size.height as i32 - 48,
            )
        } else {
            (100, 100)
        };

        let (x, y) = match (self.config.window.x, self.config.window.y) {
            (Some(cx), Some(cy)) => {
                if let Ok(wa) = platform::primary_work_area() {
                    platform::clamp_position_to_work_area(
                        cx,
                        cy,
                        size.width as i32,
                        size.height as i32,
                        wa,
                    )
                } else {
                    (cx, cy)
                }
            }
            _ => (default_x, default_y),
        };
        window.set_outer_position(PhysicalPosition::new(x, y));
        self.config.window.x = Some(x);
        self.config.window.y = Some(y);

        if let Err(e) = platform::ensure_topmost(window.as_ref()) {
            warn!("ensure_topmost: {e}");
        }

        let pet_dir = self.assets_dir.join("pets/cow-cat");
        let library = AnimationLibrary::load_all(&pet_dir);
        let now = Instant::now();
        let pet = PetController::new(library, now);
        let clip = pet.active_clip();
        self.sprite_logical = (clip.frame_width, clip.frame_height);
        self.last_clip_name = clip.name.clone();

        let first_frame = clip.frame_rgba(0);

        // Do NOT attach a wgpu/DXGI surface to this HWND — it fights UpdateLayeredWindow
        // and produces a solid magenta/white square instead of a silhouette.

        let tray_icon = self.assets_dir.join("tray/icon.png");
        let tray = TrayHandle::new(&tray_icon)?;

        info!(
            logical = logical,
            pet_scale = self.config.pet.scale,
            dpr = self.scale_factor,
            anim = %clip.name,
            "pet window + layered present + tray ready (M4)"
        );

        self.hit_rgba = first_frame;
        self.hit_size = (clip.frame_width, clip.frame_height);
        // window already stored before resize
        self.tray = Some(tray);
        self.pet = Some(pet);
        self.last_frame = Instant::now();
        self.texture_dirty = true;

        // First paint: silhouette via UpdateLayeredWindow.
        self.redraw();
        Ok(())
    }

    /// Cached icon for a shortcut (lazy extract; None cached for failures).
    fn shortcut_icon(&mut self, item: &ShortcutItem) -> Option<Arc<IconRgba>> {
        if !item.is_path_valid() {
            return None;
        }
        let key = item.target_path.clone();
        if let Some(hit) = self.shortcut_icons.get(&key) {
            return hit.clone();
        }
        let icon = extract_icon(&key).map(Arc::new);
        self.shortcut_icons.insert(key, icon.clone());
        icon
    }

    fn sync_texture_from_pet(&mut self) {
        // Keep the dock pet slot at the size the user last chose (preview
        // draft, or the committed scale after 「完成」). Window / card stay
        // locked — only the draw rect grows from the slot's top-left — so
        // the launcher does not snap back to the open-time size.
        if self.menu_ui_active {
            self.sync_menu_pet_slot_to_effective_scale();
        }
        // Warm the icon cache and snapshot icons before borrowing `self.pet`
        // (extraction needs `&mut self`; the pet borrow below would block it).
        let menu_animating = self
            .pet
            .as_ref()
            .map(|p| p.is_menu_animating())
            .unwrap_or(false);
        let menu_icons: HashMap<Uuid, Option<Arc<IconRgba>>> = if (self.menu_ui_active
            && !menu_animating)
            || (self.settings_ui_active && self.settings_transition.is_some())
        {
            self.shortcuts
                .list_enabled_sorted()
                .into_iter()
                .map(|s| (s.id, self.shortcut_icon(&s)))
                .collect()
        } else {
            HashMap::new()
        };

        let Some(pet) = self.pet.as_ref() else {
            return;
        };

        if self.settings_ui_active && self.settings_embed {
            self.compose_embedded_settings();
            return;
        }

        if self.settings_ui_active {
            let dpr = self.scale_factor.clamp(1.0, 3.0) as f32;
            let reminder = (
                self.config.reminder.enabled,
                self.config.reminder.interval_minutes,
                self.config.reminder.paused,
            );
            let (sw, sh, settings_rgba) = compose_settings_frame(
                reminder,
                self.effective_pet_scale(),
                dpr,
                self.menu_say.map(|(_, s)| s),
            );
            self.sprite_logical = (SETTINGS_W, SETTINGS_H);
            self.hit_rgba = settings_rgba;
            self.hit_size = (sw, sh);
            self.texture_dirty = false;
            return;
        }

        // Keep composing while menu is open or mid-close fade (still MenuOpen until anim ends).
        if self.menu_ui_active && pet.is_menu_open() {
            let clip = pet.active_clip();
            let pet_rgba = pet.display_rgba();
            let dpr = self.scale_factor.clamp(1.0, 3.0) as f32;
            if pet.is_menu_animating() && !self.list_drag.is_dragging() {
                if let (Some(layout), Some((cw, ch, card))) =
                    (self.menu_layout.as_ref(), self.menu_card_cache.as_ref())
                {
                    let fade = menu_visual_fade(pet.menu_open_t);
                    let scale = if !platform::client_area_animation_enabled() {
                        1.0
                    } else {
                        menu_visual_scale(pet.menu_open_t)
                    };
                    let mut out = vec![0u8; (*cw * *ch * 4) as usize];
                    present_menu_cached(
                        &mut out,
                        *cw,
                        *ch,
                        card,
                        *cw,
                        *ch,
                        &pet_rgba,
                        clip.frame_width,
                        clip.frame_height,
                        layout,
                        dpr,
                        scale,
                        fade,
                    );
                    self.sprite_logical = self.menu_logical_size;
                    self.hit_rgba = out;
                    self.hit_size = (*cw, *ch);
                    self.texture_dirty = false;
                    return;
                }
                // First open frame only: cache is built right after this present.
                // Closing without a cache must fall through to live compose.
                if !pet.menu_closing {
                    if let Some(layout) = self.menu_layout.as_ref() {
                        let (w, h, composed) = compose_menu_pet_only(
                            &pet_rgba,
                            clip.frame_width,
                            clip.frame_height,
                            layout,
                            dpr,
                        );
                        self.sprite_logical = self.menu_logical_size;
                        self.hit_rgba = composed;
                        self.hit_size = (w, h);
                        self.texture_dirty = false;
                        return;
                    }
                }
            }
            let entries = build_entries(
                self.shortcuts.list_enabled_sorted().as_slice(),
                |s| menu_icons.get(&s.id).cloned().flatten(),
            );
            let total = count_shortcuts(&entries);
            self.menu_list_scroll = clamp_list_scroll(self.menu_list_scroll, total);
            let (lw, lh) = self.menu_logical_size;
            // L3-02: geometry locked at open (open_t=1.0 for layout); visual uses menu_open_t.
            // Always re-layout shortcuts with current scroll so all apps are reachable.
            let mut layout = self
                .menu_layout
                .as_ref()
                .map(|prev| {
                    layout_pinned_scroll(
                        &entries,
                        lw,
                        lh,
                        (prev.pet_x, prev.pet_y, prev.pet_w, prev.pet_h),
                        (prev.card_x, prev.card_y, prev.card_w, prev.card_h),
                        radial_menu::ExpandDir::Right,
                        1.0,
                        self.menu_list_scroll,
                    )
                })
                .unwrap_or_else(|| {
                    layout_pinned_scroll(
                        &entries,
                        lw,
                        lh,
                        {
                            let ps = self.pet_size() as f32;
                            (0.0, 0.0, ps, ps)
                        },
                        {
                            let ps = self.pet_size() as f32;
                            (ps, 0.0, CARD_LOGICAL_W as f32, CARD_LOGICAL_H as f32)
                        },
                        radial_menu::ExpandDir::Right,
                        1.0,
                        self.menu_list_scroll,
                    )
                });
            layout.open_t = pet.menu_open_t;
            let chrome = MenuChromeState {
                hover: self.menu_hover,
                press: self.menu_press,
                hover_t: self.menu_hover_t,
                press_t: self.menu_press_t,
                closing: pet.menu_closing,
                say: self.menu_say.map(|(_, s)| s),
                reduced_motion: !platform::client_area_animation_enabled(),
                drag: self.list_drag_visual.clone(),
                drag_draft: false,
                rows_blank: false,
            };

            // Drag fast path: the card and rows are pre-rendered layers, so each frame
            // only re-blits them (insertion-slot changes shift the rows for free).
            if self.list_drag.is_dragging() && layout.open_t >= 0.999 {
                if self.list_drag_visual.is_none() {
                    let (w, h, composed) = compose_menu_frame(
                        &pet_rgba,
                        clip.frame_width,
                        clip.frame_height,
                        &layout,
                        dpr,
                        chrome,
                    );
                    self.menu_layout = Some(layout);
                    self.sprite_logical = (lw, lh);
                    self.hit_rgba = composed;
                    self.hit_size = (w, h);
                    self.texture_dirty = false;
                    return;
                }
                let key = DragLayersKey {
                    scroll: layout.list_scroll,
                    total,
                    hover: self.menu_hover,
                    press: self.menu_press,
                    say: self.menu_say.map(|(_, s)| s),
                };
                ensure_drag_layers(&mut self.menu_drag_layers, &layout, dpr, &chrome, key);
                let (_, (cw, ch, base), rows) = self
                    .menu_drag_layers
                    .as_ref()
                    .expect("drag layers built above");
                let need = (cw * ch * 4) as usize;
                if self.hit_rgba.len() < need {
                    self.hit_rgba = vec![0u8; need];
                }
                present_menu_drag(
                    &mut self.hit_rgba[..need],
                    *cw,
                    *ch,
                    base,
                    rows,
                    &pet_rgba,
                    clip.frame_width,
                    clip.frame_height,
                    &layout,
                    dpr,
                    self.menu_say.map(|(_, s)| s),
                    self.list_drag_visual.as_ref(),
                );
                self.menu_layout = Some(layout);
                self.sprite_logical = (lw, lh);
                self.hit_size = (*cw, *ch);
                self.texture_dirty = false;
                return;
            }

            let (w, h, composed) = compose_menu_frame(
                &pet_rgba,
                clip.frame_width,
                clip.frame_height,
                &layout,
                dpr,
                chrome,
            );
            // Unscaled geometry for hit-test; open_t only drives fade/scale in compose.
            self.menu_layout = Some(layout);
            self.sprite_logical = (lw, lh);
            self.hit_rgba = composed;
            self.hit_size = (w, h);
            self.texture_dirty = false;
            return;
        }

        if self.reminder_ui_active {
            if let Some(place) = self.reminder_place {
                let clip = pet.active_clip();
                let pet_rgba = pet.display_rgba();
                let now = Instant::now();
                let (sx, sy) = pet.reminder_squash(now);
                let slot = self.reminder_slot.unwrap_or(Point::new(
                    place.dest_local.x as f64,
                    place.dest_local.y as f64,
                ));
                let sw = place.origin_local.width.max(1) as f32;
                let sh = place.origin_local.height.max(1) as f32;
                let pw = (sw * sx).max(1.0);
                let ph = (sh * sy).max(1.0);
                let px = slot.x as f32 + (sw - pw) * 0.5;
                let py = slot.y as f32 + (sh - ph);
                let card_t = self.reminder_card_t.clamp(0.0, 1.0);
                let traveling = pet.is_reminder_moving();
                let pet_alpha = if traveling { 1.0 } else { 1.0 - card_t };
                let feeding = matches!(pet.state, PetState::Reminder(ReminderStage::Feeding));
                let card_blit = if !traveling && card_t > 0.01 {
                    self.reminder_card.as_ref().map(|card| OverlayCardBlit {
                        card,
                        bowl: self.feed_bowl.as_ref(),
                        feeding,
                        alpha: card_t,
                        x: place.card_local.x,
                        y: place.card_local.y,
                        w: place.card_local.width.max(1) as u32,
                        h: place.card_local.height.max(1) as u32,
                    })
                } else {
                    None
                };
                let (w, h, composed) = compose_reminder_overlay(
                    place.window.width.max(1) as u32,
                    place.window.height.max(1) as u32,
                    &pet_rgba,
                    clip.frame_width,
                    clip.frame_height,
                    px,
                    py,
                    pw,
                    ph,
                    pet_alpha,
                    card_blit,
                );
                let dpr = snap_dpr(self.scale_factor);
                self.sprite_logical = (
                    physical_to_logical_u32(place.window.width, dpr),
                    physical_to_logical_u32(place.window.height, dpr),
                );
                self.hit_rgba = composed;
                self.hit_size = (w, h);
                self.texture_dirty = false;
                return;
            }
            let feeding = matches!(pet.state, PetState::Reminder(ReminderStage::Feeding));
            if let Some(card) = &self.reminder_card {
                let (w, h, composed) =
                    compose_reminder_card_frame(card, self.feed_bowl.as_ref(), feeding);
                self.sprite_logical = (w, h);
                self.hit_rgba = composed;
                self.hit_size = (w, h);
                self.texture_dirty = false;
                return;
            }
            let clip = pet.active_clip();
            let pet_rgba = pet.display_rgba();
            let pulse = {
                let t = Instant::now().elapsed().as_secs_f32();
                1.0 + 0.05 * (t * std::f32::consts::TAU / 1.2).sin()
            };
            let (w, h, composed) = compose_reminder_frame(
                &pet_rgba,
                clip.frame_width,
                clip.frame_height,
                &pet.reminder_message,
                pulse,
                feeding,
            );
            self.sprite_logical = (w, h);
            self.hit_rgba = composed;
            self.hit_size = (w, h);
            self.texture_dirty = false;
            return;
        }

        if self.yawn_ui_active {
            if let Some(place) = self.yawn_place {
                let clip = pet.active_clip();
                let pet_rgba = pet.display_rgba();
                let dpr = snap_dpr(self.scale_factor);
                let pet_phys = logical_to_physical(self.pet_size(), dpr) as u32;
                // Idle present letterboxes the 256 sprite (side/top/paw margins).
                // Filling the overlay box 1:1 made the cat pop larger on yawn
                // enter and shrink on exit. Match the idle scaler exactly.
                let letterboxed = scale_rgba_centered(
                    &pet_rgba,
                    clip.frame_width,
                    clip.frame_height,
                    pet_phys,
                    pet_phys,
                    1.0,
                    false,
                );
                let (w, h, composed) = compose_yawn_frame(
                    &letterboxed,
                    pet_phys,
                    pet_phys,
                    place,
                    pet_phys,
                    pet.yawn_bubble_alpha(),
                    dpr,
                );
                self.sprite_logical = (
                    physical_to_logical_u32(place.window.width, dpr),
                    physical_to_logical_u32(place.window.height, dpr),
                );
                self.hit_rgba = composed;
                self.hit_size = (w, h);
                self.texture_dirty = false;
                return;
            }
        }

        // Idle after the dock has been grown once: keep painting the pet in
        // its slot inside the dock-sized buffer. Letterboxing into inner_size
        // would stretch the cat across the leftover overlay HWND.
        if let Some(dock) = self.dock_hwnd {
            let clip = pet.active_clip();
            let pet_rgba = pet.display_rgba();
            let dpr = self.scale_factor.clamp(1.0, 3.0) as f32;
            let (w, h, composed) = compose_pet_in_slot(
                &pet_rgba,
                clip.frame_width,
                clip.frame_height,
                dock.win_log_w,
                dock.win_log_h,
                dock.pet_x,
                dock.pet_y,
                dock.pet_w,
                dock.pet_h,
                dpr,
            );
            self.sprite_logical = (dock.win_log_w, dock.win_log_h);
            self.hit_rgba = composed;
            self.hit_size = (w, h);
            self.last_clip_name = clip.name.clone();
            self.texture_dirty = false;
            return;
        }

        let clip = pet.active_clip();
        // Sub-frame blend for smooth 30fps motion without identity swaps.
        let rgba = pet.display_rgba();
        let size = (clip.frame_width, clip.frame_height);
        self.sprite_logical = size;
        self.hit_rgba = rgba;
        self.hit_size = size;
        self.last_clip_name = clip.name.clone();
        self.texture_dirty = false;
    }

    fn redraw(&mut self) {
        if !self.visible {
            return;
        }
        // Refresh sprite when dirty or overlay is active.
        self.sync_texture_from_pet();

        let Some(window) = self.window.as_ref() else {
            return;
        };

        if self.hit_rgba.is_empty() {
            return;
        }

        // Scale content to physical window for layered present.
        let (sw, sh) = self.hit_size;
        let overlay = self.menu_ui_active
            || self.settings_ui_active
            || self.reminder_ui_active
            || self.yawn_ui_active;
        let drag_scale = if overlay {
            1.0
        } else {
            self.pet.as_ref().map(|p| p.drag_scale).unwrap_or(1.0)
        };

        // Always draw the authored facing (no mouse-driven flip).
        // Horizontal mirror made the cat face away from the cursor and looked inverted.
        let mirror_x = false;

        // Overlays: present at composed device-pixel size (1:1). Do not wait for
        // winit's async resize — that empty intermediate frame is the pet "flash".
        if overlay && !mirror_x {
            let pos = if self.settings_embed || self.menu_ui_active {
                self.menu_present_pos
            } else if self.settings_ui_active && self.settings_transition.is_some() {
                self.settings_present_pos
            } else if self.yawn_ui_active {
                self.yawn_present_pos
            } else if self.reminder_present_pos.is_some() {
                self.reminder_present_pos
            } else {
                None
            };
            if let Err(e) = platform::update_layered_rgba_ex(
                window.as_ref(),
                sw.max(1),
                sh.max(1),
                &self.hit_rgba,
                pos,
            ) {
                error!("update_layered_rgba: {e}");
            }
        } else if let Some(dock) = self.dock_hwnd {
            if let Err(e) = platform::update_layered_rgba_ex(
                window.as_ref(),
                sw.max(1),
                sh.max(1),
                &self.hit_rgba,
                Some(dock.pos),
            ) {
                error!("update_layered_rgba: {e}");
            }
        } else {
            // While the idle lock is set, present at the committed pet size and
            // desk spot — do not use the stale HWND inner_size (still the
            // overlay, or the pre-scale window).
            let (win_w, win_h, pos) = if let Some(pos) = self.idle_present_pos {
                let (pw, ph) = self.idle_target_phys();
                (pw.max(1), ph.max(1), Some(pos))
            } else {
                let win = window.inner_size();
                (win.width.max(1), win.height.max(1), None)
            };
            let present = scale_rgba_centered(
                &self.hit_rgba,
                sw,
                sh,
                win_w,
                win_h,
                drag_scale,
                mirror_x,
            );
            if let Err(e) =
                platform::update_layered_rgba_ex(window.as_ref(), win_w, win_h, &present, pos)
            {
                error!("update_layered_rgba: {e}");
            }
        }
    }

    fn update_click_through(&mut self) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        // Per-pixel alpha via UpdateLayeredWindow: transparent pixels already pass
        // clicks. Never enable WS_EX_TRANSPARENT on the whole window — that would
        // make the cat body unclickable.
        if self.click_through {
            let _ = platform::set_click_through(window.as_ref(), false);
            self.click_through = false;
        }
        let _ = window;
    }

    fn persist_window_pos(&mut self) {
        if self.window.is_none() {
            return;
        }
        // Don't persist temporary overlay positions as home config.
        if self.reminder_ui_active
            || self.menu_ui_active
            || self.settings_ui_active
            || self.yawn_ui_active
        {
            return;
        }
        let p = self
            .idle_present_pos
            .map(|(x, y)| Point::new(x as f64, y as f64))
            .or_else(|| self.pet_desk_origin());
        if let Some(p) = p {
            self.config.window.x = Some(p.x as i32);
            self.config.window.y = Some(p.y as i32);
            if let Some(saver) = self.saver.as_mut() {
                saver.mark_dirty();
            }
        }
    }

    fn work_area_center_top_left(&self, win_w: i32, win_h: i32) -> Point {
        let wa = self
            .window
            .as_ref()
            .and_then(|w| platform::work_area_for_window(w.as_ref()).ok())
            .or_else(|| platform::primary_work_area().ok())
            .unwrap_or(platform::Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            });
        Point::new(
            wa.x as f64 + (wa.width - win_w).max(0) as f64 / 2.0,
            wa.y as f64 + (wa.height - win_h).max(0) as f64 / 2.0,
        )
    }

    fn resize_pet_window(&mut self, logical_w: u32, logical_h: u32) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        // Physical size only — matches HiDPI compose buffers 1:1 (no soft scale).
        let dpr = self.scale_factor.clamp(1.0, 3.0);
        let dpr = if (dpr - dpr.round()).abs() < 0.08 {
            dpr.round()
        } else {
            dpr
        };
        let phys_w = ((logical_w as f64) * dpr).round().max(1.0) as u32;
        let phys_h = ((logical_h as f64) * dpr).round().max(1.0) as u32;
        let _ = window.request_inner_size(winit::dpi::PhysicalSize::new(phys_w, phys_h));
        info!(logical_w, logical_h, phys_w, phys_h, dpr, "pet window resized");
    }

    /// Physical idle pet HWND size for the committed scale (matches `resize_pet_window`).
    fn idle_target_phys(&self) -> (u32, u32) {
        let dpr = snap_dpr(self.scale_factor);
        let s = logical_to_physical(self.pet_size(), dpr) as u32;
        (s.max(1), s.max(1))
    }

    /// Tear down an overlay (or apply a live scale) without a one-frame jump:
    /// lock ULW to `origin` + the committed pet size, then ask winit to catch up.
    fn begin_idle_present_at(&mut self, origin: Point) {
        self.idle_present_pos = Some((origin.x as i32, origin.y as i32));
        let s = self.pet_size();
        self.resize_pet_window(s, s);
        if let Some(w) = &self.window {
            w.set_outer_position(PhysicalPosition::new(origin.x as i32, origin.y as i32));
        }
        self.texture_dirty = true;
        self.redraw();
    }

    fn enter_reminder_travel(&mut self, desk_origin: Point, now: Instant) -> bool {
        let dpr = snap_dpr(self.scale_factor);
        let pet_phys = logical_to_physical(self.pet_size(), dpr);
        let origin_pet = platform::Rect {
            x: desk_origin.x.round() as i32,
            y: desk_origin.y.round() as i32,
            width: pet_phys,
            height: pet_phys,
        };
        let work = platform::work_area_from_point(origin_pet.x + pet_phys / 2, origin_pet.y + pet_phys / 2)
            .map(|m| m.work_area)
            .ok()
            .or_else(|| {
                self.window
                    .as_ref()
                    .and_then(|w| platform::work_area_for_window(w.as_ref()).ok())
            })
            .or_else(|| platform::primary_work_area().ok())
            .unwrap_or(platform::Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            });
        let desk = platform::work_area_from_point(origin_pet.x + pet_phys / 2, origin_pet.y + pet_phys / 2)
            .map(|m| m.bounds)
            .ok()
            .unwrap_or(platform::Rect {
                x: work.x,
                y: work.y,
                width: work.width,
                height: work.height,
            });
        let dest_x = work.x + (work.width - pet_phys).max(0) / 2;
        let dest_y = work.y + (work.height - pet_phys).max(0) / 2;
        let dist = ((dest_x - origin_pet.x) as f64).hypot((dest_y - origin_pet.y) as f64);
        let lift = hop_arc_height(dist).round() as i32;
        let place = place_reminder_travel(
            origin_pet,
            work,
            desk,
            dpr,
            lift,
            REMINDER_WINDOW_W,
            REMINDER_WINDOW_H,
        );
        let start = Point::new(place.origin_local.x as f64, place.origin_local.y as f64);
        let dest = Point::new(place.dest_local.x as f64, place.dest_local.y as f64);
        let msg = pick_message(&self.config.reminder.custom_messages);

        let Some(pet) = self.pet.as_mut() else {
            return false;
        };
        if !pet.begin_reminder(desk_origin, start, dest, msg, now) {
            return false;
        }

        self.clear_dock_hwnd();
        self.idle_present_pos = None;
        self.overlay_origin = Some(desk_origin);
        self.reminder_place = Some(place);
        self.reminder_present_pos = Some((place.window.x, place.window.y));
        self.reminder_slot = Some(start);
        self.reminder_card_t = 0.0;
        self.reminder_card_anim = None;
        self.reminder_ui_active = true;

        if let Some(w) = &self.window {
            if let Err(e) = platform::sync_layered_hwnd(
                w.as_ref(),
                place.window.x,
                place.window.y,
                place.window.width.max(1) as u32,
                place.window.height.max(1) as u32,
            ) {
                warn!("sync_layered_hwnd reminder travel: {e}");
            }
            let _ = platform::set_click_through(w.as_ref(), false);
            self.click_through = false;
        }
        self.texture_dirty = true;
        self.redraw();
        info!(
            win = ?(place.window.x, place.window.y, place.window.width, place.window.height),
            "reminder travel overlay entered"
        );
        true
    }

    fn start_reminder_card_reveal(&mut self, now: Instant) {
        self.reminder_card_anim = Some((
            now,
            self.reminder_card_t,
            1.0,
            Duration::from_millis(140),
        ));
    }

    fn start_reminder_card_dismiss(&mut self, now: Instant) {
        self.reminder_card_anim = Some((
            now,
            self.reminder_card_t,
            0.0,
            Duration::from_millis(120),
        ));
    }

    fn tick_reminder_card_anim(&mut self, now: Instant) -> bool {
        let Some((started, from, to, dur)) = self.reminder_card_anim else {
            return false;
        };
        let u = if dur.is_zero() {
            1.0
        } else {
            (now.duration_since(started).as_secs_f32() / dur.as_secs_f32()).clamp(0.0, 1.0)
        };
        self.reminder_card_t = from + (to - from) * ease_out_cubic(u);
        if u < 1.0 {
            return true;
        }
        self.reminder_card_t = to;
        self.reminder_card_anim = None;
        if to <= 0.01 {
            if let (Some(place), Some(pet)) = (self.reminder_place, self.pet.as_mut()) {
                let from = Point::new(place.dest_local.x as f64, place.dest_local.y as f64);
                let home = Point::new(place.origin_local.x as f64, place.origin_local.y as f64);
                pet.start_reminder_return(from, home, now);
                self.reminder_slot = Some(from);
            }
        }
        true
    }

    fn current_reminder_pet_screen(&self) -> Point {
        if let Some(place) = self.reminder_place {
            let slot = self.reminder_slot.unwrap_or(Point::new(
                place.dest_local.x as f64,
                place.dest_local.y as f64,
            ));
            return Point::new(
                place.window.x as f64 + slot.x,
                place.window.y as f64 + slot.y,
            );
        }
        self.pet_desk_origin()
            .unwrap_or(Point::new(100.0, 100.0))
    }

    fn exit_reminder_overlay_to_pet(&mut self, top_left: Point) {
        self.reminder_ui_active = false;
        self.reminder_place = None;
        self.reminder_present_pos = None;
        self.reminder_slot = None;
        self.reminder_card_t = 0.0;
        self.reminder_card_anim = None;
        self.overlay_origin = None;
        if let Some(pet) = self.pet.as_mut() {
            pet.food_button_rect = None;
        }
        self.begin_idle_present_at(top_left);
    }

    fn enter_yawn_ui(&mut self) {
        if self.menu_ui_active || self.settings_ui_active || self.reminder_ui_active {
            return;
        }
        if self.yawn_ui_active {
            return;
        }
        self.idle_present_pos = None;
        self.capture_overlay_origin();
        self.clear_dock_hwnd();
        let dpr = snap_dpr(self.scale_factor);
        let origin = self.overlay_origin.unwrap_or(Point::new(100.0, 100.0));
        let pet_phys = logical_to_physical(self.pet_size(), dpr);
        let pet_rect = platform::Rect {
            x: origin.x as i32,
            y: origin.y as i32,
            width: pet_phys,
            height: pet_phys,
        };
        let pet_cx = pet_rect.x + pet_rect.width / 2;
        let pet_cy = pet_rect.y + pet_rect.height / 2;
        let work = platform::work_area_from_point(pet_cx, pet_cy)
            .map(|m| m.work_area)
            .ok()
            .or_else(|| {
                self.window
                    .as_ref()
                    .and_then(|w| platform::work_area_for_window(w.as_ref()).ok())
            })
            .or_else(|| platform::primary_work_area().ok())
            .unwrap_or(platform::Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            });
        let place = place_yawn_bubble(pet_rect, work, dpr);
        self.yawn_place = Some(place);
        self.yawn_present_pos = Some((place.window.x, place.window.y));
        self.yawn_ui_active = true;
        let win_log_w = physical_to_logical_u32(place.window.width, dpr);
        let win_log_h = physical_to_logical_u32(place.window.height, dpr);
        self.resize_pet_window(win_log_w, win_log_h);
        if let Some(w) = &self.window {
            w.set_outer_position(PhysicalPosition::new(place.window.x, place.window.y));
        }
        self.texture_dirty = true;
        self.redraw();
        info!(
            left = place.bubble_on_left,
            win = ?(place.window.x, place.window.y, place.window.width, place.window.height),
            "yawn bubble overlay entered"
        );
    }

    fn exit_yawn_ui(&mut self) {
        if !self.yawn_ui_active {
            return;
        }
        self.yawn_ui_active = false;
        self.yawn_place = None;
        self.yawn_present_pos = None;
        if !self.menu_ui_active && !self.settings_ui_active && !self.reminder_ui_active {
            self.restore_overlay_origin_window();
        }
        self.texture_dirty = true;
    }

    fn yawn_hit_bubble(&self) -> bool {
        let Some(p) = self.yawn_place else {
            return false;
        };
        let x = self.cursor_in_window.x as i32;
        let y = self.cursor_in_window.y as i32;
        x >= p.bubble_local.0
            && x < p.bubble_local.0 + p.bubble_w
            && y >= p.bubble_local.1
            && y < p.bubble_local.1 + p.bubble_h
    }

    fn capture_overlay_origin(&mut self) {
        if self.overlay_origin.is_some() {
            return;
        }
        if let Some(p) = self.pet_desk_origin() {
            self.overlay_origin = Some(p);
        }
    }

    fn restore_overlay_origin_window(&mut self) {
        // Any preview draft dies here: without 「完成」the pet keeps its committed size.
        self.pet_scale_draft = None;
        let origin = self
            .overlay_origin
            .take()
            .unwrap_or(Point::new(100.0, 100.0));
        self.menu_ui_active = false;
        self.menu_outside_guard = None;
        self.settings_ui_active = false;
        self.yawn_ui_active = false;
        self.yawn_place = None;
        self.yawn_present_pos = None;
        self.menu_layout = None;
        // Keep the dock-sized HWND. Shrinking back to 128 discards the layered
        // bitmap; the next open then grows and flashes. Re-anchor the existing
        // box so the pet slot sits on `origin`, then ULW pet-only (same size).
        if let Some(mut dock) = self.dock_hwnd {
            let dpr = snap_dpr(self.scale_factor);
            let lx = (dock.pet_x as f64 * dpr).round() as i32;
            let ly = (dock.pet_y as f64 * dpr).round() as i32;
            dock.pos = (origin.x as i32 - lx, origin.y as i32 - ly);
            self.dock_hwnd = Some(dock);
            self.idle_present_pos = None;
            self.texture_dirty = true;
            self.redraw();
            return;
        }
        // First session (dock never grown): lock ULW to the pet box.
        self.begin_idle_present_at(origin);
    }

    fn enter_menu_ui(&mut self, now: Instant) {
        if self.reminder_ui_active || self.settings_ui_active {
            return;
        }
        self.idle_present_pos = None;

        // L2: leave edge-hide before capture so pin uses fully-visible home.
        // Do not move the HWND here — that would wipe the layered bitmap before
        // the first dock present. Seed overlay_origin so placement uses home.
        if let Some(pet) = self.pet.as_mut() {
            if let Some(home) = pet.snap_restore_from_edge(now) {
                self.overlay_origin = Some(home);
            }
        }

        let Some(pet) = self.pet.as_mut() else {
            return;
        };
        if !pet.open_menu(now) {
            return;
        }
        self.capture_overlay_origin();

        let dpr = snap_dpr(self.scale_factor);
        let origin = self.overlay_origin.unwrap_or(Point::new(100.0, 100.0));
        let pet_phys = logical_to_physical(self.pet_size(), dpr);
        let pet_rect = platform::Rect {
            x: origin.x as i32,
            y: origin.y as i32,
            width: pet_phys,
            height: pet_phys,
        };
        let card_w = logical_to_physical(CARD_LOGICAL_W, dpr);
        let card_h = logical_to_physical(CARD_LOGICAL_H, dpr);

        // Multi-monitor: work area of the screen under the pet center.
        let pet_cx = pet_rect.x + pet_rect.width / 2;
        let pet_cy = pet_rect.y + pet_rect.height / 2;
        let work = platform::work_area_from_point(pet_cx, pet_cy)
            .map(|m| m.work_area)
            .ok()
            .or_else(|| {
                self.window
                    .as_ref()
                    .and_then(|w| platform::work_area_for_window(w.as_ref()).ok())
            })
            .or_else(|| platform::primary_work_area().ok())
            .unwrap_or(platform::Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            });

        let place = place_launcher(
            pet_rect,
            card_w,
            card_h,
            DEFAULT_GAP,
            work,
            DEFAULT_MARGIN,
        );

        let win_log_w = physical_to_logical_u32(place.window.width, dpr);
        let win_log_h = physical_to_logical_u32(place.window.height, dpr);
        self.menu_logical_size = (win_log_w, win_log_h);

        let pet_local = (
            physical_to_logical(place.pet_local.x, dpr),
            physical_to_logical(place.pet_local.y, dpr),
            physical_to_logical(place.pet_local.width, dpr),
            physical_to_logical(place.pet_local.height, dpr),
        );
        let card_local = (
            physical_to_logical(place.card_local.x, dpr),
            physical_to_logical(place.card_local.y, dpr),
            physical_to_logical(place.card_local.width, dpr),
            physical_to_logical(place.card_local.height, dpr),
        );

        let entries = build_entries(
            self.shortcuts.list_enabled_sorted().as_slice(),
            |s| self.shortcut_icon(s),
        );
        self.menu_list_scroll = 0;
        self.menu_say = None;
        self.menu_layout = Some(layout_pinned_scroll(
            &entries,
            win_log_w,
            win_log_h,
            pet_local,
            card_local,
            place.dir,
            0.0,
            0,
        ));

        self.menu_ui_active = true;
        self.menu_hover = None;
        self.menu_press = None;
        self.menu_hover_t = 0.0;
        self.menu_press_t = 0.0;
        self.list_drag = ListDrag::Idle;
        self.list_drag_visual = None;
        self.list_drag_edge_at = None;
        self.menu_drag_layers = None;
        // Atomic layered present target (physical). Avoids empty frames during resize.
        self.menu_present_pos = Some((place.window.x, place.window.y));
        // Reserve canvas for a 100% pet so settings ± never clips the tail.
        let _ = self.ensure_overlay_fits_max_pet();

        // Rasterize the rest card while the idle pet is still on screen, so the
        // first ULW after the HWND grows is a complete frame.
        self.menu_card_cache = None;
        if let Some(layout) = self.menu_layout.as_ref() {
            let dpr_f = self.scale_factor.clamp(1.0, 3.0) as f32;
            let (cw, ch, card) = compose_menu_card_layer(
                layout,
                dpr_f,
                MenuChromeState {
                    reduced_motion: !platform::client_area_animation_enabled(),
                    ..MenuChromeState::default()
                },
            );
            self.menu_card_cache = Some((cw, ch, card));
        }

        // Growing a layered HWND discards the ULW bitmap (the flash). After
        // the first open we keep that size and only UpdateLayeredWindow.
        let dpr_phys = snap_dpr(self.scale_factor);
        let (target, present) = if let Some(layout) = self.menu_layout.as_ref() {
            let present = self
                .menu_present_pos
                .unwrap_or((place.window.x, place.window.y));
            (
                (
                    logical_to_physical(layout.window_w, dpr_phys).max(1) as u32,
                    logical_to_physical(layout.window_h, dpr_phys).max(1) as u32,
                ),
                present,
            )
        } else {
            (
                (
                    place.window.width.max(1) as u32,
                    place.window.height.max(1) as u32,
                ),
                (place.window.x, place.window.y),
            )
        };
        let reuse = self
            .dock_hwnd
            .is_some_and(|d| d.phys == target);
        if !reuse {
            if let Some(w) = &self.window {
                if let Err(e) = platform::sync_layered_hwnd(
                    w.as_ref(),
                    present.0,
                    present.1,
                    target.0,
                    target.1,
                ) {
                    warn!("sync_layered_hwnd: {e}");
                }
            }
        }
        if let Some(layout) = self.menu_layout.clone() {
            self.remember_dock_hwnd(present, target, &layout);
        }
        self.texture_dirty = true;
        self.redraw();
        if let Some(w) = &self.window {
            let _ = platform::set_click_through(w.as_ref(), false);
            self.click_through = false;
        }

        // Outside-click guard: clicking the desktop / another window closes the dock.
        let guard = platform::OutsideClickGuard::install(place.window);
        if guard.is_none() {
            warn!("outside-click hook unavailable; dock closes via in-window clicks only");
        }
        self.menu_outside_guard = guard;
        info!(
            ?place.dir,
            win = ?(place.window.x, place.window.y, place.window.width, place.window.height),
            delta = ?place.pet_screen_delta,
            "launcher dock entered (pin-pet)"
        );
    }

    /// Instant close — keyboard / any path that must not wait on motion.
    fn exit_menu_ui_instant(&mut self, now: Instant) {
        if let Some(pet) = self.pet.as_mut() {
            pet.close_menu(now);
        }
        self.finish_exit_menu_ui(now);
    }

    /// Request menu close with L3 closing animation when possible.
    fn exit_menu_ui(&mut self, now: Instant) {
        if let Some(pet) = self.pet.as_mut() {
            if pet.begin_close_menu(now) {
                self.texture_dirty = true;
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
                info!("launcher close anim started");
                return;
            }
        }
        self.finish_exit_menu_ui(now);
    }

    /// After close anim (or snap): restore pet window + debounce reopen.
    fn finish_exit_menu_ui(&mut self, now: Instant) {
        if let Some(pet) = self.pet.as_mut() {
            if pet.is_menu_open() {
                pet.close_menu(now);
            }
        }
        self.menu_hover = None;
        self.menu_press = None;
        self.menu_hover_t = 0.0;
        self.menu_press_t = 0.0;
        self.menu_present_pos = None;
        self.menu_card_cache = None;
        self.menu_list_scroll = 0;
        self.menu_say = None;
        self.list_drag = ListDrag::Idle;
        self.list_drag_visual = None;
        self.list_drag_edge_at = None;
        self.menu_drag_layers = None;
        self.settings_ui_active = false;
        self.settings_embed = false;
        self.settings_transition = None;
        self.settings_card_cache = None;
        self.settings_layout_snapshot = None;
        self.pet_scale_draft = None;
        self.sync_dock_hwnd_slot_from_layout();
        self.restore_overlay_origin_window();
        // L3-05: ignore click that closed the dock + brief double-tap reopen.
        self.menu_reopen_after = Some(now + Duration::from_millis(280));
        info!("launcher dock exited");
    }

    /// Mouse wheel: scroll shortcut list when dock is open (many apps).
    fn scroll_menu_list(&mut self, lines: i32) {
        if lines == 0 || self.settings_transition.is_some() {
            return;
        }
        if self.settings_embed && self.settings_ui_active {
            return;
        }
        if !self.menu_ui_active {
            return;
        }
        if self
            .pet
            .as_ref()
            .map(|p| p.is_menu_animating())
            .unwrap_or(false)
        {
            return;
        }
        let total = self
            .shortcuts
            .list_enabled_sorted()
            .len();
        let cur = self.menu_list_scroll as i32;
        let next = (cur - lines).max(0) as usize; // wheel up → show earlier items
        let next = clamp_list_scroll(next, total);
        if next != self.menu_list_scroll {
            self.menu_list_scroll = next;
            self.menu_hover = None;
            self.menu_press = None;
            self.menu_card_cache = None;
            self.texture_dirty = true;
            if let Some(w) = &self.window {
                w.request_redraw();
            }
            info!(scroll = next, total, "menu list scroll");
        }
    }

    fn enter_settings_ui(&mut self) {
        self.enter_settings_ui_plain();
    }

    /// Settings opened from a launcher button: slide in from the card's right.
    fn begin_settings_from_launcher(
        &mut self,
        _anchor: (i32, i32),
        now: Instant,
    ) {
        if self.reminder_ui_active || !self.menu_ui_active {
            return;
        }
        let Some(pet) = self.pet.as_ref() else {
            return;
        };
        if !pet.is_menu_open() {
            return;
        }
        if self.menu_layout.is_none() {
            self.enter_settings_ui_plain();
            return;
        }
        // Fresh session: any previously discarded preview draft must not leak
        // into this one (config is always the source of truth on entry).
        self.pet_scale_draft = None;
        // Pad before caches/snapshot so ± never clips the tail and Esc
        // restores the already-reserved canvas (no HWND shrink).
        if self.ensure_overlay_fits_max_pet() {
            self.menu_card_cache = None;
            if let (Some(w), Some(pos), Some(layout)) = (
                self.window.as_ref(),
                self.menu_present_pos,
                self.menu_layout.as_ref(),
            ) {
                let dpr = snap_dpr(self.scale_factor);
                let tw = logical_to_physical(layout.window_w, dpr).max(1) as u32;
                let th = logical_to_physical(layout.window_h, dpr).max(1) as u32;
                if let Err(e) = platform::sync_layered_hwnd(w.as_ref(), pos.0, pos.1, tw, th)
                {
                    warn!("sync_layered_hwnd: {e}");
                }
            }
        }
        self.settings_ui_active = true;
        self.settings_embed = true;
        self.settings_card_cache = None;
        if self.menu_card_cache.is_none() {
            if let Some(layout) = self.menu_layout.as_ref() {
                let dpr = self.scale_factor.clamp(1.0, 3.0) as f32;
                let (cw, ch, card) = compose_menu_card_layer(
                    layout,
                    dpr,
                    MenuChromeState {
                        reduced_motion: !platform::client_area_animation_enabled(),
                        ..MenuChromeState::default()
                    },
                );
                self.menu_card_cache = Some((cw, ch, card));
            }
        }
        self.ensure_settings_card_cache();
        if let (Some(lay), Some(pos)) = (self.menu_layout.clone(), self.menu_present_pos) {
            self.settings_layout_snapshot = Some(SettingsLayoutSnapshot {
                layout: lay,
                present_pos: pos,
                logical_size: self.menu_logical_size,
            });
        }
        self.settings_transition = Some(SettingsTransition {
            started: now,
            duration: Duration::from_millis(220),
            t: 0.0,
            entering: true,
        });
        self.texture_dirty = true;
        self.redraw();
        info!("settings slide-in from launcher card");
    }

    fn begin_settings_pop(&mut self, now: Instant) {
        if !self.settings_embed || !self.settings_ui_active {
            self.exit_settings_ui();
            return;
        }
        // Leaving the settings session ends the preview: on Esc this discards
        // the draft and snaps the dock back to the entry snapshot; on 「完成」
        // the draft was already committed, so the previewed geometry stays.
        let discarding = self.pet_scale_draft.is_some();
        self.pet_scale_draft = None;
        if discarding {
            self.restore_settings_layout_snapshot();
        }
        self.settings_layout_snapshot = None;
        if self.settings_transition.is_some() {
            return;
        }
        self.ensure_settings_card_cache();
        if self.menu_card_cache.is_none() {
            if let Some(layout) = self.menu_layout.as_ref() {
                let dpr = self.scale_factor.clamp(1.0, 3.0) as f32;
                let (cw, ch, card) = compose_menu_card_layer(
                    layout,
                    dpr,
                    MenuChromeState {
                        reduced_motion: !platform::client_area_animation_enabled(),
                        ..MenuChromeState::default()
                    },
                );
                self.menu_card_cache = Some((cw, ch, card));
            }
        }
        self.settings_transition = Some(SettingsTransition {
            started: now,
            duration: Duration::from_millis(220),
            t: 0.0,
            entering: false,
        });
        self.texture_dirty = true;
        if let Some(w) = &self.window {
            w.request_redraw();
        }
        info!("settings slide-out back to launcher");
    }

    fn ensure_settings_card_cache(&mut self) {
        if self.settings_card_cache.is_some() {
            return;
        }
        let Some(layout) = self.menu_layout.as_ref() else {
            return;
        };
        let dpr = self.scale_factor.clamp(1.0, 3.0) as f32;
        let reminder = (
            self.config.reminder.enabled,
            self.config.reminder.interval_minutes,
            self.config.reminder.paused,
        );
        let (sw, sh, buf) = compose_settings_card(
            reminder,
            self.effective_pet_scale(),
            dpr,
            layout.card_w,
            layout.card_h,
        );
        self.settings_card_cache = Some((sw, sh, buf));
    }

    fn compose_embedded_settings(&mut self) {
        let Some(layout) = self.menu_layout.clone() else {
            return;
        };
        let dpr = self.scale_factor.clamp(1.0, 3.0) as f32;
        let Some((fw, fh, pet_rgba)) = self.pet.as_ref().map(|p| {
            let clip = p.active_clip();
            (clip.frame_width, clip.frame_height, p.display_rgba())
        }) else {
            return;
        };
        // Overlay HWND / card stay frozen during ±. The canvas was reserved
        // for a max-size pet on enter, so the slot should stay on-canvas.
        let (ww, hh, mut out) = compose_menu_pet_only(&pet_rgba, fw, fh, &layout, dpr);
        let d = if (dpr - dpr.round()).abs() < 0.08 {
            dpr.round()
        } else {
            dpr
        };
        let cx = (layout.card_x * d).round() as i32;
        let cy = (layout.card_y * d).round() as i32;
        let cw = (layout.card_w * d).round().max(1.0) as i32;
        let ch = (layout.card_h * d).round().max(1.0) as i32;
        let clip_r = (cx, cy, cw, ch);

        let reduced = !platform::client_area_animation_enabled();
        if let Some(tr) = self.settings_transition {
            self.ensure_settings_card_cache();
            let k = if reduced {
                if tr.t >= 1.0 {
                    1.0
                } else {
                    tr.t
                }
            } else {
                ease_in_out_cubic(tr.t)
            };
            let (menu_shift, set_shift) = if tr.entering {
                (-(k * cw as f32).round() as i32, ((1.0 - k) * cw as f32).round() as i32)
            } else {
                (-((1.0 - k) * cw as f32).round() as i32, (k * cw as f32).round() as i32)
            };
            if reduced {
                let fade_set = if tr.entering { k } else { 1.0 - k };
                let fade_menu = 1.0 - fade_set;
                if let Some((mw, mh, menu)) = self.menu_card_cache.as_ref() {
                    let mut faded = menu.clone();
                    fade_rgba_alpha(&mut faded, fade_menu);
                    blit_rgba_clipped(&mut out, ww, hh, &faded, *mw, *mh, 0, 0, clip_r);
                }
                if let Some((sw, sh, set)) = self.settings_card_cache.as_ref() {
                    let mut faded = set.clone();
                    fade_rgba_alpha(&mut faded, fade_set);
                    blit_rgba_clipped(&mut out, ww, hh, &faded, *sw, *sh, cx, cy, clip_r);
                }
            } else {
                if let Some((mw, mh, menu)) = self.menu_card_cache.as_ref() {
                    blit_rgba_clipped(
                        &mut out,
                        ww,
                        hh,
                        menu,
                        *mw,
                        *mh,
                        menu_shift,
                        0,
                        clip_r,
                    );
                }
                if let Some((sw, sh, set)) = self.settings_card_cache.as_ref() {
                    blit_rgba_clipped(
                        &mut out,
                        ww,
                        hh,
                        set,
                        *sw,
                        *sh,
                        cx + set_shift,
                        cy,
                        clip_r,
                    );
                }
            }
        } else {
            let reminder = (
                self.config.reminder.enabled,
                self.config.reminder.interval_minutes,
                self.config.reminder.paused,
            );
            let (sw, sh, set) = compose_settings_card(
                reminder,
                self.effective_pet_scale(),
                dpr,
                layout.card_w,
                layout.card_h,
            );
            blit_rgba(&mut out, ww, hh, &set, sw, sh, cx.max(0) as u32, cy.max(0) as u32);
        }

        if let Some((_, line)) = self.menu_say {
            draw_say_bubble(&mut out, ww, hh, dpr, Some(&layout), None, line, 1.0);
        }

        self.sprite_logical = self.menu_logical_size;
        self.hit_rgba = out;
        self.hit_size = (ww, hh);
        self.texture_dirty = false;
    }

    /// Settings opened without a launcher anchor (tray): centered, no transition.
    fn enter_settings_ui_plain(&mut self) {
        if self.reminder_ui_active {
            return;
        }
        // Fresh settings session — start from the committed size, not a stale draft.
        self.clear_dock_hwnd();
        self.idle_present_pos = None;
        self.pet_scale_draft = None;
        self.settings_transition = None;
        self.settings_present_pos = None;
        self.settings_embed = false;
        self.settings_card_cache = None;
        self.settings_layout_snapshot = None;
        // Close menu into settings without losing origin.
        if self.menu_ui_active {
            if let Some(pet) = self.pet.as_mut() {
                let now = Instant::now();
                pet.close_menu(now);
            }
            self.menu_ui_active = false;
            self.menu_outside_guard = None;
            self.menu_layout = None;
            self.menu_hover = None;
            self.menu_press = None;
            self.menu_hover_t = 0.0;
            self.menu_press_t = 0.0;
            self.menu_present_pos = None;
            self.menu_card_cache = None;
        } else {
            self.capture_overlay_origin();
        }
        self.settings_ui_active = true;
        self.resize_pet_window(SETTINGS_W, SETTINGS_H);
        let center = self.work_area_center_top_left(SETTINGS_W as i32, SETTINGS_H as i32);
        if let Some(w) = &self.window {
            w.set_outer_position(PhysicalPosition::new(center.x as i32, center.y as i32));
            let _ = platform::set_click_through(w.as_ref(), false);
            self.click_through = false;
            w.request_redraw();
        }
        self.texture_dirty = true;
        info!("settings UI entered");
    }

    fn exit_settings_ui(&mut self) {
        if self.settings_embed && self.menu_ui_active {
            self.begin_settings_pop(Instant::now());
            return;
        }
        self.settings_ui_active = false;
        self.settings_transition = None;
        self.settings_present_pos = None;
        self.settings_embed = false;
        self.settings_card_cache = None;
        self.restore_overlay_origin_window();
        info!("settings UI exited");
    }

    /// Advance the launcher→settings transition; returns `true` when finished.
    fn tick_settings_transition(&mut self, now: Instant) -> bool {
        let Some(tr) = self.settings_transition else {
            return false;
        };
        let t = ((now - tr.started).as_secs_f32() / tr.duration.as_secs_f32()).clamp(0.0, 1.0);
        if t >= 1.0 {
            self.finish_settings_transition(now);
            return true;
        }
        self.settings_transition = Some(SettingsTransition { t, ..tr });
        self.texture_dirty = true;
        false
    }

    /// After the in-card slide finishes: stay on the dock window.
    fn finish_settings_transition(&mut self, now: Instant) {
        let Some(tr) = self.settings_transition else {
            return;
        };
        let entering = tr.entering;
        self.settings_transition = None;
        self.settings_card_cache = None;
        if entering {
            self.settings_ui_active = true;
            self.settings_embed = true;
        } else {
            self.settings_ui_active = false;
            self.settings_embed = false;
            self.sync_menu_pet_slot_to_effective_scale();
        }
        let _ = now;
        self.texture_dirty = true;
        self.redraw();
        info!(entering, "settings card slide settled");
    }

    /// Screen position (physical px) of the launcher item that opened settings.
    fn menu_anchor_screen_for(&self, item_idx: Option<usize>) -> Option<(i32, i32)> {
        let layout = self.menu_layout.as_ref()?;
        let pos = self.menu_present_pos?;
        let item = item_idx.and_then(|i| layout.items.get(i)).or_else(|| {
            layout
                .items
                .iter()
                .find(|it| matches!(it.entry, MenuEntry::Manage))
        })?;
        let dpr = snap_dpr(self.scale_factor);
        Some((
            pos.0 + (item.cx * dpr as f32).round() as i32,
            pos.1 + (item.cy * dpr as f32).round() as i32,
        ))
    }

    /// Map client cursor to menu layout logical coords.
    fn menu_cursor_logical(&self) -> Option<(f32, f32)> {
        if !self.menu_ui_active {
            return None;
        }
        let (lw, lh) = self.menu_logical_size;
        let (cw, ch) = self.window.as_ref().map(|w| {
            let s = w.inner_size();
            (s.width as f64, s.height as f64)
        })?;
        let lx = (self.cursor_in_window.x / cw.max(1.0) * lw as f64) as f32;
        let ly = (self.cursor_in_window.y / ch.max(1.0) * lh as f64) as f32;
        Some((lx, ly))
    }

    fn begin_list_press(&mut self, idx: usize, lx: f32, ly: f32, now: Instant) {
        let Some(layout) = self.menu_layout.clone() else {
            return;
        };
        let Some(item) = layout.items.get(idx) else {
            return;
        };
        let MenuEntry::Shortcut { id, name, valid, icon } = item.entry.clone() else {
            self.list_drag = ListDrag::Idle;
            self.menu_drag_layers = None;
            return;
        };
        let Some(from) = self
            .shortcuts
            .list_enabled_sorted()
            .iter()
            .position(|s| s.id == id)
        else {
            return;
        };
        self.list_drag = ListDrag::Pressing {
            id,
            item_idx: idx,
            from,
            origin: (lx, ly),
            armed: true,
            t0: now,
        };
        // Warm everything the drag will need during the 400 ms hold, so lifting
        // the row paints smoothly: lifted-row image, bowl, hints, static card
        // layer and row bitmaps. A tap just wastes this one-time prerender.
        let dpr = self.scale_factor.clamp(1.0, 3.0) as f32;
        let total = self.shortcuts.list_enabled_sorted().len();
        let insert_at = insert_index_from_y(
            ly,
            layout.list_top,
            ROW_H + ROW_GAP,
            layout.list_scroll,
            total.saturating_sub(1),
        );
        let bowl = bowl_rect(
            layout.pet_x,
            layout.pet_y,
            layout.pet_w,
            layout.pet_h,
            layout.window_w as f32,
            layout.window_h as f32,
        );
        let over_bowl = crate::ui::list_drag::hit_bowl(lx, ly, bowl);
        self.list_drag_visual = Some(MenuDragChrome {
            id,
            name,
            valid,
            icon,
            pointer_x: lx,
            pointer_y: ly,
            grab_dx: lx - item.x,
            grab_dy: ly - item.y,
            from,
            insert_at,
            over_bowl,
            row_w: item.w,
            row_h: item.h,
            ghost_img: None,
            bowl_img: None,
            bowl_over_img: None,
            hint_ink: None,
            hint_kicker: None,
        });
        if let Some(visual) = self.list_drag_visual.as_mut() {
            prerender_drag_images(visual, dpr);
        }
        let chrome = MenuChromeState {
            hover: self.menu_hover,
            press: self.menu_press,
            hover_t: self.menu_hover_t,
            press_t: self.menu_press_t,
            closing: self.pet.as_ref().map(|p| p.menu_closing).unwrap_or(false),
            say: self.menu_say.map(|(_, s)| s),
            reduced_motion: !platform::client_area_animation_enabled(),
            drag: None,
            drag_draft: false,
            rows_blank: false,
        };
        let key = DragLayersKey {
            scroll: layout.list_scroll,
            total,
            hover: self.menu_hover,
            press: self.menu_press,
            say: self.menu_say.map(|(_, s)| s),
        };
        self.rebuild_drag_layers_if_stale(&layout, dpr, &chrome, key);
    }

    /// Rebuild the static drag layers (blank card + row bitmaps) only when the
    /// key changed; insertion-slot changes never rebuild them.
    fn rebuild_drag_layers_if_stale(
        &mut self,
        layout: &RadialLayout,
        dpr: f32,
        chrome: &MenuChromeState,
        key: DragLayersKey,
    ) {
        ensure_drag_layers(&mut self.menu_drag_layers, layout, dpr, chrome, key);
    }

    fn tick_list_drag_hold(&mut self, now: Instant) {
        let ListDrag::Pressing {
            id,
            item_idx,
            from,
            origin,
            armed,
            t0,
        } = self.list_drag.clone()
        else {
            return;
        };
        let Some((lx, ly)) = self.menu_cursor_logical() else {
            return;
        };
        let dist = pointer_dist(origin, (lx, ly));
        if dist >= SLOP_PX {
            if let ListDrag::Pressing { armed, .. } = &mut self.list_drag {
                *armed = false;
            }
            return;
        }
        if !armed || !should_start_drag(now.saturating_duration_since(t0), dist) {
            return;
        }
        let Some(layout) = self.menu_layout.as_ref() else {
            return;
        };
        let Some(item) = layout.items.get(item_idx) else {
            return;
        };
        let total = self.shortcuts.list_enabled_sorted().len();
        let max_insert = total.saturating_sub(1);
        let insert_at = insert_index_from_y(
            ly,
            layout.list_top,
            ROW_H + ROW_GAP,
            layout.list_scroll,
            max_insert,
        );
        let bowl = bowl_rect(
            layout.pet_x,
            layout.pet_y,
            layout.pet_w,
            layout.pet_h,
            layout.window_w as f32,
            layout.window_h as f32,
        );
        let over_bowl = crate::ui::list_drag::hit_bowl(lx, ly, bowl);
        self.list_drag = ListDrag::Dragging {
            id,
            from,
            grab_dx: lx - item.x,
            grab_dy: ly - item.y,
            pointer: (lx, ly),
            insert_at,
            over_bowl,
            row_w: item.w,
            row_h: item.h,
        };
        // Reuse the press-time prerendered visual (ghost / bowl / hints) instead
        // of rebuilding it; the long-press hold already paid that cost.
        if let Some(visual) = self.list_drag_visual.as_mut() {
            visual.pointer_x = lx;
            visual.pointer_y = ly;
            visual.grab_dx = lx - item.x;
            visual.grab_dy = ly - item.y;
            visual.from = from;
            visual.insert_at = insert_at;
            visual.over_bowl = over_bowl;
            visual.row_w = item.w;
            visual.row_h = item.h;
        } else {
            let (name, valid, icon) = match &item.entry {
                MenuEntry::Shortcut {
                    name,
                    valid,
                    icon,
                    ..
                } => (name.clone(), *valid, icon.clone()),
                _ => return,
            };
            let mut visual = MenuDragChrome {
                id,
                name,
                valid,
                icon,
                pointer_x: lx,
                pointer_y: ly,
                grab_dx: lx - item.x,
                grab_dy: ly - item.y,
                from,
                insert_at,
                over_bowl,
                row_w: item.w,
                row_h: item.h,
                ghost_img: None,
                bowl_img: None,
                bowl_over_img: None,
                hint_ink: None,
                hint_kicker: None,
            };
            let dpr = self.scale_factor.clamp(1.0, 3.0) as f32;
            prerender_drag_images(&mut visual, dpr);
            self.list_drag_visual = Some(visual);
        }
        self.menu_card_cache = None;
        self.texture_dirty = true;
    }

    fn tick_list_drag_move(&mut self) {
        let ListDrag::Dragging {
            id,
            from,
            grab_dx,
            grab_dy,
            row_w,
            row_h,
            ..
        } = self.list_drag.clone()
        else {
            if let ListDrag::Pressing { origin, .. } = &self.list_drag {
                if let Some((lx, ly)) = self.menu_cursor_logical() {
                    if pointer_dist(*origin, (lx, ly)) >= SLOP_PX {
                        if let ListDrag::Pressing { armed, .. } = &mut self.list_drag {
                            *armed = false;
                        }
                    }
                }
            }
            return;
        };
        let Some((lx, ly)) = self.menu_cursor_logical() else {
            return;
        };
        let Some(layout) = self.menu_layout.as_ref() else {
            return;
        };
        let total = self.shortcuts.list_enabled_sorted().len();
        let max_insert = total.saturating_sub(1);
        let insert_at = insert_index_from_y(
            ly,
            layout.list_top,
            ROW_H + ROW_GAP,
            layout.list_scroll,
            max_insert,
        );
        let bowl = bowl_rect(
            layout.pet_x,
            layout.pet_y,
            layout.pet_w,
            layout.pet_h,
            layout.window_w as f32,
            layout.window_h as f32,
        );
        let over_bowl = crate::ui::list_drag::hit_bowl(lx, ly, bowl);
        self.list_drag = ListDrag::Dragging {
            id,
            from,
            grab_dx,
            grab_dy,
            pointer: (lx, ly),
            insert_at,
            over_bowl,
            row_w,
            row_h,
        };
        if let Some(v) = self.list_drag_visual.as_mut() {
            v.pointer_x = lx;
            v.pointer_y = ly;
            v.insert_at = insert_at;
            v.over_bowl = over_bowl;
        }
        let now = Instant::now();
        let delta = edge_scroll_delta(ly, layout.list_top, layout.list_bottom);
        if delta != 0 {
            let due = self
                .list_drag_edge_at
                .map(|t| now.saturating_duration_since(t) >= Duration::from_millis(180))
                .unwrap_or(true);
            if due {
                self.list_drag_edge_at = Some(now);
                self.scroll_menu_list(delta);
            }
        }
        self.texture_dirty = true;
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn drop_list_drag(&mut self, now: Instant) {
        let ListDrag::Dragging {
            id,
            insert_at,
            over_bowl,
            ..
        } = std::mem::replace(&mut self.list_drag, ListDrag::Idle)
        else {
            self.list_drag = ListDrag::Idle;
            self.list_drag_visual = None;
            self.menu_drag_layers = None;
            return;
        };
        self.list_drag_visual = None;
        self.list_drag_edge_at = None;
        self.menu_drag_layers = None;
        if over_bowl {
            self.shortcuts.remove(id);
            self.shortcut_icons.clear();
            self.persist_shortcuts();
            self.menu_say = Some((now, SAY_EATEN));
            info!(%id, "shortcut fed to the bowl");
        } else {
            let ids: Vec<_> = self
                .shortcuts
                .list_enabled_sorted()
                .iter()
                .map(|s| s.id)
                .collect();
            let next = reorder_ids(&ids, id, insert_at);
            self.shortcuts.reorder(&next);
            self.persist_shortcuts();
        }
        self.menu_card_cache = None;
        self.texture_dirty = true;
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    /// Commit a row only if the pointer is still on it (tap: down highlight, up commit).
    fn commit_menu_press(&mut self, now: Instant) {
        if self.list_drag.is_dragging() {
            self.menu_press = None;
            self.drop_list_drag(now);
            return;
        }
        let was_armed = match &self.list_drag {
            ListDrag::Pressing { armed, .. } => *armed,
            _ => true,
        };
        self.list_drag = ListDrag::Idle;
        self.list_drag_visual = None;
        self.menu_drag_layers = None;
        let Some(idx) = self.menu_press.take() else {
            return;
        };
        if !was_armed {
            self.texture_dirty = true;
            return;
        }
        let still_on = self
            .menu_cursor_logical()
            .and_then(|(lx, ly)| {
                self.menu_layout
                    .as_ref()
                    .and_then(|lay| hit_test_index(lay, lx, ly))
            })
            == Some(idx);
        if still_on {
            if let Some(entry) = self
                .menu_layout
                .as_ref()
                .and_then(|lay| lay.items.get(idx))
                .map(|it| it.entry.clone())
            {
                self.handle_menu_entry(entry, Some(idx), now);
            }
        }
        self.texture_dirty = true;
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn update_menu_hover(&mut self) {
        if self.settings_transition.is_some() {
            return;
        }
        if self.list_drag.is_dragging() {
            return;
        }
        if !self
            .pet
            .as_ref()
            .map(|p| p.is_menu_interactive())
            .unwrap_or(false)
        {
            return;
        }
        let Some((lx, ly)) = self.menu_cursor_logical() else {
            return;
        };
        let next = self
            .menu_layout
            .as_ref()
            .and_then(|lay| hit_test_index(lay, lx, ly));
        if next != self.menu_hover {
            self.menu_hover = next;
            self.texture_dirty = true;
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
    }

    fn persist_shortcuts(&mut self) {
        self.config.shortcuts = self.shortcuts.list_sorted();
        if let Some(saver) = self.saver.as_mut() {
            saver.mark_dirty();
        }
    }

    /// Open system file dialog on a **background COM STA thread**.
    /// Avoids freezing the pet event loop (was the main cause of 卡顿).
    fn begin_pick_executable(&mut self) {
        if self.file_picker_busy {
            info!("file picker already open — ignore");
            return;
        }
        let Some(proxy) = self.event_proxy.clone() else {
            warn!("no event proxy — cannot open file picker async");
            return;
        };
        self.file_picker_busy = true;

        // Capture picker settings on the UI thread with the launcher as owner.
        // The native dialog keeps the owner relationship without switching the
        // pet window away from AlwaysOnTop.
        let picker = build_pick_context(self.window.as_deref());

        info!("file picker starting on worker thread");
        let _ = std::thread::Builder::new()
            .name("pawdesk-file-picker".into())
            .spawn(move || {
                // IFileDialog requires STA apartment on Windows.
                #[cfg(windows)]
                unsafe {
                    use windows::Win32::System::Com::{
                        CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED,
                    };
                    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                    let path = pick_executable(picker).ok().flatten();
                    CoUninitialize();
                    if proxy.send_event(UserEvent::FilePicked(path)).is_err() {
                        // Event loop gone
                    }
                }
                #[cfg(not(windows))]
                {
                    let path = pick_executable(picker).ok().flatten();
                    let _ = proxy.send_event(UserEvent::FilePicked(path));
                }
            });
    }

    fn on_file_picked(&mut self, path: Option<PathBuf>) {
        self.file_picker_busy = false;
        let repair = self.file_picker_repair.take();
        if let Some(path) = path {
            if let Some(id) = repair {
                if self.shortcuts.retarget(id, &path) {
                    self.shortcut_icons.clear();
                    self.persist_shortcuts();
                    self.menu_card_cache = None;
                    self.texture_dirty = true;
                    info!(path = %path.display(), "shortcut retargeted (async picker)");
                }
            } else {
                let order = self.shortcuts.items().len() as u32;
                self.shortcuts.add(ShortcutItem::from_path(&path, order));
                self.shortcut_icons.clear();
                self.persist_shortcuts();
                self.menu_card_cache = None;
                self.texture_dirty = true;
                info!(path = %path.display(), "shortcut added (async picker)");
            }
        } else {
            info!("file picker cancelled");
        }
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn handle_menu_entry(&mut self, entry: MenuEntry, item_idx: Option<usize>, now: Instant) {
        match entry {
            MenuEntry::AddShortcut => {
                self.begin_pick_executable();
            }
            MenuEntry::Manage => {
                let anchor = self.menu_anchor_screen_for(item_idx);
                match anchor {
                    Some(a) => self.begin_settings_from_launcher(a, now),
                    None => self.enter_settings_ui(),
                }
            }
            MenuEntry::Shortcut { id, valid, name, .. }
            | MenuEntry::Recent { id, valid, name, .. } => {
                if !valid {
                    warn!(%name, "shortcut path invalid — open picker to repair");
                    if !self.file_picker_busy {
                        self.file_picker_repair = Some(id);
                        self.begin_pick_executable();
                    }
                    return;
                }
                if let Some(item) = self.shortcuts.get(id).cloned() {
                    match launch(&item) {
                        Ok(()) => {
                            if self.shortcuts.record_launch(id) {
                                self.persist_shortcuts();
                            }
                            // Tens/day: close now. Don't make the user wait on a line of copy.
                            self.exit_menu_ui(now);
                        }
                        Err(e) => {
                            warn!(error = %e, "launch failed");
                            self.menu_say = Some((now, SAY_FAIL));
                            self.texture_dirty = true;
                            if let Some(w) = &self.window {
                                w.request_redraw();
                            }
                        }
                    }
                }
            }
        }
    }

    fn handle_settings_hit(&mut self, hit: SettingsHit) {
        let now = Instant::now();
        self.settings_card_cache = None;
        match hit {
            SettingsHit::Done => {
                // Commit the pending pet-size preview. The slot already sits
                // at the previewed gap; pin that desk spot as the new home.
                if let Some(draft) = self.pet_scale_draft.take() {
                    self.config.pet.scale = draft;
                    if let Some(saver) = self.saver.as_mut() {
                        saver.mark_dirty();
                    }
                    info!(scale = draft, "settings: pet size committed");
                }
                if self.settings_embed {
                    self.sync_pet_slot_to_scale(self.config.pet.scale);
                    self.commit_previewed_pet_origin();
                    let grew = self.grow_overlay_to_contain_pet_slot();
                    self.sync_dock_hwnd_slot_from_layout();
                    if grew {
                        if let (Some(w), Some(pos), Some(layout)) = (
                            self.window.as_ref(),
                            self.menu_present_pos,
                            self.menu_layout.as_ref(),
                        ) {
                            let dpr = snap_dpr(self.scale_factor);
                            let tw = logical_to_physical(layout.window_w, dpr).max(1) as u32;
                            let th = logical_to_physical(layout.window_h, dpr).max(1) as u32;
                            if let Err(e) = platform::sync_layered_hwnd(
                                w.as_ref(),
                                pos.0,
                                pos.1,
                                tw,
                                th,
                            ) {
                                warn!("sync_layered_hwnd: {e}");
                            }
                        }
                    }
                }
                self.exit_settings_ui();
            }
            SettingsHit::ToggleEnabled => {
                let next = !self.config.reminder.enabled;
                self.config.reminder.enabled = next;
                if let Some(s) = self.scheduler.as_mut() {
                    s.set_enabled(next, now);
                }
                self.persist_reminder_config();
                self.texture_dirty = true;
                info!(enabled = next, "settings: reminder enabled toggled");
            }
            SettingsHit::IntervalDec => {
                let cur = self.config.reminder.interval_minutes;
                let next = crate::config::clamp_interval_minutes(cur.saturating_sub(15).max(15));
                if next != cur {
                    self.config.reminder.interval_minutes = next;
                    if let Some(s) = self.scheduler.as_mut() {
                        s.set_interval_minutes(next, now);
                    }
                    self.persist_reminder_config();
                    self.texture_dirty = true;
                }
            }
            SettingsHit::IntervalInc => {
                let cur = self.config.reminder.interval_minutes;
                let next = crate::config::clamp_interval_minutes(cur.saturating_add(15));
                if next != cur {
                    self.config.reminder.interval_minutes = next;
                    if let Some(s) = self.scheduler.as_mut() {
                        s.set_interval_minutes(next, now);
                    }
                    self.persist_reminder_config();
                    self.texture_dirty = true;
                }
            }
            SettingsHit::TogglePause => {
                // Button stays; the feature does not. The cat refuses out loud.
                self.menu_say = Some((now, SAY_NO_PAUSE));
                if let Some(pet) = self.pet.as_mut() {
                    pet.begin_sly_pause(now);
                }
                self.texture_dirty = true;
                info!("settings: pause refused (no such feature)");
            }
            SettingsHit::PetScaleDec => {
                self.preview_pet_scale(-1);
            }
            SettingsHit::PetScaleInc => {
                self.preview_pet_scale(1);
            }
        }
    }

    /// Preview pet scale by N steps without committing: updates the draft and
    /// redraws the live preview; only 「完成」 persists it.
    fn preview_pet_scale(&mut self, delta_steps: i32) {
        let base = self.pet_scale_draft.unwrap_or(self.config.pet.scale);
        let next = crate::config::step_pet_scale(base, delta_steps);
        if (next - base).abs() < 0.001 {
            info!(scale = base, "pet scale preview already at limit");
            return;
        }
        self.pet_scale_draft = Some(next);
        if self.settings_embed && self.menu_layout.is_some() {
            self.sync_pet_slot_to_scale(next);
        }
        self.texture_dirty = true;
        if let Some(w) = &self.window {
            w.request_redraw();
        }
        info!(
            preview = next,
            logical = pet_logical_size(next),
            "pet scale preview changed"
        );
    }

    fn layout_rect_on_screen(
        present: (i32, i32),
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        dpr: f64,
    ) -> platform::Rect {
        platform::Rect {
            x: present.0 + (x as f64 * dpr).round() as i32,
            y: present.1 + (y as f64 * dpr).round() as i32,
            width: (w as f64 * dpr).round().max(1.0) as i32,
            height: (h as f64 * dpr).round().max(1.0) as i32,
        }
    }

    /// Move only the pet slot. Card local/screen, overlay size and
    /// `menu_present_pos` stay put — growing the HWND here was cropping the
    /// settings card to half. The slot may go negative; compose clips it.
    fn sync_pet_slot_to_scale(&mut self, scale: f32) {
        let Some(pos) = self.menu_present_pos else {
            return;
        };
        let Some(layout) = self.menu_layout.as_ref() else {
            return;
        };
        let dpr = snap_dpr(self.scale_factor);
        let card = Self::layout_rect_on_screen(
            pos,
            layout.card_x,
            layout.card_y,
            layout.card_w,
            layout.card_h,
            dpr,
        );
        let pet = Self::layout_rect_on_screen(
            pos,
            layout.pet_x,
            layout.pet_y,
            layout.pet_w,
            layout.pet_h,
            dpr,
        );
        let dir = if let Some(snap) = self.settings_layout_snapshot.as_ref() {
            infer_attach_dir(
                Self::layout_rect_on_screen(
                    snap.present_pos,
                    snap.layout.pet_x,
                    snap.layout.pet_y,
                    snap.layout.pet_w,
                    snap.layout.pet_h,
                    dpr,
                ),
                Self::layout_rect_on_screen(
                    snap.present_pos,
                    snap.layout.card_x,
                    snap.layout.card_y,
                    snap.layout.card_w,
                    snap.layout.card_h,
                    dpr,
                ),
            )
        } else {
            infer_attach_dir(pet, card)
        };
        let size = logical_to_physical(pet_logical_size(scale), dpr);
        let (px, py) = match dir {
            crate::ui::radial_menu::ExpandDir::Right => (card.x - DEFAULT_GAP - size, pet.y),
            crate::ui::radial_menu::ExpandDir::Left => (card.x + card.width + DEFAULT_GAP, pet.y),
            crate::ui::radial_menu::ExpandDir::Down => (pet.x, card.y - DEFAULT_GAP - size),
            crate::ui::radial_menu::ExpandDir::Up => (pet.x, card.y + card.height + DEFAULT_GAP),
        };
        let pet_x = physical_to_logical(px - pos.0, dpr);
        let pet_y = physical_to_logical(py - pos.1, dpr);
        let pet_s = physical_to_logical(size, dpr);
        let Some(layout) = self.menu_layout.as_mut() else {
            return;
        };
        layout.pet_x = pet_x;
        layout.pet_y = pet_y;
        layout.pet_w = pet_s;
        layout.pet_h = pet_s;
    }

    fn commit_previewed_pet_origin(&mut self) {
        let Some(layout) = self.menu_layout.as_ref() else {
            return;
        };
        let Some(pos) = self.menu_present_pos else {
            return;
        };
        let dpr = snap_dpr(self.scale_factor);
        let origin = Self::layout_rect_on_screen(
            pos,
            layout.pet_x,
            layout.pet_y,
            layout.pet_w,
            layout.pet_h,
            dpr,
        );
        self.overlay_origin = Some(Point::new(origin.x as f64, origin.y as f64));
        self.config.window.x = Some(origin.x);
        self.config.window.y = Some(origin.y);
        if let Some(saver) = self.saver.as_mut() {
            saver.mark_dirty();
        }
    }

    fn restore_settings_layout_snapshot(&mut self) {
        let Some(snap) = self.settings_layout_snapshot.clone() else {
            return;
        };
        self.menu_layout = Some(snap.layout);
        self.menu_present_pos = Some(snap.present_pos);
        self.menu_logical_size = snap.logical_size;
        self.menu_card_cache = None;
        if let Some(g) = &self.menu_outside_guard {
            let dpr = snap_dpr(self.scale_factor);
            g.update_rect(platform::Rect {
                x: snap.present_pos.0,
                y: snap.present_pos.1,
                width: logical_to_physical(snap.logical_size.0, dpr),
                height: logical_to_physical(snap.logical_size.1, dpr),
            });
        }
        self.texture_dirty = true;
    }

    fn persist_reminder_config(&mut self) {
        self.config.reminder.sanitize();
        if let Some(saver) = self.saver.as_mut() {
            saver.mark_dirty();
        }
    }

    fn sync_tray_tooltip(&mut self) {
        let Some(tray) = self.tray.as_mut() else {
            return;
        };
        let tip = if !self.config.reminder.enabled {
            "PawDesk — 提醒已关闭"
        } else {
            "PawDesk — 桌面互动宠物"
        };
        tray.set_tooltip(tip);
    }

    fn try_start_reminder(&mut self, now: Instant) {
        let Some(pet) = self.pet.as_ref() else {
            return;
        };
        // Already in reminder flow.
        if pet.state.is_reminder() {
            if let Some(s) = self.scheduler.as_mut() {
                s.consume_due();
            }
            return;
        }
        // Hidden: keep due pending until user shows the pet again (M5).
        if !self.visible {
            if let Some(pet) = self.pet.as_mut() {
                pet.pending_reminder = true;
            }
            return;
        }
        if self.menu_ui_active || self.settings_ui_active {
            if let Some(pet) = self.pet.as_mut() {
                pet.pending_reminder = true;
            }
            return;
        }
        if matches!(pet.state, PetState::Dragging) {
            if let Some(pet) = self.pet.as_mut() {
                pet.pending_reminder = true;
            }
            return;
        }

        let mut window_pos = self
            .idle_present_pos
            .map(|(x, y)| Point::new(x as f64, y as f64))
            .or_else(|| self.pet_desk_origin())
            .unwrap_or(Point::new(100.0, 100.0));

        if matches!(
            self.pet.as_ref().map(|p| &p.state),
            Some(PetState::HiddenAtEdge(_))
        ) {
            if let Some(pet) = self.pet.as_mut() {
                if let Some(home) = pet.snap_restore_from_edge(now) {
                    window_pos = home;
                }
            }
        }

        if self.enter_reminder_travel(window_pos, now) {
            if let Some(s) = self.scheduler.as_mut() {
                s.consume_due();
            }
            self.texture_dirty = true;
        }
    }

    fn handle_feed_completed(&mut self, now: Instant) {
        self.config.reminder.last_completed_at = Some(now_rfc3339());
        if let Some(saver) = self.saver.as_mut() {
            saver.mark_dirty();
        }
        if let Some(s) = self.scheduler.as_mut() {
            s.on_feed_completed(now);
        }
        info!("feed completed; last_completed_at updated");
    }

    fn handle_app_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::TrayCommand(cmd) => self.handle_tray(event_loop, cmd),
            AppEvent::RequestExit => {
                self.exit_requested = true;
                self.shutdown(event_loop);
            }
            AppEvent::ReminderDue => {
                let now = Instant::now();
                self.try_start_reminder(now);
            }
            AppEvent::FeedCompleted => {
                let now = Instant::now();
                self.handle_feed_completed(now);
            }
            AppEvent::WindowMoved(_) => {
                self.persist_window_pos();
            }
            other => {
                debug!(?other, "app event");
            }
        }
    }

    fn handle_tray(&mut self, event_loop: &ActiveEventLoop, cmd: TrayCommand) {
        match cmd {
            TrayCommand::Exit => {
                info!("tray: Exit");
                self.exit_requested = true;
                self.shutdown(event_loop);
            }
            TrayCommand::HidePet => {
                if let Some(w) = &self.window {
                    w.set_visible(false);
                    self.visible = false;
                    info!("pet hidden");
                }
            }
            TrayCommand::ShowPet => {
                self.visible = true;
                if let Some(w) = &self.window {
                    w.set_visible(true);
                    // Minimize/hide can leave layered state stale — re-arm + hard present.
                    if let Err(e) = platform::enable_transparent_window(w.as_ref()) {
                        warn!("re-enable layered on show: {e}");
                    }
                    let logical = self.pet_size();
                    self.resize_pet_window(logical, logical);
                }
                self.clamp_window_to_work_area();
                self.texture_dirty = true;
                // Direct present (don't rely only on request_redraw after restore).
                self.redraw();
                info!("pet shown");
                // Deliver any reminder that fired while hidden.
                let now = Instant::now();
                let pending = self
                    .pet
                    .as_ref()
                    .map(|p| p.pending_reminder)
                    .unwrap_or(false)
                    || self
                        .scheduler
                        .as_ref()
                        .map(|s| s.has_pending_due())
                        .unwrap_or(false);
                if pending {
                    if let Some(pet) = self.pet.as_mut() {
                        pet.pending_reminder = false;
                    }
                    self.try_start_reminder(now);
                }
            }
            TrayCommand::PetScaleUp => {
                info!("tray: PetScaleUp");
                self.nudge_pet_scale(1);
            }
            TrayCommand::PetScaleDown => {
                info!("tray: PetScaleDown");
                self.nudge_pet_scale(-1);
            }
        }
    }

    /// Adjust pet scale by N steps (±1 typical). Persists and resizes pet window when idle.
    fn nudge_pet_scale(&mut self, delta_steps: i32) {
        let next = crate::config::step_pet_scale(self.config.pet.scale, delta_steps);
        if (next - self.config.pet.scale).abs() < 0.001 {
            info!(
                scale = self.config.pet.scale,
                "pet scale already at limit"
            );
            return;
        }
        self.config.pet.scale = next;
        if let Some(saver) = self.saver.as_mut() {
            saver.mark_dirty();
        }
        // Apply live when not in an expanded overlay (settings/menu/reminder keep their size).
        let overlay =
            self.settings_ui_active || self.menu_ui_active || self.reminder_ui_active;
        if !overlay {
            let origin = self
                .pet_desk_origin()
                .unwrap_or(Point::new(100.0, 100.0));
            self.clear_dock_hwnd();
            self.begin_idle_present_at(origin);
        } else {
            // Settings panel still open — just refresh the percentage label.
            self.texture_dirty = true;
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
        info!(
            scale = next,
            logical = pet_logical_size(next),
            "pet scale changed"
        );
    }

    fn clamp_window_to_work_area(&mut self) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let Ok(wa) = platform::work_area_for_window(window.as_ref()) else {
            return;
        };
        let Ok(pos) = window.outer_position() else {
            return;
        };
        let size = window.outer_size();
        let (nx, ny) = platform::clamp_top_left_to_work_area(
            pos.x,
            pos.y,
            size.width as i32,
            size.height as i32,
            wa,
        );
        if nx != pos.x || ny != pos.y {
            window.set_outer_position(PhysicalPosition::new(nx, ny));
            info!(nx, ny, "window clamped to work area");
        }
    }

    fn shutdown(&mut self, event_loop: &ActiveEventLoop) {
        info!("shutting down");
        if let Some(saver) = self.saver.as_mut() {
            if let Err(e) = saver.flush(&self.config) {
                warn!("config flush on exit: {e}");
            }
        }
        self.tray = None;
        self.pet = None;
        self.window = None;
        event_loop.exit();
    }

    fn poll_tray(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(tray) = &self.tray {
            let mut cmds = Vec::new();
            while let Some(cmd) = tray.poll_command() {
                cmds.push(cmd);
            }
            for cmd in cmds {
                self.handle_tray(event_loop, cmd);
            }
        }
    }

    fn frame_interval(&self) -> Duration {
        if !self.visible {
            // Hidden: poll tray slowly.
            return Duration::from_millis(200);
        }
        // Menu open/close + hover microinteractions need ~60fps for silk motion.
        // Launcher→settings handoff is the same class of motion.
        if self.menu_ui_active
            || self.settings_transition.is_some()
            || self.reminder_ui_active
        {
            return Duration::from_millis(16);
        }
        if self.settings_ui_active
            || self.drag.dragging
            || self.file_picker_busy
        {
            return Duration::from_millis(33);
        }
        let animating = self.pet.as_ref().map(|p| {
            p.movement.is_active()
                || p.is_playing_cute_action()
                || p.is_crossfading()
                || matches!(
                    p.state,
                    PetState::Reminder(_)
                        | PetState::MenuOpen
                        | PetState::Watching
                        | PetState::Dragging
                        | PetState::HiddenAtEdge(_)
                )
                // Dense idle loops (breath + blink) need ~30fps for smooth sub-frame blend.
                || p.state.is_idle()
        }).unwrap_or(false);
        if animating {
            Duration::from_millis(33)
        } else {
            // RND-07: calmer cadence only when nothing is animating.
            Duration::from_millis(66)
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        if matches!(cause, StartCause::Init) {
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(66),
            ));
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            if let Err(e) = self.create_window(event_loop) {
                error!("failed to create window: {e} ({})", e.user_message());
                event_loop.exit();
            }
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::App(app_event) => self.handle_app_event(event_loop, app_event),
            UserEvent::FilePicked(path) => {
                let _ = event_loop;
                self.on_file_picked(path);
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                info!("window close requested — hiding pet (use tray to exit)");
                if let Some(w) = &self.window {
                    w.set_visible(false);
                }
                self.visible = false;
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } => {
                if !is_synthetic
                    && event.state == ElementState::Pressed
                    && !event.repeat
                    && event.logical_key == Key::Named(NamedKey::Escape)
                    && self.settings_transition.is_none()
                {
                    if self.settings_embed && self.settings_ui_active {
                        self.begin_settings_pop(Instant::now());
                    } else if self.menu_ui_active {
                        self.exit_menu_ui_instant(Instant::now());
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                self.redraw();
            }
            WindowEvent::Resized(_size) => {
                // Layered present uses current inner_size each frame; no swapchain.
                // After minimize/restore Windows may change size — force a clean present.
                if self.idle_present_pos.is_some() {
                    if let Some(w) = &self.window {
                        let inner = w.inner_size();
                        let (pw, ph) = self.idle_target_phys();
                        if inner.width == pw && inner.height == ph {
                            self.idle_present_pos = None;
                        }
                    }
                }
                self.texture_dirty = true;
                if self.visible {
                    self.redraw();
                }
            }
            WindowEvent::Occluded(occluded) => {
                // Restoring from taskbar minimize: re-arm layered + present full pet.
                if !occluded && self.visible {
                    if let Some(w) = &self.window {
                        if let Err(e) = platform::enable_transparent_window(w.as_ref()) {
                            warn!("re-enable layered after un-occlude: {e}");
                        }
                    }
                    self.texture_dirty = true;
                    self.redraw();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if self.menu_ui_active
                    && self
                        .pet
                        .as_ref()
                        .map(|p| p.is_menu_interactive())
                        .unwrap_or(false)
                {
                    let lines = match delta {
                        MouseScrollDelta::LineDelta(_, y) => {
                            if y > 0.1 {
                                1
                            } else if y < -0.1 {
                                -1
                            } else {
                                0
                            }
                        }
                        MouseScrollDelta::PixelDelta(p) => {
                            if p.y > 8.0 {
                                1
                            } else if p.y < -8.0 {
                                -1
                            } else {
                                0
                            }
                        }
                    };
                    self.scroll_menu_list(lines);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_in_window = Point::new(position.x, position.y);
                let now = Instant::now();
                if self.menu_ui_active {
                    self.update_menu_hover();
                    self.tick_list_drag_move();
                }
                // Threshold drag start.
                if self.menu_press.is_none() && !self.list_drag.is_active() && self.drag.consider_drag_start() {
                    // Follow the HWND from the first drag frame; a leftover idle
                    // lock would pin the sprite to the pre-drag desk spot.
                    self.idle_present_pos = None;
                    if let Some(pet) = self.pet.as_mut() {
                        if self.menu_ui_active {
                            pet.close_menu(now);
                            self.menu_ui_active = false;
                            self.menu_layout = None;
                            // Keep overlay_origin for restore after drag end.
                        }
                        if self.yawn_ui_active {
                            self.yawn_ui_active = false;
                            self.yawn_place = None;
                            self.yawn_present_pos = None;
                        }
                        pet.begin_drag(now);
                    }
                    if self.menu_ui_active || self.settings_ui_active {
                        self.settings_ui_active = false;
                        self.settings_transition = None;
                        self.settings_present_pos = None;
                        self.menu_present_pos = None;
                        self.menu_card_cache = None;
                        self.menu_list_scroll = 0;
                        // Keep overlay_origin for restore after drag end.
                    }
                    if self.reminder_ui_active {
                        let pos = self.current_reminder_pet_screen();
                        self.exit_reminder_overlay_to_pet(pos);
                    } else if self.settings_ui_active {
                        self.settings_ui_active = false;
                        self.settings_transition = None;
                        self.settings_present_pos = None;
                        let s = self.pet_size();
                        self.resize_pet_window(s, s);
                    } else if self.overlay_origin.is_some()
                        && !self.menu_ui_active
                        && self.dock_hwnd.is_none()
                    {
                        if let Some(o) = self.overlay_origin {
                            self.begin_idle_present_at(o);
                        }
                    }
                    // Drag owns the HWND now — don't keep the restore lock.
                    self.idle_present_pos = None;
                    self.texture_dirty = true;
                }
                if let Some(w) = self.window.as_ref() {
                    self.drag.apply_drag(w);
                    if self.drag.dragging {
                        if let Some(d) = self.dock_hwnd.as_mut() {
                            if let Ok(pos) = w.outer_position() {
                                d.pos = (pos.x, pos.y);
                            }
                        }
                        self.persist_window_pos();
                    }
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let now = Instant::now();

                // --- Press: settings / menu / food / press-pending ---
                if (button, state) == (WinitMouseButton::Left, ElementState::Pressed) {
                    // Settings hits (only after the open transition settles).
                    if self.settings_ui_active && self.settings_transition.is_none() {
                        if self.settings_embed {
                            if let (Some((lx, ly)), Some(layout)) =
                                (self.menu_cursor_logical(), self.menu_layout.as_ref())
                            {
                                if let Some(hit) = hit_settings_card(
                                    lx - layout.card_x,
                                    ly - layout.card_y,
                                    layout.card_w,
                                    layout.card_h,
                                ) {
                                    self.handle_settings_hit(hit);
                                    if let Some(w) = &self.window {
                                        w.request_redraw();
                                    }
                                    return;
                                }
                            }
                        } else {
                            let (cw, ch) = self
                                .window
                                .as_ref()
                                .map(|w| {
                                    let s = w.inner_size();
                                    (s.width as f64, s.height as f64)
                                })
                                .unwrap_or((SETTINGS_W as f64, SETTINGS_H as f64));
                            let lx = self.cursor_in_window.x / cw.max(1.0) * SETTINGS_W as f64;
                            let ly = self.cursor_in_window.y / ch.max(1.0) * SETTINGS_H as f64;
                            if let Some(hit) = hit_settings(lx as f32, ly as f32) {
                                self.handle_settings_hit(hit);
                                if let Some(w) = &self.window {
                                    w.request_redraw();
                                }
                                return;
                            }
                        }
                    }

                    // Menu hits. Press highlights immediately; commit is on release.
                    if self.menu_ui_active {
                        if self.settings_transition.is_some() {
                            return;
                        }
                        if self.settings_embed {
                            if let Some((lx, ly)) = self.menu_cursor_logical() {
                                if let Some(layout) = self.menu_layout.as_ref() {
                                    if hit_center(layout, lx, ly) {
                                        self.exit_menu_ui(now);
                                        if let Some(w) = &self.window {
                                            w.request_redraw();
                                        }
                                    }
                                }
                            }
                            return;
                        }
                        let closing = self
                            .pet
                            .as_ref()
                            .map(|p| p.menu_closing)
                            .unwrap_or(false);
                        if closing {
                            // Grab mid-close: reverse from the live visual.
                            if let Some(pet) = self.pet.as_mut() {
                                pet.open_menu(now);
                            }
                            self.texture_dirty = true;
                            if let Some(w) = &self.window {
                                w.request_redraw();
                            }
                            return;
                        }
                        if let Some((lx, ly)) = self.menu_cursor_logical() {
                            if let Some(layout) = self.menu_layout.clone() {
                                if let Some(idx) = hit_test_index(&layout, lx, ly) {
                                    self.menu_press = Some(idx);
                                    self.menu_hover = Some(idx);
                                    self.begin_list_press(idx, lx, ly, now);
                                    self.texture_dirty = true;
                                    if let Some(w) = &self.window {
                                        w.request_redraw();
                                    }
                                    return;
                                }
                                if hit_center(&layout, lx, ly) {
                                    self.exit_menu_ui(now);
                                    if let Some(w) = &self.window {
                                        w.request_redraw();
                                    }
                                    return;
                                }
                                self.exit_menu_ui(now);
                                if let Some(w) = &self.window {
                                    w.request_redraw();
                                }
                                return;
                            }
                        }
                    }

                    // Food button
                    if self.reminder_ui_active {
                        if let Some(pet) = self.pet.as_ref() {
                            if matches!(pet.state, PetState::Reminder(ReminderStage::Showing))
                                && self.reminder_card_t > 0.8
                            {
                                let (lx, ly) = if let Some(place) = self.reminder_place {
                                    let dpr = snap_dpr(self.scale_factor).max(0.01);
                                    (
                                        (self.cursor_in_window.x - place.card_local.x as f64)
                                            / dpr,
                                        (self.cursor_in_window.y - place.card_local.y as f64)
                                            / dpr,
                                    )
                                } else {
                                    (self.cursor_in_window.x, self.cursor_in_window.y)
                                };
                                let hit = pet.hit_food_button(lx, ly)
                                    || feed_zone_hit(lx, ly);
                                if hit {
                                    if let Some(pet) = self.pet.as_mut() {
                                        if pet.on_feed_click(now) {
                                            self.feed_persist_pending = true;
                                            self.texture_dirty = true;
                                            if let Some(w) = &self.window {
                                                w.request_redraw();
                                            }
                                            return;
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // HiddenAtEdge restore on press
                    if let Some(pet) = self.pet.as_ref() {
                        if let PetState::HiddenAtEdge(edge) = &pet.state {
                            if let (Some(w), Ok(cursor)) =
                                (self.window.as_ref(), platform::cursor_pos())
                            {
                                if let Ok(win_pos) = w.outer_position() {
                                    let win_size = w.outer_size();
                                    let click = Point::new(cursor.0 as f64, cursor.1 as f64);
                                    let win_pt = Point::new(win_pos.x as f64, win_pos.y as f64);
                                    if crate::pet::InteractionDetector::is_in_peek_area(
                                        click,
                                        win_pt,
                                        *edge,
                                        win_size.width,
                                    ) {
                                        if let Some(pet) = self.pet.as_mut() {
                                            pet.restore_from_edge(now);
                                            self.texture_dirty = true;
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Begin press (not drag yet)
                    let suppress = self.reminder_ui_active
                        && matches!(
                            self.pet.as_ref().map(|p| &p.state),
                            Some(PetState::Reminder(ReminderStage::Showing))
                                | Some(PetState::Reminder(ReminderStage::Feeding))
                        );
                    if !suppress {
                        let _ = self
                            .drag
                            .on_mouse_input(button, state, self.cursor_in_window);
                    }
                }

                // --- Release: click opens menu or ends drag ---
                if (button, state) == (WinitMouseButton::Left, ElementState::Released) {
                    if self.menu_ui_active && (self.menu_press.is_some() || self.list_drag.is_active()) {
                        self.commit_menu_press(now);
                    }
                    let was_dragging = self.drag.dragging;
                    let was_click = self.drag.finish_press();
                    let _ = self
                        .drag
                        .on_mouse_input(button, state, self.cursor_in_window);

                    if was_dragging {
                        let pending = self
                            .pet
                            .as_ref()
                            .map(|p| p.pending_reminder)
                            .unwrap_or(false);
                        if let Some(pet) = self.pet.as_mut() {
                            pet.end_drag(now);
                            self.texture_dirty = true;
                        }
                        // Restore size if we left an overlay for drag
                        if self.overlay_origin.is_some() {
                            self.restore_overlay_origin_window();
                        }
                        self.clamp_window_to_work_area();
                        self.persist_window_pos();
                        if pending {
                            if let Some(s) = self.scheduler.as_mut() {
                                s.defer_due();
                            }
                            self.try_start_reminder(now);
                        }
                    } else if was_click
                        && !self.reminder_ui_active
                        && !self.settings_ui_active
                        && !self.menu_ui_active
                    {
                        if self.yawn_ui_active && self.yawn_hit_bubble() {
                            // Bubble is not a launcher hit target.
                        } else {
                            // Debounce: avoid reopen on the same click that closed, or double-tap.
                            let reopen_ok = self
                                .menu_reopen_after
                                .map(|t| now >= t)
                                .unwrap_or(true);
                            // Click pet → launcher (Idle base / cute / Watching / edge peek).
                            let can_open = reopen_ok
                                && matches!(
                                    self.pet.as_ref().map(|p| &p.state),
                                    Some(PetState::Idle(_))
                                        | Some(PetState::Watching)
                                        | Some(PetState::HiddenAtEdge(_))
                                );
                            if self.yawn_ui_active {
                                if can_open {
                                    // Opening the dock next — do not shrink back to
                                    // idle first (that wipe + grow is a guaranteed flash).
                                    self.yawn_ui_active = false;
                                    self.yawn_place = None;
                                    self.yawn_present_pos = None;
                                } else {
                                    self.exit_yawn_ui();
                                }
                            }
                            if can_open {
                                self.menu_reopen_after = None;
                                self.enter_menu_ui(now);
                            }
                        }
                    }
                }

                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            WindowEvent::Moved(pos) => {
                self.handle_app_event(
                    event_loop,
                    AppEvent::WindowMoved(Point::new(pos.x as f64, pos.y as f64)),
                );
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.scale_factor = scale_factor;
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.exit_requested {
            return;
        }

        self.poll_tray(event_loop);
        self.update_click_through();

        if let Some(saver) = self.saver.as_mut() {
            if let Err(e) = saver.tick(&self.config) {
                warn!("config debounced save: {e}");
            }
        }

        let now = Instant::now();
        if now.duration_since(self.last_frame) >= self.frame_interval() {
            let dt = now
                .duration_since(self.last_frame)
                .as_secs_f32()
                .clamp(0.0, 0.05);
            self.last_frame = now;

            if !self.visible {
                // Still tick scheduler so due can pend while hidden.
                let due = self
                    .scheduler
                    .as_mut()
                    .and_then(|s| s.tick(now))
                    .is_some()
                    || self
                        .scheduler
                        .as_ref()
                        .map(|s| s.has_pending_due())
                        .unwrap_or(false);
                if due {
                    if let Some(pet) = self.pet.as_mut() {
                        pet.pending_reminder = true;
                    }
                }
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    Instant::now() + self.frame_interval(),
                ));
                return;
            }

            if self.drag.dragging {
                if let Some(w) = &self.window {
                    self.drag.apply_drag(w);
                    if let Some(d) = self.dock_hwnd.as_mut() {
                        if let Ok(pos) = w.outer_position() {
                            d.pos = (pos.x, pos.y);
                        }
                    }
                }
            }

            let mut need_redraw = self.drag.dragging
                || self.reminder_ui_active
                || self.menu_ui_active
                || self.settings_ui_active
                || self.texture_dirty;
            // Present menu frames immediately in this tick (skip extra RedrawRequested hop).
            let mut present_now = false;

            // Launcher → settings handoff animation (grow + card crossfade).
            if self.settings_ui_active && self.settings_transition.is_some() {
                if self.tick_settings_transition(now) {
                    need_redraw = true;
                    present_now = true;
                } else {
                    self.texture_dirty = true;
                    need_redraw = true;
                    present_now = true;
                }
            }

            // Menu open/close animation (L3) + Appica hover/press blends
            if let Some(pet) = self.pet.as_mut() {
                let (animating, close_done) = pet.tick_menu_anim(now);
                if close_done {
                    self.finish_exit_menu_ui(now);
                    need_redraw = true;
                    present_now = true;
                } else if animating || pet.is_menu_open() {
                    need_redraw = true;
                    self.texture_dirty = true;
                    if animating {
                        present_now = true;
                    }
                }
            }
            if let Some((t0, line)) = self.menu_say {
                let hold_ms = if line == SAY_NO_PAUSE { 2800 } else { 1400 };
                let elapsed = now.saturating_duration_since(t0);
                if elapsed >= Duration::from_millis(hold_ms) {
                    self.menu_say = None;
                    self.texture_dirty = true;
                    need_redraw = true;
                } else {
                    need_redraw = true;
                }
            }
            if self.menu_ui_active {
                self.tick_list_drag_hold(now);
                let ht = if self.menu_hover.is_some() { 1.0 } else { 0.0 };
                let pt = if self.menu_press.is_some() && self.menu_hover == self.menu_press {
                    1.0
                } else {
                    0.0
                };
                let nh = crate::render::easing::approach(self.menu_hover_t, ht, 14.0, dt);
                let np = crate::render::easing::approach(self.menu_press_t, pt, 18.0, dt);
                if (nh - self.menu_hover_t).abs() > 0.002 || (np - self.menu_press_t).abs() > 0.002
                {
                    self.menu_hover_t = nh;
                    self.menu_press_t = np;
                    self.texture_dirty = true;
                    need_redraw = true;
                    present_now = true;
                } else {
                    self.menu_hover_t = nh;
                    self.menu_press_t = np;
                }
            }

            // Scheduler tick.
            if let Some(s) = self.scheduler.as_mut() {
                if s.tick(now).is_some() {
                    // Deliver after borrow ends.
                    need_redraw = true;
                    // Flag via temporary: call try_start after mut borrow of scheduler ends.
                }
            }
            // Re-check due without double-borrow issues.
            let due = self
                .scheduler
                .as_ref()
                .map(|s| s.has_pending_due())
                .unwrap_or(false);
            if due {
                let busy = self
                    .pet
                    .as_ref()
                    .map(|p| p.state.is_reminder() || matches!(p.state, PetState::Dragging))
                    .unwrap_or(false);
                if !busy {
                    self.try_start_reminder(now);
                } else if matches!(
                    self.pet.as_ref().map(|p| &p.state),
                    Some(PetState::Dragging)
                ) {
                    if let Some(pet) = self.pet.as_mut() {
                        pet.pending_reminder = true;
                    }
                }
            }

            // Movement.
            let movement_was_active = self
                .pet
                .as_ref()
                .map(|p| p.movement.is_active())
                .unwrap_or(false);
            let mut entered_showing = false;
            let mut returned_idle = false;
            let reminder_home = self
                .pet
                .as_ref()
                .and_then(|p| p.reminder_origin);
            let mut hop_slot: Option<Point> = None;
            let mut hwnd_slide: Option<Point> = None;
            if movement_was_active {
                if let Some(pet) = self.pet.as_mut() {
                    let reminder_hop = pet.is_reminder_moving();
                    if let Some(new_pos) = pet.update_movement(now) {
                        if reminder_hop {
                            hop_slot = Some(new_pos);
                        } else {
                            hwnd_slide = Some(new_pos);
                        }
                    }
                    if movement_was_active
                        && !pet.movement.is_active()
                        && pet.on_movement_complete(now)
                    {
                        self.texture_dirty = true;
                        entered_showing =
                            matches!(pet.state, PetState::Reminder(ReminderStage::Showing));
                        returned_idle = pet.state.is_idle();
                    }
                }
            }
            if let Some(slot) = hop_slot {
                self.reminder_slot = Some(slot);
                need_redraw = true;
                present_now = true;
            }
            if let Some(pos) = hwnd_slide {
                if let Some(w) = &self.window {
                    w.set_outer_position(PhysicalPosition::new(pos.x as i32, pos.y as i32));
                }
                need_redraw = true;
            }
            if entered_showing {
                if let Some(place) = self.reminder_place {
                    self.reminder_slot = Some(Point::new(
                        place.dest_local.x as f64,
                        place.dest_local.y as f64,
                    ));
                }
                self.start_reminder_card_reveal(now);
                present_now = true;
            }
            if returned_idle {
                let home = reminder_home.unwrap_or_else(|| self.current_reminder_pet_screen());
                self.exit_reminder_overlay_to_pet(home);
                self.persist_window_pos();
            }

            if self.tick_reminder_card_anim(now) {
                need_redraw = true;
                present_now = true;
            }

            // Gaze follows the screen cursor whenever we can see the pet.
            let cursor_pt = platform::cursor_pos()
                .ok()
                .map(|(x, y)| Point::new(x as f64, y as f64));
            let window_info = self.window.as_ref().and_then(|w| {
                w.outer_position().ok().map(|pos| {
                    let size = w.outer_size();
                    (pos, size)
                })
            });
            if let (Some(cursor), Some((pos, size)), Some(pet)) =
                (cursor_pt, window_info, self.pet.as_mut())
            {
                let pet_center = if self.menu_ui_active {
                    if let (Some((wx, wy)), Some(lay)) =
                        (self.menu_present_pos, self.menu_layout.as_ref())
                    {
                        let d = snap_dpr(self.scale_factor);
                        Point::new(
                            wx as f64 + (lay.pet_x as f64 + lay.pet_w as f64 * 0.5) * d,
                            wy as f64 + (lay.pet_y as f64 + lay.pet_h as f64 * 0.5) * d,
                        )
                    } else {
                        Point::new(
                            pos.x as f64 + size.width as f64 / 2.0,
                            pos.y as f64 + size.height as f64 / 2.0,
                        )
                    }
                } else if let Some(d) = self.dock_hwnd {
                    let dpr = snap_dpr(self.scale_factor);
                    Point::new(
                        d.pos.0 as f64 + (d.pet_x as f64 + d.pet_w as f64 * 0.5) * dpr,
                        d.pos.1 as f64 + (d.pet_y as f64 + d.pet_h as f64 * 0.5) * dpr,
                    )
                } else {
                    Point::new(
                        pos.x as f64 + size.width as f64 / 2.0,
                        pos.y as f64 + size.height as f64 / 2.0,
                    )
                };
                let track = !self.drag.dragging && !pet.state.is_reminder();
                pet.update_gaze(cursor, pet_center, track);
            }

            // Interaction + edge (not while dragging / reminder / menu / settings).
            if !self.drag.dragging {
                let in_reminder = self
                    .pet
                    .as_ref()
                    .map(|p| p.state.is_reminder())
                    .unwrap_or(false);
                let overlay = self.menu_ui_active || self.settings_ui_active;
                if !in_reminder && !overlay {
                    let work_area = self
                        .window
                        .as_ref()
                        .and_then(|w| platform::work_area_for_window(w.as_ref()).ok());

                    if let (Some(cursor), Some((pos, size))) = (cursor_pt, window_info) {
                        let dpr = snap_dpr(self.scale_factor);
                        let (window_top_left, pet_center, pet_rect, win_w, win_h) =
                            if let Some(d) = self.dock_hwnd {
                                let px = d.pos.0 as f64 + d.pet_x as f64 * dpr;
                                let py = d.pos.1 as f64 + d.pet_y as f64 * dpr;
                                let pw = (d.pet_w as f64 * dpr).max(1.0);
                                let ph = (d.pet_h as f64 * dpr).max(1.0);
                                (
                                    Point::new(px, py),
                                    Point::new(px + pw * 0.5, py + ph * 0.5),
                                    platform::Rect {
                                        x: px.round() as i32,
                                        y: py.round() as i32,
                                        width: pw.round() as i32,
                                        height: ph.round() as i32,
                                    },
                                    pw,
                                    ph,
                                )
                            } else {
                                (
                                    Point::new(pos.x as f64, pos.y as f64),
                                    Point::new(
                                        pos.x as f64 + size.width as f64 / 2.0,
                                        pos.y as f64 + size.height as f64 / 2.0,
                                    ),
                                    platform::Rect {
                                        x: pos.x,
                                        y: pos.y,
                                        width: size.width as i32,
                                        height: size.height as i32,
                                    },
                                    size.width as f64,
                                    size.height as f64,
                                )
                            };
                        if let Some(pet) = self.pet.as_mut() {
                            if pet.update_interaction(
                                cursor,
                                pet_center,
                                window_top_left,
                                win_w,
                                win_h,
                                now,
                            ) {
                                self.texture_dirty = true;
                                need_redraw = true;
                            }
                            if self.config.pet.edge_hide_enabled {
                                if let Some(wa) = work_area {
                                    let pet_rect = pet_rect;
                                    if pet.update_edge(pet_rect, wa, now).is_some() {
                                        self.texture_dirty = true;
                                        need_redraw = true;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Animation + interaction end + feed end.
            let mut playing_yawn = false;
            let reminder_card_animating = self.reminder_card_anim.is_some();
            if let Some(pet) = self.pet.as_mut() {
                let prev_clip = pet.player.clip_name().to_string();
                let prev_f = pet.display_frame_f;
                if pet.tick(now) {
                    need_redraw = true;
                    // Any sub-frame motion dirties texture for smooth present.
                    if pet.player.clip_name() != prev_clip
                        || (pet.display_frame_f - prev_f).abs() > 0.0005
                    {
                        self.texture_dirty = true;
                    }
                }
                playing_yawn = pet.is_playing_yawn();
                // Dense sprite motion: present *in this tick* (layered windows often
                // drop/coalesce RedrawRequested, so request_redraw alone can freeze on
                // frame 0 of a oneshot — log says "action started" but no visible motion).
                let pet_motion = pet.is_crossfading()
                    || pet.is_playing_cute_action()
                    || pet.state.is_idle()
                    || matches!(
                        pet.state,
                        PetState::Watching
                            | PetState::Dragging
                            | PetState::HiddenAtEdge(_)
                            | PetState::Reminder(_)
                    );
                if pet_motion {
                    need_redraw = true;
                    self.texture_dirty = true;
                    // Direct present for clip playback (same path as menu open silk).
                    if pet.is_playing_cute_action()
                        || pet.is_crossfading()
                        || pet.is_reminder_moving()
                        || reminder_card_animating
                        || matches!(
                            pet.state,
                            PetState::Watching
                                | PetState::Dragging
                                | PetState::HiddenAtEdge(_)
                        )
                        || pet.state.is_idle()
                    {
                        present_now = true;
                    }
                }
            }
            if playing_yawn {
                if !self.yawn_ui_active {
                    self.enter_yawn_ui();
                }
            } else if self.yawn_ui_active {
                self.exit_yawn_ui();
            }

            // Feed animation done → fade card out, then overlay-hop home.
            let feed_done = self
                .pet
                .as_mut()
                .map(|p| p.feed_animation_done(now))
                .unwrap_or(false);
            if feed_done {
                if self.feed_persist_pending {
                    self.handle_feed_completed(now);
                    self.feed_persist_pending = false;
                }
                self.start_reminder_card_dismiss(now);
                self.texture_dirty = true;
                need_redraw = true;
                present_now = true;
            }

            if present_now && self.visible {
                // Direct present for open/close — lower latency than request_redraw hop.
                self.redraw();
            } else if need_redraw || self.texture_dirty {
                if let Some(w) = &self.window {
                    if self.visible {
                        w.request_redraw();
                    }
                }
            }

            self.handle_app_event(event_loop, AppEvent::Tick(now));
        }

        // Launcher outside-click guard: keep the window rect synced and close
        // the dock when a left click lands outside it (desktop / other apps).
        let now = Instant::now();
        // Skip during launcher→settings transition: clicking outside then would
        // abort the in-flight grow animation (was previously ignored).
        if self.menu_ui_active
            && self.settings_transition.is_none()
            && !self.list_drag.is_dragging()
        {
            let outside_clicked = {
                let guard = self.menu_outside_guard.as_ref();
                if let (Some(guard), Some(w)) = (guard, &self.window) {
                    if let Ok(pos) = w.outer_position() {
                        let size = w.outer_size();
                        guard.update_rect(platform::Rect {
                            x: pos.x as i32,
                            y: pos.y as i32,
                            width: size.width as i32,
                            height: size.height as i32,
                        });
                    }
                    platform::OutsideClickGuard::take_outside_click()
                } else {
                    false
                }
            };
            if outside_clicked {
                info!("launcher: outside click -> close");
                self.exit_menu_ui(now);
            }
        }

        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + self.frame_interval(),
        ));
    }
}

/// Click zone matching the visible feed pill (layout coords).
fn feed_zone_hit(lx: f64, ly: f64) -> bool {
    let (x, y, w, h) = food_button_layout();
    let pad = 8.0;
    lx >= (x - pad) as f64
        && ly >= (y - pad) as f64
        && lx <= (x + w + pad) as f64
        && ly <= (y + h + pad) as f64
}

/// Resolve assets for both `cargo run` and portable release layouts.
fn asset_root() -> PathBuf {
    // 1) Next to the executable (portable dist/PawDesk/assets).
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let next = dir.join("assets");
            if next.is_dir() {
                return next;
            }
            // 2) Parent of exe (e.g. target/release → project assets when testing)
            if let Some(parent) = dir.parent() {
                let up = parent.join("assets");
                if up.is_dir() {
                    return up;
                }
                if let Some(grand) = parent.parent() {
                    let g = grand.join("assets");
                    if g.is_dir() {
                        return g;
                    }
                }
            }
        }
    }
    // 3) Dev fallback
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets")
}

/// Overlay UI path: letterbox with **nearest** sampling (no bilinear soft blur).
fn scale_rgba_centered_crisp(
    src: &[u8],
    sw: u32,
    sh: u32,
    dw: u32,
    dh: u32,
) -> Vec<u8> {
    let mut out = vec![0u8; (dw * dh * 4) as usize];
    if sw == 0 || sh == 0 || src.len() < (sw * sh * 4) as usize {
        return out;
    }
    if sw == dw && sh == dh {
        out.copy_from_slice(&src[..(dw * dh * 4) as usize]);
        return out;
    }
    let fit = (dw as f64 / sw as f64).min(dh as f64 / sh as f64);
    let fw = ((sw as f64) * fit).round().max(1.0) as u32;
    let fh = ((sh as f64) * fit).round().max(1.0) as u32;
    let ox = (dw.saturating_sub(fw) / 2) as i32;
    let oy = (dh.saturating_sub(fh) / 2) as i32;
    for dy in 0..fh {
        for dx in 0..fw {
            let sx = ((dx as f64 + 0.5) * sw as f64 / fw as f64).floor() as u32;
            let sy = ((dy as f64 + 0.5) * sh as f64 / fh as f64).floor() as u32;
            let sx = sx.min(sw - 1);
            let sy = sy.min(sh - 1);
            let si = ((sy * sw + sx) * 4) as usize;
            let px = ox + dx as i32;
            let py = oy + dy as i32;
            if px < 0 || py < 0 || px >= dw as i32 || py >= dh as i32 {
                continue;
            }
            let di = ((py as u32 * dw + px as u32) * 4) as usize;
            out[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
    out
}

/// Scale `src` (sw×sh RGBA) into a transparent `dw×dh` canvas with optional uniform scale.
///
/// Upscales to fill the destination (needed on high-DPI: 128 logical → 256 physical).
/// Uses bilinear sampling so cartoon edges stay smooth instead of blocky nearest-neighbor.
///
/// **Vertical align = bottom-weighted** (feet/anchor sit on the desk edge of the window)
/// so minimize/restore or slight size jitter does not crop paws the way pure centering can.
fn scale_rgba_centered(
    src: &[u8],
    sw: u32,
    sh: u32,
    dw: u32,
    dh: u32,
    scale: f32,
    mirror_x: bool,
) -> Vec<u8> {
    let mut out = vec![0u8; (dw * dh * 4) as usize];
    if sw == 0 || sh == 0 || src.len() < (sw * sh * 4) as usize {
        return out;
    }
    let scale = scale.clamp(0.5, 2.0) as f64;
    let tw = ((sw as f64) * scale).max(1.0);
    let th = ((sh as f64) * scale).max(1.0);
    // Fill destination while preserving aspect (allow upscale on high-DPI).
    let fit = (dw as f64 / tw).min(dh as f64 / th);
    let fw = (tw * fit).round().max(1.0) as u32;
    let fh = (th * fit).round().max(1.0) as u32;
    // Safe margin: extra on the bottom so soft paw AA is never flush with HWND edge.
    let margin_x = 2u32.min(fw / 16);
    let margin_top = 2u32.min(fh / 16);
    let margin_bot = 4u32.min(fh / 12).max(3);
    let fw = fw.saturating_sub(margin_x * 2).max(1);
    let fh = fh
        .saturating_sub(margin_top + margin_bot)
        .max(1);
    let ox = ((dw.saturating_sub(fw)) / 2) as i32;
    // Bottom-align content inside the drawable area (feet down).
    let oy = (dh.saturating_sub(fh + margin_bot)) as i32;

    let sw_f = sw as f64;
    let sh_f = sh as f64;
    let fw_f = fw as f64;
    let fh_f = fh as f64;

    // Scale factor in source-texels per dest pixel. >1 = downscale.
    let scale_x = sw_f / fw_f;
    let scale_y = sh_f / fh_f;
    // Downscale or near 1:1: bilinear keeps silhouette AA soft.
    // Strong upscale of face features used to smear nose/mouth — still prefer bilinear
    // for edges now that sprites carry a soft alpha ramp; interior stays crisp because
    // neighboring texels share the same fur color.
    for dy in 0..fh {
        for dx in 0..fw {
            let src_dx = if mirror_x { fw - 1 - dx } else { dx };
            // Map dest pixel center into source continuous coords.
            let sx = (src_dx as f64 + 0.5) * scale_x - 0.5;
            let sy = (dy as f64 + 0.5) * scale_y - 0.5;
            let sample = sample_rgba_bilinear(src, sw, sh, sx, sy);
            let px = ox + dx as i32;
            let py = oy + dy as i32;
            if px < 0 || py < 0 || px >= dw as i32 || py >= dh as i32 {
                continue;
            }
            let di = ((py as u32 * dw + px as u32) * 4) as usize;
            out[di..di + 4].copy_from_slice(&sample);
        }
    }
    out
}

/// Multiply alpha on a tightly-packed RGBA8 buffer (straight-alpha fade).
fn fade_rgba_alpha(rgba: &mut [u8], alpha: f32) {
    let alpha = alpha.clamp(0.0, 1.0);
    for c in rgba.chunks_exact_mut(4) {
        c[3] = (c[3] as f32 * alpha).round() as u8;
    }
}

/// Scale an RGBA buffer around its anchor point, with optional global fade.
///
/// Used by the settings open transition: the panel bitmap is scaled by `scale`
/// while `fade` is applied to alpha, so it grows outward from the clicked
/// launcher button without an oversized intermediate buffer.
fn scale_rgba_around_anchor(
    src: &[u8],
    sw: u32,
    sh: u32,
    scale: f32,
    fade: f32,
) -> (Vec<u8>, u32, u32) {
    let scale = scale.clamp(0.05, 3.0) as f64;
    let dw = ((sw as f64) * scale).round().max(1.0) as u32;
    let dh = ((sh as f64) * scale).round().max(1.0) as u32;
    let mut out = vec![0u8; (dw * dh * 4) as usize];
    if sw == 0 || sh == 0 || src.len() < (sw * sh * 4) as usize {
        return (out, dw, dh);
    }
    let scale_x = sw as f64 / dw as f64;
    let scale_y = sh as f64 / dh as f64;
    let alpha = fade.clamp(0.0, 1.0);
    for dy in 0..dh {
        for dx in 0..dw {
            // Sample back through the same center-relative scaling.
            let sx = (dx as f64 + 0.5) * scale_x - 0.5;
            let sy = (dy as f64 + 0.5) * scale_y - 0.5;
            let mut c = sample_rgba_bilinear(src, sw, sh, sx, sy);
            if alpha < 1.0 {
                c[3] = (c[3] as f32 * alpha).round() as u8;
            }
            let i = ((dy * dw + dx) * 4) as usize;
            out[i..i + 4].copy_from_slice(&c);
        }
    }
    (out, dw, dh)
}

fn init_logging() -> Result<(), AppError> {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::EnvFilter;

    let base = dirs_log_dir()?;
    std::fs::create_dir_all(&base).map_err(|source| AppError::Io {
        path: base.clone(),
        source,
    })?;

    let file_appender = tracing_appender::rolling::never(&base, "app.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);
    std::mem::forget(guard);

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new("info,wgpu_hal=warn,wgpu_core=warn,naga=warn,wgpu=warn")
    });

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_target(false),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false)
                .with_target(true),
        )
        .try_init();

    info!(path = %base.join("app.log").display(), "logging initialized");
    Ok(())
}

fn dirs_log_dir() -> Result<PathBuf, AppError> {
    let local = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| AppError::Platform("LOCALAPPDATA is not set".into()))?;
    Ok(PathBuf::from(local).join("PawDesk").join("logs"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fade_rgba_scales_only_alpha() {
        let mut px = vec![10u8, 20, 30, 255, 40, 50, 60, 128];
        fade_rgba_alpha(&mut px, 0.5);
        assert_eq!(&px[0..3], &[10, 20, 30]);
        assert_eq!(px[3], 128);
        assert_eq!(&px[4..7], &[40, 50, 60]);
        assert_eq!(px[7], 64);
    }

    fn seed_embedded_settings(app: &mut App, scale: f32) {
        let pet = pet_logical_size(scale) as f32;
        app.scale_factor = 1.0;
        app.menu_ui_active = true;
        app.settings_ui_active = true;
        app.settings_embed = true;
        app.menu_logical_size = (400, 400);
        app.menu_present_pos = Some((40, 50));
        let layout = layout_pinned_scroll(
            &[],
            400,
            400,
            (8.0, 80.0, pet, pet),
            (100.0, 20.0, 360.0, 450.0),
            crate::ui::radial_menu::ExpandDir::Right,
            1.0,
            0,
        );
        app.settings_layout_snapshot = Some(SettingsLayoutSnapshot {
            layout: layout.clone(),
            present_pos: (40, 50),
            logical_size: (400, 400),
        });
        app.menu_layout = Some(layout);
    }

    fn card_screen_of(app: &App) -> platform::Rect {
        let lay = app.menu_layout.as_ref().expect("layout");
        let pos = app.menu_present_pos.expect("present");
        App::layout_rect_on_screen(
            pos,
            lay.card_x,
            lay.card_y,
            lay.card_w,
            lay.card_h,
            snap_dpr(app.scale_factor),
        )
    }

    fn pet_screen_of(app: &App) -> platform::Rect {
        let lay = app.menu_layout.as_ref().expect("layout");
        let pos = app.menu_present_pos.expect("present");
        App::layout_rect_on_screen(
            pos,
            lay.pet_x,
            lay.pet_y,
            lay.pet_w,
            lay.pet_h,
            snap_dpr(app.scale_factor),
        )
    }

    #[test]
    fn done_pins_pet_to_card_gap_and_keeps_card() {
        let repo = crate::config::ConfigRepository::default_paths().expect("config paths");
        let saver = crate::config::DebouncedSaver::new(repo);
        let config = crate::config::AppConfig::default();
        let mut app = App::new(PathBuf::from("."), config, saver);
        let old_scale = app.config.pet.scale;
        seed_embedded_settings(&mut app, old_scale);
        let card_before = card_screen_of(&app);
        app.pet_scale_draft = Some(0.9);

        app.handle_settings_hit(SettingsHit::Done);

        assert!(
            (app.config.pet.scale - 0.9).abs() < 0.001,
            "draft must be committed"
        );
        assert!(app.pet_scale_draft.is_none(), "committed draft must be cleared");
        let card_after = card_screen_of(&app);
        assert_eq!(card_after, card_before, "card screen rect must stay locked");
        let pet = pet_screen_of(&app);
        let new_pet = pet_logical_size(0.9) as i32;
        assert_eq!(pet.width, new_pet);
        assert_eq!(pet.height, new_pet);
        assert_eq!(
            pet.x + pet.width + DEFAULT_GAP,
            card_after.x,
            "facing edge must keep DEFAULT_GAP from the card"
        );
        assert_eq!(
            app.overlay_origin.map(|p| (p.x as i32, p.y as i32)),
            Some((pet.x, pet.y)),
            "home origin must be the previewed pet spot"
        );
        assert_eq!(app.config.window.x, Some(pet.x));
        assert_eq!(app.config.window.y, Some(pet.y));
    }

    #[test]
    fn close_after_scale_commit_keeps_new_slot_on_dock_hwnd() {
        let repo = crate::config::ConfigRepository::default_paths().expect("config paths");
        let saver = crate::config::DebouncedSaver::new(repo);
        let config = crate::config::AppConfig::default();
        let mut app = App::new(PathBuf::from("."), config, saver);
        let old_scale = app.config.pet.scale;
        let old_pet = pet_logical_size(old_scale) as f32;
        seed_embedded_settings(&mut app, old_scale);
        let lay = app.menu_layout.as_ref().unwrap();
        app.dock_hwnd = Some(DockHwnd {
            pos: (40, 50),
            phys: (500, 480),
            win_log_w: lay.window_w,
            win_log_h: lay.window_h,
            pet_x: lay.pet_x,
            pet_y: lay.pet_y,
            pet_w: old_pet,
            pet_h: old_pet,
        });
        app.pet_scale_draft = Some(0.9);

        app.handle_settings_hit(SettingsHit::Done);
        app.finish_exit_menu_ui(Instant::now());

        let expected = pet_logical_size(0.9) as f32;
        let dock = app.dock_hwnd.expect("dock hwnd must stay after close");
        assert!(
            dock.phys.0 >= 500 && dock.phys.1 >= 480,
            "must not shrink the layered HWND, got {:?}",
            dock.phys
        );
        assert!(
            (dock.pet_w - expected).abs() < 0.01 && (dock.pet_h - expected).abs() < 0.01,
            "idle slot must be the committed size, got {}x{}",
            dock.pet_w,
            dock.pet_h
        );
        assert!(
            dock.pet_x >= -0.01,
            "idle slot must not hang off the left of the dock canvas"
        );
        assert!(
            dock.pet_x + dock.pet_w <= dock.win_log_w as f32 + 0.01,
            "idle slot must not hang off the right of the dock canvas"
        );
        assert!((app.config.pet.scale - 0.9).abs() < 0.001);
        assert!(
            (dock.pet_w - old_pet).abs() > 0.01,
            "slot must have left the pre-settings size"
        );
    }

    #[test]
    fn scale_steps_do_not_move_card_or_present() {
        let repo = crate::config::ConfigRepository::default_paths().expect("config paths");
        let saver = crate::config::DebouncedSaver::new(repo);
        let config = crate::config::AppConfig::default();
        let mut app = App::new(PathBuf::from("."), config, saver);
        let start_scale = app.config.pet.scale;
        seed_embedded_settings(&mut app, start_scale);
        let card0 = card_screen_of(&app);
        let pos0 = app.menu_present_pos;
        let card_local0 = {
            let l = app.menu_layout.as_ref().unwrap();
            (l.card_x, l.card_y, l.card_w, l.card_h)
        };
        let win0 = {
            let l = app.menu_layout.as_ref().unwrap();
            (l.window_w, l.window_h)
        };

        app.preview_pet_scale(1);
        app.preview_pet_scale(1);
        app.preview_pet_scale(-1);

        assert_eq!(app.menu_present_pos, pos0, "present origin must stay frozen");
        assert_eq!(card_screen_of(&app), card0, "card screen must stay frozen");
        let l = app.menu_layout.as_ref().unwrap();
        assert_eq!(
            (l.card_x, l.card_y, l.card_w, l.card_h),
            card_local0,
            "card local must stay frozen across ±"
        );
        assert_eq!(
            (l.window_w, l.window_h),
            win0,
            "overlay canvas size must stay frozen (cropped card = window grew)"
        );
        let pet = pet_screen_of(&app);
        assert_eq!(
            pet.x + pet.width + DEFAULT_GAP,
            card0.x,
            "facing gap stays DEFAULT_GAP"
        );
    }

    #[test]
    fn pad_then_preview_max_keeps_pet_inside_canvas() {
        let repo = crate::config::ConfigRepository::default_paths().expect("config paths");
        let saver = crate::config::DebouncedSaver::new(repo);
        let config = crate::config::AppConfig::default();
        let mut app = App::new(PathBuf::from("."), config, saver);
        let start_scale = app.config.pet.scale;
        seed_embedded_settings(&mut app, start_scale);
        let card0 = card_screen_of(&app);
        let pet0 = pet_screen_of(&app);

        assert!(
            app.ensure_overlay_fits_max_pet(),
            "60% seed must reserve room for 100%"
        );
        assert_eq!(card_screen_of(&app), card0, "pad must not move the card");
        assert_eq!(
            pet_screen_of(&app),
            pet0,
            "pad must not move the current pet"
        );

        // 0.6 → 1.0 is four + steps.
        for _ in 0..4 {
            app.preview_pet_scale(1);
        }
        assert!((app.effective_pet_scale() - 1.0).abs() < 0.001);

        let lay = app.menu_layout.as_ref().unwrap();
        assert!(
            lay.pet_x >= -0.01,
            "previewed pet must not clip on the left, pet_x={}",
            lay.pet_x
        );
        assert!(
            lay.pet_y >= -0.01,
            "previewed pet must not clip on the top, pet_y={}",
            lay.pet_y
        );
        assert!(
            lay.pet_x + lay.pet_w <= lay.window_w as f32 + 0.01,
            "previewed pet must not clip on the right"
        );
        assert!(
            (lay.pet_x + lay.pet_w + DEFAULT_GAP as f32 - lay.card_x).abs() < 0.6,
            "facing gap stays DEFAULT_GAP after pad+preview, pet={}x{} card={}",
            lay.pet_x,
            lay.pet_w,
            lay.card_x
        );
        assert_eq!(card_screen_of(&app), card0, "± after pad must not move the card");
    }

    #[test]
    fn done_after_pad_keeps_idle_slot_inside_dock() {
        let repo = crate::config::ConfigRepository::default_paths().expect("config paths");
        let saver = crate::config::DebouncedSaver::new(repo);
        let config = crate::config::AppConfig::default();
        let mut app = App::new(PathBuf::from("."), config, saver);
        let start_scale = app.config.pet.scale;
        seed_embedded_settings(&mut app, start_scale);
        assert!(app.ensure_overlay_fits_max_pet());
        let lay = app.menu_layout.as_ref().unwrap();
        app.dock_hwnd = Some(DockHwnd {
            pos: app.menu_present_pos.unwrap(),
            phys: (lay.window_w, lay.window_h),
            win_log_w: lay.window_w,
            win_log_h: lay.window_h,
            pet_x: lay.pet_x,
            pet_y: lay.pet_y,
            pet_w: lay.pet_w,
            pet_h: lay.pet_h,
        });
        for _ in 0..4 {
            app.preview_pet_scale(1);
        }

        app.handle_settings_hit(SettingsHit::Done);
        app.finish_exit_menu_ui(Instant::now());

        let expected = pet_logical_size(1.0) as f32;
        let dock = app.dock_hwnd.expect("dock hwnd must stay after close");
        assert!(
            (dock.pet_w - expected).abs() < 0.01 && (dock.pet_h - expected).abs() < 0.01
        );
        assert!(dock.pet_x >= -0.01);
        assert!(dock.pet_y >= -0.01);
        assert!(dock.pet_x + dock.pet_w <= dock.win_log_w as f32 + 0.01);
        assert!(dock.pet_y + dock.pet_h <= dock.win_log_h as f32 + 0.01);
    }

    #[test]
    fn contain_slot_grows_canvas_when_pet_is_off_left() {
        let repo = crate::config::ConfigRepository::default_paths().expect("config paths");
        let saver = crate::config::DebouncedSaver::new(repo);
        let config = crate::config::AppConfig::default();
        let mut app = App::new(PathBuf::from("."), config, saver);
        let start_scale = app.config.pet.scale;
        seed_embedded_settings(&mut app, start_scale);
        {
            let lay = app.menu_layout.as_mut().unwrap();
            lay.pet_x = -20.0;
            lay.pet_w = 128.0;
            lay.pet_h = 128.0;
        }
        let card0 = card_screen_of(&app);
        assert!(app.grow_overlay_to_contain_pet_slot());
        let lay = app.menu_layout.as_ref().unwrap();
        assert!(lay.pet_x >= -0.01);
        assert!(lay.pet_x + lay.pet_w <= lay.window_w as f32 + 0.01);
        assert_eq!(card_screen_of(&app), card0, "contain-grow must not move the card");
    }

    #[test]
    fn esc_discards_preview_and_restores_snapshot() {
        let repo = crate::config::ConfigRepository::default_paths().expect("config paths");
        let saver = crate::config::DebouncedSaver::new(repo);
        let config = crate::config::AppConfig::default();
        let mut app = App::new(PathBuf::from("."), config, saver);
        let old_scale = app.config.pet.scale;
        seed_embedded_settings(&mut app, old_scale);
        let card_before = card_screen_of(&app);
        let pet_before = pet_screen_of(&app);
        let pos_before = app.menu_present_pos;
        app.pet_scale_draft = Some(1.2);
        app.sync_pet_slot_to_scale(1.2);
        assert_ne!(
            pet_screen_of(&app).width,
            pet_before.width,
            "preview must move the slot"
        );

        app.begin_settings_pop(Instant::now());

        assert!(app.pet_scale_draft.is_none());
        assert!((app.config.pet.scale - old_scale).abs() < 0.001);
        assert_eq!(app.menu_present_pos, pos_before);
        assert_eq!(pet_screen_of(&app), pet_before);
        assert_eq!(card_screen_of(&app), card_before);
    }

    #[test]
    fn restore_with_dock_hwnd_keeps_size() {
        let repo = crate::config::ConfigRepository::default_paths().expect("config paths");
        let saver = crate::config::DebouncedSaver::new(repo);
        let config = crate::config::AppConfig::default();
        let mut app = App::new(PathBuf::from("."), config, saver);
        app.scale_factor = 1.0;
        app.dock_hwnd = Some(DockHwnd {
            pos: (40, 50),
            phys: (500, 480),
            win_log_w: 500,
            win_log_h: 480,
            pet_x: 8.0,
            pet_y: 80.0,
            pet_w: 128.0,
            pet_h: 128.0,
        });
        app.overlay_origin = Some(Point::new(100.0, 200.0));
        app.restore_overlay_origin_window();
        let dock = app.dock_hwnd.expect("dock hwnd must stay");
        assert_eq!(dock.phys, (500, 480), "must not shrink the layered HWND");
        assert_eq!(
            dock.pos,
            (100 - 8, 200 - 80),
            "window origin re-anchors so the pet slot sits on overlay_origin"
        );
        assert!(
            app.idle_present_pos.is_none(),
            "must not lock a 128×128 idle present (that would grow again on open)"
        );
        assert!(app.overlay_origin.is_none());
    }

    #[test]
    fn restore_locks_idle_present_at_origin() {
        let repo = crate::config::ConfigRepository::default_paths().expect("config paths");
        let saver = crate::config::DebouncedSaver::new(repo);
        let config = crate::config::AppConfig::default();
        let mut app = App::new(PathBuf::from("."), config, saver);
        app.overlay_origin = Some(Point::new(100.0, 200.0));
        app.config.pet.scale = 0.9;
        app.scale_factor = 1.0;
        app.restore_overlay_origin_window();
        assert_eq!(
            app.idle_present_pos,
            Some((100, 200)),
            "idle ULW must lock to the pre-overlay desk spot"
        );
        assert!(app.overlay_origin.is_none(), "origin is consumed on restore");
        let expected = pet_logical_size(0.9);
        assert_eq!(
            app.idle_target_phys(),
            (expected, expected),
            "lock size is the committed pet window"
        );
    }

    #[test]
    fn pause_click_refuses_without_toggling() {
        let repo = crate::config::ConfigRepository::default_paths().expect("config paths");
        let saver = crate::config::DebouncedSaver::new(repo);
        let config = crate::config::AppConfig::default();
        let mut app = App::new(PathBuf::from("."), config, saver);
        assert!(!app.config.reminder.paused);
        app.handle_settings_hit(SettingsHit::TogglePause);
        assert!(
            !app.config.reminder.paused,
            "pause button must not persist a paused state"
        );
        assert_eq!(
            app.menu_say.map(|(_, s)| s),
            Some(SAY_NO_PAUSE),
            "cat must refuse out loud"
        );
    }

    #[test]
    fn startup_clears_leftover_paused() {
        let repo = crate::config::ConfigRepository::default_paths().expect("config paths");
        let saver = crate::config::DebouncedSaver::new(repo);
        let mut config = crate::config::AppConfig::default();
        config.reminder.paused = true;
        let app = App::new(PathBuf::from("."), config, saver);
        assert!(
            !app.config.reminder.paused,
            "stale paused configs must not keep reminders silent"
        );
    }

    #[test]
    fn scale_around_anchor_matches_dimensions_and_fade() {
        let src = vec![
            0u8, 0, 0, 255,
            255, 0, 0, 255,
            0, 255, 0, 255,
            0, 0, 255, 255,
        ];
        let (out, dw, dh) = scale_rgba_around_anchor(&src, 2, 2, 0.5, 0.5);
        assert_eq!(dw, 1);
        assert_eq!(dh, 1);
        assert!(out[3] > 0 && out[3] <= 128);
        let (full, dw2, dh2) = scale_rgba_around_anchor(&src, 2, 2, 1.0, 1.0);
        assert_eq!((dw2, dh2), (2, 2));
        assert_eq!(full, src);
    }
}
