//! Application lifecycle (M0–M4).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tracing::{debug, error, info, warn};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, MouseButton as WinitMouseButton, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowId, WindowLevel};

use crate::config::{AppConfig, ConfigRepository, DebouncedSaver};
use crate::error::AppError;
use crate::event::{AppEvent, Point, TrayCommand};
use crate::pet::{
    AnimationLibrary, PetController, PetState, ReminderStage, PET_WINDOW_SIZE, REMINDER_WINDOW_H,
    REMINDER_WINDOW_W,
};
use crate::platform;
use crate::reminder::{now_rfc3339, pick_message, ReminderScheduler};
use crate::render::menu_ui::{
    compose_menu_frame, compose_settings_frame, hit_settings, MenuChromeState, SettingsHit,
    SETTINGS_H, SETTINGS_W,
};
use crate::render::reminder_ui::{client_to_layout, compose_reminder_frame, food_button_layout};
// Present path uses CPU + UpdateLayeredWindow only (no wgpu surface on the pet HWND).
// Attaching a DXGI/Vulkan swapchain to a WS_EX_LAYERED window breaks per-pixel alpha.
use crate::shortcut::{launch, pick_executable, ShortcutItem, ShortcutRepository};
use crate::ui::launcher_place::{
    logical_to_physical, physical_to_logical, physical_to_logical_u32, place_launcher, snap_dpr,
    DEFAULT_GAP, DEFAULT_MARGIN,
};
use crate::ui::pet_window::DragState;
use crate::ui::radial_menu::{
    self, build_entries, hit_center, hit_test_index, layout_pinned, MenuEntry, RadialLayout,
    CARD_LOGICAL_H, CARD_LOGICAL_W, MENU_WINDOW_H, MENU_WINDOW_W,
};
use crate::ui::tray::TrayHandle;

#[derive(Debug, Clone)]
pub enum UserEvent {
    App(AppEvent),
    /// Async file-dialog result (never block UI thread with rfd).
    FilePicked(Option<PathBuf>),
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
    /// Feed completed this session cycle; persist once when return starts.
    feed_persist_pending: bool,
    /// M4 shortcuts.
    shortcuts: ShortcutRepository,
    /// Expanded radial menu UI.
    menu_ui_active: bool,
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
    /// Settings list row to emphasize (from invalid launcher item).
    settings_highlight_row: Option<usize>,
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
            feed_persist_pending: false,
            shortcuts,
            menu_ui_active: false,
            menu_reopen_after: None,
            settings_ui_active: false,
            menu_layout: None,
            menu_logical_size: (MENU_WINDOW_W, MENU_WINDOW_H),
            menu_hover: None,
            menu_press: None,
            settings_highlight_row: None,
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

    fn create_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), AppError> {
        let logical = platform::PET_WINDOW_LOGICAL_SIZE;
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
        info!(scale = self.scale_factor, "window DPI scale factor");

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
            scale = self.scale_factor,
            anim = %clip.name,
            "pet window + layered present + tray ready (M4)"
        );

        self.hit_rgba = first_frame;
        self.hit_size = (clip.frame_width, clip.frame_height);
        self.window = Some(window);
        self.tray = Some(tray);
        self.pet = Some(pet);
        self.last_frame = Instant::now();
        self.texture_dirty = true;

        // First paint: silhouette via UpdateLayeredWindow.
        self.redraw();
        Ok(())
    }

    fn sync_texture_from_pet(&mut self) {
        let Some(pet) = self.pet.as_ref() else {
            return;
        };

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
            let (w, h, composed) =
                compose_settings_frame(&rows, reminder, dpr, self.settings_highlight_row);
            // Logical size for hit-mapping; physical pixels in hit_rgba.
            self.sprite_logical = (SETTINGS_W, SETTINGS_H);
            self.hit_rgba = composed;
            self.hit_size = (w, h);
            self.texture_dirty = false;
            return;
        }

        // Keep composing while menu is open or mid-close fade (still MenuOpen until anim ends).
        if self.menu_ui_active && pet.is_menu_open() {
            let clip = pet.active_clip();
            let pet_rgba = pet.display_rgba();
            let paused = self
                .scheduler
                .as_ref()
                .map(|s| s.is_paused())
                .unwrap_or(false);
            let entries = build_entries(self.shortcuts.list_enabled_sorted().as_slice(), paused);
            let (lw, lh) = self.menu_logical_size;
            // L3-02: geometry locked at open (open_t=1.0 for layout); visual uses menu_open_t.
            let mut layout = self
                .menu_layout
                .as_ref()
                .map(|prev| {
                    layout_pinned(
                        &entries,
                        lw,
                        lh,
                        (prev.pet_x, prev.pet_y, prev.pet_w, prev.pet_h),
                        (prev.card_x, prev.card_y, prev.card_w, prev.card_h),
                        radial_menu::ExpandDir::Right,
                        1.0,
                    )
                })
                .unwrap_or_else(|| {
                    layout_pinned(
                        &entries,
                        lw,
                        lh,
                        (0.0, 0.0, 128.0, 128.0),
                        (128.0, 0.0, CARD_LOGICAL_W as f32, CARD_LOGICAL_H as f32),
                        radial_menu::ExpandDir::Right,
                        1.0,
                    )
                });
            layout.open_t = pet.menu_open_t;
            let dpr = self.scale_factor.clamp(1.0, 3.0) as f32;
            let chrome = MenuChromeState {
                hover: self.menu_hover,
                press: self.menu_press,
            };
            let (w, h, composed) = compose_menu_frame(
                &pet_rgba,
                clip.frame_width,
                clip.frame_height,
                &layout,
                paused,
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
            let clip = pet.active_clip();
            let pet_rgba = pet.display_rgba();
            let pulse = {
                let t = Instant::now().elapsed().as_secs_f32();
                1.0 + 0.05 * (t * std::f32::consts::TAU / 1.2).sin()
            };
            let feeding = matches!(pet.state, PetState::Reminder(ReminderStage::Feeding));
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
        let win = window.inner_size();
        let win_w = win.width.max(1);
        let win_h = win.height.max(1);

        if self.hit_rgba.is_empty() {
            return;
        }

        // Scale content to physical window for layered present.
        let (sw, sh) = self.hit_size;
        let overlay =
            self.menu_ui_active || self.settings_ui_active || self.reminder_ui_active;
        let drag_scale = if overlay {
            1.0
        } else {
            self.pet.as_ref().map(|p| p.drag_scale).unwrap_or(1.0)
        };

        let mirror_x = self
            .pet
            .as_ref()
            .map(|p| {
                matches!(p.state, crate::pet::PetState::Approaching { .. }) && p.face_dir < 0.0
            })
            .unwrap_or(false);

        // HiDPI overlays are already composed at device pixels — present 1:1.
        // Bilinear upscale is what made text/outlines look soft.
        let present = if overlay && sw == win_w && sh == win_h && !mirror_x {
            self.hit_rgba.clone()
        } else if overlay && !mirror_x {
            // Prefer nearest-ish path when only a few px off (window snap).
            scale_rgba_centered_crisp(&self.hit_rgba, sw, sh, win_w, win_h)
        } else {
            scale_rgba_centered(
                &self.hit_rgba,
                sw,
                sh,
                win_w,
                win_h,
                drag_scale,
                mirror_x,
            )
        };

        if let Err(e) = platform::update_layered_rgba(window.as_ref(), win_w, win_h, &present) {
            error!("update_layered_rgba: {e}");
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
        if self.reminder_ui_active || self.menu_ui_active || self.settings_ui_active {
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
        self.resize_pet_window(PET_WINDOW_SIZE, PET_WINDOW_SIZE);
        if let Some(w) = &self.window {
            w.set_outer_position(PhysicalPosition::new(top_left.x as i32, top_left.y as i32));
        }
        self.texture_dirty = true;
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
        self.settings_ui_active = false;
        self.menu_layout = None;
        self.resize_pet_window(PET_WINDOW_SIZE, PET_WINDOW_SIZE);
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
        let pet_phys = logical_to_physical(PET_WINDOW_SIZE, dpr);
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

        let paused = self
            .scheduler
            .as_ref()
            .map(|s| s.is_paused())
            .unwrap_or(false);
        let entries = build_entries(self.shortcuts.list_enabled_sorted().as_slice(), paused);
        self.menu_layout = Some(layout_pinned(
            &entries,
            win_log_w,
            win_log_h,
            pet_local,
            card_local,
            place.dir,
            0.0,
        ));

        self.menu_ui_active = true;
        self.menu_hover = None;
        self.menu_press = None;
        self.resize_pet_window(win_log_w, win_log_h);
        if let Some(w) = &self.window {
            w.set_outer_position(PhysicalPosition::new(place.window.x, place.window.y));
            let _ = platform::set_click_through(w.as_ref(), false);
            self.click_through = false;
            w.request_redraw();
        }
        self.texture_dirty = true;
        info!(
            ?place.dir,
            win = ?(place.window.x, place.window.y, place.window.width, place.window.height),
            delta = ?place.pet_screen_delta,
            "launcher dock entered (pin-pet)"
        );
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
        self.restore_overlay_origin_window();
        // L3-05: ignore click that closed the dock + brief double-tap reopen.
        self.menu_reopen_after = Some(now + Duration::from_millis(280));
        info!("launcher dock exited");
    }

    fn enter_settings_ui(&mut self) {
        self.enter_settings_ui_highlight(None);
    }

    fn enter_settings_ui_highlight(&mut self, highlight_row: Option<usize>) {
        if self.reminder_ui_active {
            return;
        }
        // Close menu into settings without losing origin.
        if self.menu_ui_active {
            if let Some(pet) = self.pet.as_mut() {
                let now = Instant::now();
                pet.close_menu(now);
            }
            self.menu_ui_active = false;
            self.menu_layout = None;
            self.menu_hover = None;
            self.menu_press = None;
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
        self.settings_ui_active = false;
        self.settings_highlight_row = None;
        self.restore_overlay_origin_window();
        info!("settings UI exited");
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

    fn update_menu_hover(&mut self) {
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

        // Always-on-top layered window steals focus / fights COM dialog → lag.
        if let Some(w) = &self.window {
            w.set_window_level(WindowLevel::Normal);
        }

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
                    let path = pick_executable().ok().flatten();
                    CoUninitialize();
                    if proxy.send_event(UserEvent::FilePicked(path)).is_err() {
                        // Event loop gone
                    }
                }
                #[cfg(not(windows))]
                {
                    let path = pick_executable().ok().flatten();
                    let _ = proxy.send_event(UserEvent::FilePicked(path));
                }
            });
    }

    fn on_file_picked(&mut self, path: Option<PathBuf>) {
        self.file_picker_busy = false;
        // Restore pet above desktop icons.
        if let Some(w) = &self.window {
            w.set_window_level(WindowLevel::AlwaysOnTop);
            w.request_redraw();
        }
        if let Some(path) = path {
            let order = self.shortcuts.items().len() as u32;
            self.shortcuts.add(ShortcutItem::from_path(&path, order));
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

    fn handle_menu_entry(&mut self, entry: MenuEntry, now: Instant) {
        match entry {
            MenuEntry::AddShortcut => {
                self.begin_pick_executable();
            }
            MenuEntry::Manage => {
                self.enter_settings_ui();
            }
            MenuEntry::PauseReminder => {
                if let Some(s) = self.scheduler.as_mut() {
                    let paused = s.toggle_paused(now);
                    self.config.reminder.paused = paused;
                    if let Some(saver) = self.saver.as_mut() {
                        saver.mark_dirty();
                    }
                    info!(paused, "menu: reminder pause toggled");
                }
                self.texture_dirty = true;
            }
            MenuEntry::Shortcut { id, valid, name } => {
                if !valid {
                    warn!(%name, "shortcut path invalid — open manager to fix");
                    let row = self
                        .shortcuts
                        .list_sorted()
                        .iter()
                        .position(|s| s.id == id);
                    self.enter_settings_ui_highlight(row);
                    return;
                }
                if let Some(item) = self.shortcuts.get(id).cloned() {
                    match launch(&item) {
                        Ok(()) => {
                            self.exit_menu_ui(now);
                        }
                        Err(e) => warn!(error = %e, "launch failed"),
                    }
                }
            }
        }
    }

    fn handle_settings_hit(&mut self, hit: SettingsHit) {
        let now = Instant::now();
        match hit {
            SettingsHit::Close => self.exit_settings_ui(),
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
            SettingsHit::Add => {
                self.begin_pick_executable();
            }
            SettingsHit::RowToggle(i) => {
                if let Some(item) = self.shortcuts.list_sorted().get(i).cloned() {
                    self.shortcuts.set_enabled(item.id, !item.enabled);
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

        let center = self.work_area_center_top_left(PET_WINDOW_SIZE as i32, PET_WINDOW_SIZE as i32);
        let msg = pick_message(&self.config.reminder.custom_messages);

        if let Some(pet) = self.pet.as_mut() {
            if pet.begin_reminder(window_pos, center, msg, now) {
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
                }
                self.clamp_window_to_work_area();
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
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
        }
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
        if self.menu_ui_active
            || self.settings_ui_active
            || self.reminder_ui_active
            || self.drag.dragging
            || self.file_picker_busy
        {
            return Duration::from_millis(33);
        }
        let animating = self.pet.as_ref().map(|p| {
            p.movement.is_active()
                || p.is_playing_cute_action()
                || matches!(
                    p.state,
                    PetState::Approaching { .. }
                        | PetState::PlayingInteraction(_)
                        | PetState::Reminder(_)
                        | PetState::MenuOpen
                )
        }).unwrap_or(false);
        if animating {
            Duration::from_millis(33)
        } else {
            // RND-07: calmer idle cadence.
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
            WindowEvent::RedrawRequested => {
                self.redraw();
            }
            WindowEvent::Resized(_size) => {
                // Layered present uses current inner_size each frame; no swapchain.
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_in_window = Point::new(position.x, position.y);
                let now = Instant::now();
                if self.menu_ui_active {
                    self.update_menu_hover();
                }
                // Threshold drag start.
                if self.drag.consider_drag_start() {
                    if let Some(pet) = self.pet.as_mut() {
                        if self.menu_ui_active {
                            pet.close_menu(now);
                            self.menu_ui_active = false;
                            self.menu_layout = None;
                            // Keep overlay_origin for restore after drag end.
                        }
                        pet.begin_drag(now);
                    }
                    if self.menu_ui_active || self.settings_ui_active {
                        // Don't expand while dragging.
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
                        self.resize_pet_window(PET_WINDOW_SIZE, PET_WINDOW_SIZE);
                    } else if self.overlay_origin.is_some() && !self.menu_ui_active {
                        self.resize_pet_window(PET_WINDOW_SIZE, PET_WINDOW_SIZE);
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
                    // Settings hits
                    if self.settings_ui_active {
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

                    // Menu hits (map client → layout logical); ignore while closing
                    if self.menu_ui_active {
                        let interactive = self
                            .pet
                            .as_ref()
                            .map(|p| p.is_menu_interactive())
                            .unwrap_or(false);
                        if !interactive {
                            return;
                        }
                        if let Some((lx, ly)) = self.menu_cursor_logical() {
                            if let Some(layout) = self.menu_layout.clone() {
                                if let Some(idx) = hit_test_index(&layout, lx, ly) {
                                    self.menu_press = Some(idx);
                                    self.texture_dirty = true;
                                    if let Some(entry) = layout.items.get(idx) {
                                        self.handle_menu_entry(entry.entry.clone(), now);
                                    }
                                    self.menu_press = None;
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
                                // Empty card / transparent union padding → close
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
                                    || ((100.0..=260.0).contains(&lx)
                                        && (190.0..=250.0).contains(&ly));
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

            // Menu open/close animation (L3)
            if let Some(pet) = self.pet.as_mut() {
                let (animating, close_done) = pet.tick_menu_anim(now);
                if close_done {
                    self.finish_exit_menu_ui(now);
                    need_redraw = true;
                } else if animating || pet.is_menu_open() {
                    need_redraw = true;
                    self.texture_dirty = true;
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
                    if let Some(new_pos) = pet.update_movement(now) {
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

            // Interaction + edge (not while dragging / reminder / menu / settings).
            if !self.drag.dragging {
                let in_reminder = self
                    .pet
                    .as_ref()
                    .map(|p| p.state.is_reminder())
                    .unwrap_or(false);
                let overlay = self.menu_ui_active || self.settings_ui_active;
                if !in_reminder && !overlay {
                    let cursor_pt = platform::cursor_pos()
                        .ok()
                        .map(|(x, y)| Point::new(x as f64, y as f64));
                    let window_info = self.window.as_ref().and_then(|w| {
                        w.outer_position().ok().map(|pos| {
                            let size = w.outer_size();
                            (pos, size)
                        })
                    });
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
                // Idle dense loop always advances — keep presenting at 30fps.
                if pet.state.is_idle() || matches!(pet.state, PetState::Approaching { .. }) {
                    need_redraw = true;
                    self.texture_dirty = true;
                }
                if pet.tick_interaction(now) {
                    pet.begin_returning(now);
                    self.texture_dirty = true;
                    need_redraw = true;
                }
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
                let center_small =
                    self.work_area_center_top_left(PET_WINDOW_SIZE as i32, PET_WINDOW_SIZE as i32);
                self.exit_reminder_ui_to_pet_size(center_small);
                if let Some(pet) = self.pet.as_mut() {
                    pet.start_reminder_return(center_small, now);
                    self.texture_dirty = true;
                }
                need_redraw = true;
            }

            if need_redraw || self.texture_dirty {
                if let Some(w) = &self.window {
                    if self.visible {
                        w.request_redraw();
                    }
                }
            }

            self.handle_app_event(event_loop, AppEvent::Tick(now));
        }

        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + self.frame_interval(),
        ));
    }
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

/// Scale `src` (sw×sh RGBA) into a transparent `dw×dh` canvas, centered, with optional uniform scale.
///
/// Upscales to fill the destination (needed on high-DPI: 128 logical → 256 physical).
/// Uses bilinear sampling so cartoon edges stay smooth instead of blocky nearest-neighbor.
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
    // Leave a tiny safe margin so soft edges are not clipped by the window.
    let margin = 2u32.min(fw / 16).min(fh / 16);
    let fw = fw.saturating_sub(margin * 2).max(1);
    let fh = fh.saturating_sub(margin * 2).max(1);
    let ox = ((dw.saturating_sub(fw)) / 2) as i32;
    let oy = ((dh.saturating_sub(fh)) / 2) as i32;

    let sw_f = sw as f64;
    let sh_f = sh as f64;
    let fw_f = fw as f64;
    let fh_f = fh as f64;

    for dy in 0..fh {
        for dx in 0..fw {
            let src_dx = if mirror_x { fw - 1 - dx } else { dx };
            // Map dest pixel center into continuous source coords.
            let sx = (src_dx as f64 + 0.5) * sw_f / fw_f - 0.5;
            let sy = (dy as f64 + 0.5) * sh_f / fh_f - 0.5;
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

/// Bilinear sample of tightly-packed RGBA8. Transparent outside bounds.
fn sample_rgba_bilinear(src: &[u8], w: u32, h: u32, x: f64, y: f64) -> [u8; 4] {
    if w == 0 || h == 0 {
        return [0, 0, 0, 0];
    }
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let x1 = x0 + 1;
    let y1 = y0 + 1;
    let fx = (x - x0 as f64).clamp(0.0, 1.0);
    let fy = (y - y0 as f64).clamp(0.0, 1.0);

    let p = |ix: i32, iy: i32| -> [f64; 4] {
        if ix < 0 || iy < 0 || ix >= w as i32 || iy >= h as i32 {
            return [0.0, 0.0, 0.0, 0.0];
        }
        let i = ((iy as u32 * w + ix as u32) * 4) as usize;
        [
            src[i] as f64,
            src[i + 1] as f64,
            src[i + 2] as f64,
            src[i + 3] as f64,
        ]
    };

    // Premultiply for correct alpha blend of edge pixels.
    let fetch = |ix: i32, iy: i32| -> [f64; 4] {
        let c = p(ix, iy);
        let a = c[3] / 255.0;
        [c[0] * a, c[1] * a, c[2] * a, c[3]]
    };

    let c00 = fetch(x0, y0);
    let c10 = fetch(x1, y0);
    let c01 = fetch(x0, y1);
    let c11 = fetch(x1, y1);

    let mut out = [0.0f64; 4];
    for i in 0..4 {
        let top = c00[i] * (1.0 - fx) + c10[i] * fx;
        let bot = c01[i] * (1.0 - fx) + c11[i] * fx;
        out[i] = top * (1.0 - fy) + bot * fy;
    }

    let a = out[3].clamp(0.0, 255.0);
    if a < 0.5 {
        return [0, 0, 0, 0];
    }
    let inv = 255.0 / a;
    [
        (out[0] * inv).clamp(0.0, 255.0).round() as u8,
        (out[1] * inv).clamp(0.0, 255.0).round() as u8,
        (out[2] * inv).clamp(0.0, 255.0).round() as u8,
        a.round() as u8,
    ]
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
