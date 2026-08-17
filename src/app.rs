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

use crate::config::{AppConfig, ConfigRepository, DebouncedSaver};
use crate::error::AppError;
use crate::event::{AppEvent, Point, TrayCommand};
use crate::pet::{
    pet_logical_size, AnimationLibrary, PetController, PetState, ReminderStage, REMINDER_WINDOW_H,
    REMINDER_WINDOW_W,
};
use crate::platform;
use crate::reminder::{now_rfc3339, pick_message, ReminderScheduler};
use crate::render::easing::ease_in_out_cubic;
use crate::render::menu_ui::{
    blit_rgba, blit_rgba_clipped, compose_menu_card_layer, compose_menu_frame,
    compose_menu_pet_only, compose_settings_card, compose_settings_frame, hit_settings,
    hit_settings_card, menu_visual_fade, menu_visual_scale, present_menu_cached,
    settings_card_metrics, settings_card_visible_rows, MenuChromeState, SettingsHit, SAY_FAIL,
    SETTINGS_H, SETTINGS_W,
};
use crate::render::sample_rgba_bilinear;
use crate::render::reminder_ui::{
    client_to_layout, compose_reminder_card_frame, compose_reminder_frame, food_button_layout,
    load_feed_bowl, load_reminder_card, FeedBowl, ReminderCard,
};
use crate::render::yawn_bubble::{compose_yawn_frame, place_yawn_bubble, YawnPlacement};
// Present path uses CPU + UpdateLayeredWindow only (no wgpu surface on the pet HWND).
// Attaching a DXGI/Vulkan swapchain to a WS_EX_LAYERED window breaks per-pixel alpha.
use crate::shortcut::{
    build_pick_context, extract_icon, launch, pick_executable, IconRgba, ShortcutItem,
    ShortcutRepository,
};
use crate::ui::launcher_place::{
    logical_to_physical, physical_to_logical, physical_to_logical_u32, place_launcher, snap_dpr,
    DEFAULT_GAP, DEFAULT_MARGIN,
};
use crate::ui::pet_window::DragState;
use crate::ui::radial_menu::{
    self, build_entries, clamp_list_scroll, count_shortcuts, hit_center, hit_test_index,
    layout_pinned_scroll, MenuEntry, RadialLayout, CARD_LOGICAL_H, CARD_LOGICAL_W, MENU_WINDOW_H,
    MENU_WINDOW_W,
};
use crate::ui::tray::TrayHandle;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum UserEvent {
    App(AppEvent),
    /// Async file-dialog result (never block UI thread with rfd).
    FilePicked(Option<PathBuf>),
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
    /// True while expanded reminder window is showing UI.
    reminder_ui_active: bool,
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
    /// Expanded settings list UI.
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
    /// Rest-state card layer (physical, no pet) for open/close scale+fade.
    menu_card_cache: Option<(u32, u32, Vec<u8>)>,
    /// Shortcut list scroll (first visible row index). Supports many apps via wheel.
    menu_list_scroll: usize,
    /// Rare speech (launch failure). Success closes immediately.
    menu_say: Option<(Instant, &'static str)>,
    /// Settings list row to emphasize (from invalid launcher item).
    settings_highlight_row: Option<usize>,
    /// Settings grows from the launcher's Manage button instead of snapping center.
    settings_transition: Option<SettingsTransition>,
    /// Overlay window top-left (physical) for atomic layered present during settings transition.
    settings_present_pos: Option<(i32, i32)>,
    /// Settings lives inside the dock card (launcher button), not a new window.
    settings_embed: bool,
    /// Card-sized settings snapshot for the slide.
    settings_card_cache: Option<(u32, u32, Vec<u8>)>,
    /// First visible row in the embedded settings list.
    settings_list_scroll: usize,
    /// Expanded comic-bubble window while `idle_yawn` plays.
    yawn_ui_active: bool,
    yawn_place: Option<YawnPlacement>,
    yawn_present_pos: Option<(i32, i32)>,
    /// Pet position before menu/settings expand (window top-left, physical).
    overlay_origin: Option<Point>,
    /// Current pet frame RGBA for alpha hit-testing (normal pet size).
    hit_rgba: Vec<u8>,
    hit_size: (u32, u32),
    _bg_rx: Option<std::sync::mpsc::Receiver<AppEvent>>,
    /// Proxy for background threads → UI (file picker, etc.).
    event_proxy: Option<EventLoopProxy<UserEvent>>,
    /// True while native file dialog is open on a worker thread.
    file_picker_busy: bool,
}

impl App {
    pub fn new(assets_dir: PathBuf, config: AppConfig, saver: DebouncedSaver) -> Self {
        let (_bg_tx, bg_rx) = std::sync::mpsc::channel();
        drop(_bg_tx);
        let now = Instant::now();
        let interval = ReminderScheduler::resolve_interval(config.reminder.interval_minutes);
        let mut scheduler = ReminderScheduler::new(
            config.reminder.enabled,
            config.reminder.paused,
            interval,
            now,
        );
        scheduler.apply_startup_catchup(config.reminder.last_completed_at.as_deref(), now);
        let shortcuts = ShortcutRepository::from_items(config.shortcuts.clone());
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
            menu_card_cache: None,
            menu_list_scroll: 0,
            menu_say: None,
            settings_highlight_row: None,
            settings_transition: None,
            settings_present_pos: None,
            settings_embed: false,
            settings_card_cache: None,
            settings_list_scroll: 0,
            yawn_ui_active: false,
            yawn_place: None,
            yawn_present_pos: None,
            overlay_origin: None,
            hit_rgba: Vec::new(),
            hit_size: (128, 128),
            _bg_rx: Some(bg_rx),
            event_proxy: None,
            file_picker_busy: false,
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
            let rows: Vec<(String, bool, bool)> = self
                .shortcuts
                .list_sorted()
                .into_iter()
                .map(|s| {
                    let valid = s.is_path_valid();
                    (s.name, s.enabled, valid)
                })
                .collect();
            let dpr = self.scale_factor.clamp(1.0, 3.0) as f32;
            let reminder = (
                self.config.reminder.enabled,
                self.config.reminder.interval_minutes,
                self.config.reminder.paused,
            );
            let (sw, sh, settings_rgba) = compose_settings_frame(
                &rows,
                reminder,
                self.config.pet.scale,
                dpr,
                self.settings_highlight_row,
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
            if pet.is_menu_animating() {
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
            };
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

        if self.reminder_ui_active
            && matches!(
                pet.state,
                PetState::Reminder(ReminderStage::Showing)
                    | PetState::Reminder(ReminderStage::Feeding)
            )
        {
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
        } else {
            let win = window.inner_size();
            let win_w = win.width.max(1);
            let win_h = win.height.max(1);
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
                platform::update_layered_rgba_ex(window.as_ref(), win_w, win_h, &present, None)
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
        let Some(window) = self.window.as_ref() else {
            return;
        };
        // Don't persist temporary overlay positions as home config.
        if self.reminder_ui_active
            || self.menu_ui_active
            || self.settings_ui_active
            || self.yawn_ui_active
        {
            return;
        }
        if let Ok(pos) = window.outer_position() {
            self.config.window.x = Some(pos.x);
            self.config.window.y = Some(pos.y);
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

    fn enter_reminder_ui(&mut self) {
        let center =
            self.work_area_center_top_left(REMINDER_WINDOW_W as i32, REMINDER_WINDOW_H as i32);
        self.reminder_ui_active = true;
        self.resize_pet_window(REMINDER_WINDOW_W, REMINDER_WINDOW_H);
        if let Some(w) = &self.window {
            w.set_outer_position(PhysicalPosition::new(center.x as i32, center.y as i32));
            // Force non-transparent click capture for whole reminder surface.
            let _ = platform::set_click_through(w.as_ref(), false);
            self.click_through = false;
        }
        if let Some(pet) = self.pet.as_mut() {
            let (x, y, bw, bh) = food_button_layout();
            pet.food_button_rect = Some((x, y, bw, bh));
        }
        self.texture_dirty = true;
        if let Some(w) = &self.window {
            w.request_redraw();
        }
        info!("reminder UI entered (food button active)");
    }

    fn exit_reminder_ui_to_pet_size(&mut self, top_left: Point) {
        self.reminder_ui_active = false;
        if let Some(pet) = self.pet.as_mut() {
            pet.food_button_rect = None;
        }
        let s = self.pet_size();
        self.resize_pet_window(s, s);
        if let Some(w) = &self.window {
            w.set_outer_position(PhysicalPosition::new(top_left.x as i32, top_left.y as i32));
        }
        self.texture_dirty = true;
    }

    fn enter_yawn_ui(&mut self) {
        if self.menu_ui_active || self.settings_ui_active || self.reminder_ui_active {
            return;
        }
        if self.yawn_ui_active {
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
        if let Some(w) = &self.window {
            if let Ok(pos) = w.outer_position() {
                self.overlay_origin = Some(Point::new(pos.x as f64, pos.y as f64));
            }
        }
    }

    fn restore_overlay_origin_window(&mut self) {
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
        let s = self.pet_size();
        self.resize_pet_window(s, s);
        if let Some(w) = &self.window {
            w.set_outer_position(PhysicalPosition::new(origin.x as i32, origin.y as i32));
        }
        self.texture_dirty = true;
    }

    fn enter_menu_ui(&mut self, now: Instant) {
        if self.reminder_ui_active || self.settings_ui_active {
            return;
        }

        // L2: leave edge-hide before capture so pin uses fully-visible home.
        if let Some(pet) = self.pet.as_mut() {
            if let Some(home) = pet.snap_restore_from_edge(now) {
                if let Some(w) = &self.window {
                    w.set_outer_position(PhysicalPosition::new(home.x as i32, home.y as i32));
                }
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
        // Atomic layered present target (physical). Avoids empty frames during resize.
        self.menu_present_pos = Some((place.window.x, place.window.y));

        // Keep winit geometry in sync for hit-testing / outer_position queries.
        self.resize_pet_window(win_log_w, win_log_h);
        if let Some(w) = &self.window {
            w.set_outer_position(PhysicalPosition::new(place.window.x, place.window.y));
            let _ = platform::set_click_through(w.as_ref(), false);
            self.click_through = false;
        }

        // Pet-only first present (card cache is still empty) so the cat never
        // flashes while the rest card is rasterized once.
        self.menu_card_cache = None;
        self.texture_dirty = true;
        self.redraw();
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
        self.settings_ui_active = false;
        self.settings_embed = false;
        self.settings_transition = None;
        self.settings_card_cache = None;
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
            let total = self.shortcuts.list_sorted().len();
            let vis = self
                .menu_layout
                .as_ref()
                .map(|l| {
                    settings_card_visible_rows(&settings_card_metrics(l.card_w, l.card_h))
                })
                .unwrap_or(3);
            let max = total.saturating_sub(vis);
            let next = (self.settings_list_scroll as i32 - lines).clamp(0, max as i32) as usize;
            if next != self.settings_list_scroll {
                self.settings_list_scroll = next;
                self.settings_card_cache = None;
                self.texture_dirty = true;
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
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
        self.enter_settings_ui_highlight(None);
    }

    /// Settings opened from a launcher button: slide in from the card's right.
    fn begin_settings_from_launcher(
        &mut self,
        _anchor: (i32, i32),
        highlight_row: Option<usize>,
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
            self.enter_settings_ui_highlight(highlight_row);
            return;
        }
        self.settings_highlight_row = highlight_row;
        self.settings_list_scroll = 0;
        if let Some(row) = highlight_row {
            if let Some(layout) = self.menu_layout.as_ref() {
                let m = settings_card_metrics(layout.card_w, layout.card_h);
                let vis = settings_card_visible_rows(&m);
                self.settings_list_scroll = row.saturating_sub(vis.saturating_sub(1));
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
        let rows: Vec<(String, bool, bool)> = self
            .shortcuts
            .list_sorted()
            .into_iter()
            .map(|s| {
                let valid = s.is_path_valid();
                (s.name, s.enabled, valid)
            })
            .collect();
        let dpr = self.scale_factor.clamp(1.0, 3.0) as f32;
        let reminder = (
            self.config.reminder.enabled,
            self.config.reminder.interval_minutes,
            self.config.reminder.paused,
        );
        let (sw, sh, buf) = compose_settings_card(
            &rows,
            reminder,
            self.config.pet.scale,
            dpr,
            self.settings_highlight_row,
            layout.card_w,
            layout.card_h,
            self.settings_list_scroll,
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
            let rows: Vec<(String, bool, bool)> = self
                .shortcuts
                .list_sorted()
                .into_iter()
                .map(|s| {
                let valid = s.is_path_valid();
                (s.name, s.enabled, valid)
            })
                .collect();
            let reminder = (
                self.config.reminder.enabled,
                self.config.reminder.interval_minutes,
                self.config.reminder.paused,
            );
            let (sw, sh, set) = compose_settings_card(
                &rows,
                reminder,
                self.config.pet.scale,
                dpr,
                self.settings_highlight_row,
                layout.card_w,
                layout.card_h,
                self.settings_list_scroll,
            );
            blit_rgba(&mut out, ww, hh, &set, sw, sh, cx.max(0) as u32, cy.max(0) as u32);
        }

        self.sprite_logical = self.menu_logical_size;
        self.hit_rgba = out;
        self.hit_size = (ww, hh);
        self.texture_dirty = false;
    }

    /// Settings opened without a launcher anchor (tray): centered, no transition.
    fn enter_settings_ui_highlight(&mut self, highlight_row: Option<usize>) {
        if self.reminder_ui_active {
            return;
        }
        self.settings_transition = None;
        self.settings_present_pos = None;
        self.settings_embed = false;
        self.settings_card_cache = None;
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
        self.settings_highlight_row = highlight_row;
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
        self.settings_highlight_row = None;
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
            self.settings_highlight_row = None;
            self.settings_list_scroll = 0;
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

    /// Commit a row only if the pointer is still on it (tap: down highlight, up commit).
    fn commit_menu_press(&mut self, now: Instant) {
        let Some(idx) = self.menu_press.take() else {
            return;
        };
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
        if let Some(path) = path {
            let order = self.shortcuts.items().len() as u32;
            self.shortcuts.add(ShortcutItem::from_path(&path, order));
            self.shortcut_icons.clear();
            self.persist_shortcuts();
            self.texture_dirty = true;
            info!(path = %path.display(), "shortcut added (async picker)");
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
                    Some(a) => self.begin_settings_from_launcher(a, None, now),
                    None => self.enter_settings_ui(),
                }
            }
            MenuEntry::Shortcut { id, valid, name, .. }
            | MenuEntry::Recent { id, valid, name, .. } => {
                if !valid {
                    warn!(%name, "shortcut path invalid — open manager to fix");
                    let row = self
                        .shortcuts
                        .list_sorted()
                        .iter()
                        .position(|s| s.id == id);
                    let anchor = self.menu_anchor_screen_for(item_idx);
                    match anchor {
                        Some(a) => self.begin_settings_from_launcher(a, row, now),
                        None => self.enter_settings_ui_highlight(row),
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
        match hit {
            SettingsHit::Close => {
                self.settings_card_cache = None;
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
                if let Some(s) = self.scheduler.as_mut() {
                    let paused = s.toggle_paused(now);
                    self.config.reminder.paused = paused;
                    self.persist_reminder_config();
                    self.sync_tray_tooltip();
                    self.texture_dirty = true;
                    info!(paused, "settings: reminder pause toggled");
                }
            }
            SettingsHit::PetScaleDec => {
                self.nudge_pet_scale(-1);
            }
            SettingsHit::PetScaleInc => {
                self.nudge_pet_scale(1);
            }
            SettingsHit::Add => {
                self.begin_pick_executable();
            }
            SettingsHit::RowToggle(i) => {
                if let Some(item) = self.shortcuts.list_sorted().get(i).cloned() {
                    self.shortcuts.set_enabled(item.id, !item.enabled);
                    self.shortcut_icons.clear();
                    self.persist_shortcuts();
                    self.texture_dirty = true;
                }
            }
            SettingsHit::RowUp(i) => {
                if let Some(item) = self.shortcuts.list_sorted().get(i).cloned() {
                    self.shortcuts.move_up(item.id);
                    self.persist_shortcuts();
                    self.texture_dirty = true;
                }
            }
            SettingsHit::RowDown(i) => {
                if let Some(item) = self.shortcuts.list_sorted().get(i).cloned() {
                    self.shortcuts.move_down(item.id);
                    self.persist_shortcuts();
                    self.texture_dirty = true;
                }
            }
            SettingsHit::RowDelete(i) => {
                if let Some(item) = self.shortcuts.list_sorted().get(i).cloned() {
                    self.shortcuts.remove(item.id);
                    self.shortcut_icons.clear();
                    self.persist_shortcuts();
                    self.texture_dirty = true;
                }
            }
        }
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
        } else if self.config.reminder.paused {
            "PawDesk — 提醒已暂停"
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

        let window_pos = self
            .window
            .as_ref()
            .and_then(|w| w.outer_position().ok())
            .map(|p| Point::new(p.x as f64, p.y as f64))
            .unwrap_or(Point::new(100.0, 100.0));

        let s = self.pet_size() as i32;
        let center = self.work_area_center_top_left(s, s);
        let msg = pick_message(&self.config.reminder.custom_messages);

        if let Some(pet) = self.pet.as_mut() {
            if pet.begin_reminder(window_pos, center, msg, now) {
                if let Some(origin) = pet.reminder_origin {
                    if (origin.x - window_pos.x).abs() > 0.5
                        || (origin.y - window_pos.y).abs() > 0.5
                    {
                        if let Some(w) = &self.window {
                            w.set_outer_position(PhysicalPosition::new(
                                origin.x as i32,
                                origin.y as i32,
                            ));
                        }
                    }
                }
                if let Some(s) = self.scheduler.as_mut() {
                    s.consume_due();
                }
                self.texture_dirty = true;
            }
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
            TrayCommand::ToggleReminderPause => {
                let now = Instant::now();
                if let Some(s) = self.scheduler.as_mut() {
                    let paused = s.toggle_paused(now);
                    self.config.reminder.paused = paused;
                    self.persist_reminder_config();
                    self.sync_tray_tooltip();
                    info!(paused, "tray: reminder pause toggled");
                }
            }
            TrayCommand::OpenSettings => {
                info!("tray: OpenSettings");
                if !self.visible {
                    if let Some(w) = &self.window {
                        w.set_visible(true);
                        self.visible = true;
                    }
                }
                self.enter_settings_ui();
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
            let s = self.pet_size();
            self.resize_pet_window(s, s);
            self.texture_dirty = true;
            if let Some(w) = &self.window {
                w.request_redraw();
            }
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
        if self.menu_ui_active || self.settings_transition.is_some() {
            return Duration::from_millis(16);
        }
        if self.settings_ui_active
            || self.reminder_ui_active
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
                }
                // Threshold drag start.
                if self.menu_press.is_none() && self.drag.consider_drag_start() {
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
                        self.settings_highlight_row = None;
                        self.menu_present_pos = None;
                        self.menu_card_cache = None;
                        self.menu_list_scroll = 0;
                        // Keep overlay_origin for restore after drag end.
                    }
                    if self.reminder_ui_active {
                        let pos = self
                            .window
                            .as_ref()
                            .and_then(|w| w.outer_position().ok())
                            .map(|p| Point::new(p.x as f64, p.y as f64))
                            .unwrap_or(Point::new(100.0, 100.0));
                        self.exit_reminder_ui_to_pet_size(pos);
                    } else if self.settings_ui_active {
                        self.settings_ui_active = false;
                        self.settings_transition = None;
                        self.settings_present_pos = None;
                        self.settings_highlight_row = None;
                        let s = self.pet_size();
                        self.resize_pet_window(s, s);
                    } else if self.overlay_origin.is_some() && !self.menu_ui_active {
                        let s = self.pet_size();
                        self.resize_pet_window(s, s);
                        if let Some(o) = self.overlay_origin {
                            if let Some(w) = &self.window {
                                w.set_outer_position(PhysicalPosition::new(o.x as i32, o.y as i32));
                            }
                        }
                    }
                    self.texture_dirty = true;
                }
                if let Some(w) = self.window.as_ref() {
                    self.drag.apply_drag(w);
                    if self.drag.dragging {
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
                                let n = self.shortcuts.list_sorted().len();
                                if let Some(hit) = hit_settings_card(
                                    lx - layout.card_x,
                                    ly - layout.card_y,
                                    layout.card_w,
                                    layout.card_h,
                                    n,
                                    self.settings_list_scroll,
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
                            let n = self.shortcuts.items().len();
                            if let Some(hit) = hit_settings(lx as f32, ly as f32, n) {
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
                            if matches!(pet.state, PetState::Reminder(ReminderStage::Showing)) {
                                let (client_w, client_h) = self
                                    .window
                                    .as_ref()
                                    .map(|w| {
                                        let s = w.inner_size();
                                        (s.width as f64, s.height as f64)
                                    })
                                    .unwrap_or((
                                        REMINDER_WINDOW_W as f64,
                                        REMINDER_WINDOW_H as f64,
                                    ));
                                let (lx, ly) = client_to_layout(
                                    self.cursor_in_window.x,
                                    self.cursor_in_window.y,
                                    client_w,
                                    client_h,
                                );
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
                    if self.menu_ui_active && self.menu_press.is_some() {
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
                            if self.yawn_ui_active {
                                self.exit_yawn_ui();
                            }
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
            if let Some((t0, _)) = self.menu_say {
                let elapsed = now.saturating_duration_since(t0);
                if elapsed >= Duration::from_millis(1400) {
                    self.menu_say = None;
                    self.texture_dirty = true;
                    need_redraw = true;
                } else {
                    need_redraw = true;
                }
            }
            if self.menu_ui_active {
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
            if movement_was_active {
                if let Some(pet) = self.pet.as_mut() {
                    if let Some(mut new_pos) = pet.update_movement(now) {
                        if pet.is_reminder_moving() {
                            if let Some(wa) = self
                                .window
                                .as_ref()
                                .and_then(|w| platform::work_area_for_window(w.as_ref()).ok())
                            {
                                new_pos.y = new_pos.y.max(wa.y as f64);
                            }
                        }
                        if let Some(w) = &self.window {
                            w.set_outer_position(PhysicalPosition::new(
                                new_pos.x as i32,
                                new_pos.y as i32,
                            ));
                        }
                        need_redraw = true;
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
            if entered_showing {
                self.enter_reminder_ui();
            }
            if returned_idle {
                self.reminder_ui_active = false;
                self.persist_window_pos();
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
                        let window_top_left = Point::new(pos.x as f64, pos.y as f64);
                        let pet_center = Point::new(
                            pos.x as f64 + size.width as f64 / 2.0,
                            pos.y as f64 + size.height as f64 / 2.0,
                        );
                        if let Some(pet) = self.pet.as_mut() {
                            if pet.update_interaction(
                                cursor,
                                pet_center,
                                window_top_left,
                                size.width as f64,
                                size.height as f64,
                                now,
                            ) {
                                self.texture_dirty = true;
                                need_redraw = true;
                            }
                            if self.config.pet.edge_hide_enabled {
                                if let Some(wa) = work_area {
                                    let pet_rect = platform::Rect {
                                        x: pos.x,
                                        y: pos.y,
                                        width: size.width as i32,
                                        height: size.height as i32,
                                    };
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

            // Feed animation done → shrink + return + persist.
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
                let ps = self.pet_size() as i32;
                let center_small = self.work_area_center_top_left(ps, ps);
                self.exit_reminder_ui_to_pet_size(center_small);
                if let Some(pet) = self.pet.as_mut() {
                    pet.start_reminder_return(center_small, now);
                    self.texture_dirty = true;
                }
                need_redraw = true;
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
        if self.menu_ui_active && self.settings_transition.is_none() {
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
