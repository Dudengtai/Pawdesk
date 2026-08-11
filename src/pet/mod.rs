//! Pet domain: state, animation, interaction, movement, reminder (M2+M3).

mod animation;
mod interaction;
mod movement;
mod state;

pub use animation::{
    AnimationClip, AnimationLibrary, AnimationPlayer, IdlePicker, IDLE_BASE,
    IDLE_ACTION_INTERVAL_SECS, blend_rgba_premul,
};
pub use interaction::{DistanceLevel, InteractionDetector};
pub use movement::{
    approach_duration, return_duration, MovementController, MovementTarget, EDGE_DURATION,
    REMINDER_MOVE_DURATION,
};
pub use state::{can_interrupt, try_transition, Edge, PetState, ReminderStage};

use std::time::{Duration, Instant};

use tracing::{debug, info, warn};

use crate::event::Point;
use crate::platform::Rect;

const INTERACTION_DURATION: Duration = Duration::from_millis(1500);
const FEED_DURATION: Duration = Duration::from_millis(900);
/// Soft blend when switching clips (UI / non-body). Pet body oneshots rely on
/// exact sit bookends; the one-shot return uses this as a short residual guard,
/// since pose-blending still ghosts open mouths/noses.
const CLIP_CROSSFADE_SECS: f32 = 0.10;
/// Hold the oneshot's last frame (sit bookend) briefly so exit feels settled.
/// With exact base_sit last frame this is invisible against idle_blink/000.
const ACTION_SETTLE_SECS: f32 = 0.20;
/// One-shot cute actions play at least this long (readable on desktop).
/// Video stretch is ~2.4s @30fps — do not time-stretch those denser clips.
const ACTION_MIN_SECS: f32 = 2.2;
/// How fast `face_dir` eases toward the cursor facing target (higher = snappier).
const FACE_DIR_SPEED: f32 = 12.0;

/// Mouse pounce (`Approaching` → cursor) is deferred to a later polish pass.
/// When `false`, near-range only keeps `Watching` (no leap / fly-to-cursor).
pub const ENABLE_MOUSE_POUNCE: bool = false;

/// Reminder UI layout in logical pixels (96 DPI baseline).
pub const REMINDER_WINDOW_W: u32 = 360;
pub const REMINDER_WINDOW_H: u32 = 260;
/// Design baseline pet window (logical px @ 96 DPI). Actual size = baseline × `pet.scale`.
pub const PET_WINDOW_SIZE: u32 = 128;
pub const FOOD_BUTTON_SIZE: f32 = 64.0;

/// Logical pet window edge length from config scale (clamped).
pub fn pet_logical_size(scale: f32) -> u32 {
    let s = scale.clamp(0.5, 2.0);
    ((PET_WINDOW_SIZE as f32) * s).round().clamp(64.0, 256.0) as u32
}

/// High-level pet controller used by the app loop.
pub struct PetController {
    pub state: PetState,
    pub library: AnimationLibrary,
    pub player: AnimationPlayer,
    pub picker: IdlePicker,
    pub current_frame: u32,
    pub drag_scale: f32,
    pub interaction: InteractionDetector,
    pub movement: MovementController,
    pub home_position: Option<Point>,
    pub pre_edge_position: Option<Point>,
    pub hidden_position: Option<Point>,
    pub interaction_started: Option<Instant>,
    pub interaction_duration: Duration,
    /// Earliest time another mouse approach may start (anti-spam after return).
    pub next_approach_at: Option<Instant>,
    // ── M3 reminder ──
    pub pending_reminder: bool,
    pub reminder_origin: Option<Point>,
    pub reminder_message: String,
    pub feed_started: Option<Instant>,
    /// Food button rect in window client coords (logical px), set while Showing.
    pub food_button_rect: Option<(f32, f32, f32, f32)>,
    // ── M4 menu ──
    /// Visual progress 0→1 open, 1→0 while closing (task §14 L3).
    pub menu_open_t: f32,
    pub menu_anim_started: Option<Instant>,
    /// True while playing close animation (still `MenuOpen` until done).
    pub menu_closing: bool,
    /// `menu_open_t` at the moment close started (for reverse lerp).
    menu_close_from_t: f32,
    /// Horizontal facing while approaching / watching: -1 = face left, 1 = face right.
    /// Smoothed toward [`Self::face_dir_target`] each tick for soft flips.
    pub face_dir: f32,
    /// Desired facing from cursor / approach direction (-1 or 1).
    face_dir_target: f32,
    /// Continuous frame index for smooth 30fps sampling (`frame_rgba_smooth`).
    pub display_frame_f: f32,
    /// Previous clip pixels while crossfading into the new clip.
    crossfade_from: Option<Vec<u8>>,
    crossfade_started: Option<Instant>,
    /// 0 = full previous frame, 1 = full current clip.
    crossfade_t: f32,
    /// Whether the captured crossfade is visible (one-shot return only).
    crossfade_display: bool,
}

impl PetController {
    pub fn new(library: AnimationLibrary, now: Instant) -> Self {
        let actions = library.idle_action_names();
        if actions.is_empty() {
            warn!("no idle one-shot actions loaded — cute timer will never fire");
        } else {
            info!(
                count = actions.len(),
                actions = %actions.join(","),
                enabled = %crate::pet::animation::IDLE_ACTION_ENABLED.join(","),
                interval_s = IDLE_ACTION_INTERVAL_SECS,
                "idle cute action pool ready"
            );
        }
        let mut picker = IdlePicker::new(actions, now);
        let initial = picker.pick_initial(now);
        let clip = library
            .get(&initial)
            .or_else(|| library.get(IDLE_BASE))
            .unwrap_or_else(|| library.first());
        let player = AnimationPlayer::start(clip, now);
        info!(
            anim = %clip.name,
            interval_s = IDLE_ACTION_INTERVAL_SECS,
            "pet idle base started (sit+blink)"
        );
        Self {
            state: PetState::Idle(clip.name.clone()),
            library,
            player,
            picker,
            current_frame: 0,
            drag_scale: 1.0,
            interaction: InteractionDetector::default(),
            movement: MovementController::default(),
            home_position: None,
            pre_edge_position: None,
            hidden_position: None,
            interaction_started: None,
            interaction_duration: INTERACTION_DURATION,
            next_approach_at: None,
            pending_reminder: false,
            reminder_origin: None,
            reminder_message: String::new(),
            feed_started: None,
            food_button_rect: None,
            menu_open_t: 0.0,
            menu_anim_started: None,
            menu_closing: false,
            menu_close_from_t: 1.0,
            face_dir: 1.0,
            face_dir_target: 1.0,
            display_frame_f: 0.0,
            crossfade_from: None,
            crossfade_started: None,
            crossfade_t: 1.0,
            crossfade_display: false,
        }
    }

    /// RGBA for current display.
    ///
    /// Ordinary body clip switches keep crossfade disabled (blend of two face
    /// poses → mouth ghosting). One-shot returns enable a short captured-pose
    /// fade as a residual guard; frame sampling remains nearest-neighbor via
    /// [`AnimationClip::frame_rgba_smooth`].
    pub fn display_rgba(&self) -> Vec<u8> {
        let current = self.active_clip().frame_rgba_smooth(self.display_frame_f);
        if self.crossfade_display {
            if let Some(from) = &self.crossfade_from {
                if self.crossfade_t < 1.0 {
                    return blend_rgba_premul(from, &current, self.crossfade_t);
                }
            }
        }
        current
    }

    /// True while a clip crossfade is still visible (caller should keep presenting).
    pub fn is_crossfading(&self) -> bool {
        self.crossfade_from.is_some() && self.crossfade_t < 1.0
    }

    /// Capture current pixels and start a soft transition into the next clip.
    fn begin_crossfade(&mut self, now: Instant) {
        // Sample without recursive crossfade so we freeze the outgoing pose cleanly.
        let from = self.active_clip().frame_rgba_smooth(self.display_frame_f);
        self.crossfade_from = Some(from);
        self.crossfade_started = Some(now);
        self.crossfade_t = 0.0;
        // Hidden by default; only one-shot return opts in via go_idle_with_settle.
        self.crossfade_display = false;
    }

    fn tick_crossfade(&mut self, now: Instant) -> bool {
        let Some(start) = self.crossfade_started else {
            return false;
        };
        let u = (now.duration_since(start).as_secs_f32() / CLIP_CROSSFADE_SECS).clamp(0.0, 1.0);
        let changed = (u - self.crossfade_t).abs() > 0.0005;
        self.crossfade_t = u;
        if u >= 1.0 {
            self.crossfade_from = None;
            self.crossfade_started = None;
            self.crossfade_t = 1.0;
            self.crossfade_display = false;
        }
        changed
    }

    /// Snap facing (no slow lerp). Soft approach was unnecessary for binary mirror
    /// and made threshold flicker feel like a half-mirrored face.
    fn tick_face_dir(&mut self, now: Instant) -> bool {
        let _ = now;
        let prev = self.face_dir;
        self.face_dir = self.face_dir_target;
        (self.face_dir - prev).abs() > 0.001
    }

    fn set_face_target(&mut self, dir: f32) {
        self.face_dir_target = if dir < 0.0 { -1.0 } else { 1.0 };
    }

    pub fn active_clip(&self) -> &AnimationClip {
        self.library
            .get(self.player.clip_name())
            .unwrap_or_else(|| self.library.first())
    }

    pub fn begin_drag(&mut self, now: Instant) {
        if matches!(self.state, PetState::Dragging) {
            return;
        }
        // RM-07: reminder yields to drag, stay pending.
        if self.state.is_reminder() {
            self.defer_reminder_for_drag();
        }
        if matches!(self.state, PetState::MenuOpen) {
            self.menu_open_t = 0.0;
            self.menu_anim_started = None;
            self.menu_closing = false;
        }
        self.movement.cancel();
        if let Ok(s) = try_transition(&self.state, PetState::Dragging) {
            self.state = s;
            self.drag_scale = 1.05;
            self.food_button_rect = None;
            self.switch_clip_for_state(now);
            debug!(at = ?now, "pet begin drag");
        }
    }

    pub fn open_menu(&mut self, now: Instant) -> bool {
        if self.state.is_reminder() {
            return false;
        }
        // Dragging / approach / play: don't steal mid-motion into menu.
        if matches!(
            self.state,
            PetState::Dragging | PetState::Approaching { .. } | PetState::PlayingInteraction(_)
        ) {
            return false;
        }
        self.movement.cancel();
        self.interaction_started = None;
        self.interaction.reset_dwell();
        // Allow opening during cute one-shot (user intent wins).
        if matches!(self.state, PetState::Idle(_)) && !self.is_on_base_idle() {
            // Force legal Idle(base) → MenuOpen by going through base name first.
            if try_transition(&self.state, PetState::Idle(IDLE_BASE.to_string())).is_ok() {
                self.state = PetState::Idle(IDLE_BASE.to_string());
            }
        }
        if let Ok(s) = try_transition(&self.state, PetState::MenuOpen) {
            self.state = s;
            self.menu_open_t = 0.0;
            self.menu_closing = false;
            self.menu_close_from_t = 1.0;
            self.menu_anim_started = Some(now);
            self.switch_clip_for_state(now);
            info!("menu opened");
            true
        } else {
            false
        }
    }

    /// Instant close (settings handoff / drag). Prefer [`begin_close_menu`] for UI.
    pub fn close_menu(&mut self, now: Instant) {
        if !matches!(self.state, PetState::MenuOpen) {
            return;
        }
        self.menu_open_t = 0.0;
        self.menu_anim_started = None;
        self.menu_closing = false;
        self.interaction.reset_dwell();
        self.go_idle(now);
        info!("menu closed (immediate)");
    }

    /// Start close animation. Returns `true` if animating; `false` if already closed / snap.
    pub fn begin_close_menu(&mut self, now: Instant) -> bool {
        if !matches!(self.state, PetState::MenuOpen) {
            return false;
        }
        if self.menu_closing {
            return true;
        }
        // Nearly closed or still at start of open → snap.
        if self.menu_open_t < 0.08 {
            self.close_menu(now);
            return false;
        }
        self.menu_closing = true;
        self.menu_close_from_t = self.menu_open_t.clamp(0.0, 1.0);
        self.menu_anim_started = Some(now);
        info!(from = self.menu_close_from_t, "menu close anim start");
        true
    }

    /// Advance open/close animation.
    /// Returns `(needs_redraw, close_finished)` — when `close_finished`, state is Idle.
    pub fn tick_menu_anim(&mut self, now: Instant) -> (bool, bool) {
        if !matches!(self.state, PetState::MenuOpen) {
            return (false, false);
        }
        // Silk open: longer settle, no overshoot. Linear clock in pet; compose multi-curves.
        // Close slightly shorter with ease-in feel via compose.
        const OPEN_DUR: f32 = 0.38;
        const CLOSE_DUR: f32 = 0.24;

        if self.menu_closing {
            let Some(start) = self.menu_anim_started else {
                self.close_menu(now);
                return (true, true);
            };
            // Linear 1→0 clock; visual ease applied in compose for smooth reverse.
            let u = (now.duration_since(start).as_secs_f32() / CLOSE_DUR).clamp(0.0, 1.0);
            self.menu_open_t = self.menu_close_from_t * (1.0 - u);
            if u >= 1.0 {
                self.menu_open_t = 0.0;
                self.menu_anim_started = None;
                self.menu_closing = false;
                self.interaction.reset_dwell();
                self.go_idle(now);
                info!("menu close anim done");
                return (true, true);
            }
            return (true, false);
        }

        // Opening — store **linear** 0→1; compose applies ease_out_quint / fade lead.
        if let Some(start) = self.menu_anim_started {
            let u = (now.duration_since(start).as_secs_f32() / OPEN_DUR).clamp(0.0, 1.0);
            self.menu_open_t = u;
            if u >= 1.0 {
                self.menu_open_t = 1.0;
                self.menu_anim_started = None;
                return (false, false);
            }
            return (true, false);
        }
        self.menu_open_t = 1.0;
        (false, false)
    }

    pub fn is_menu_open(&self) -> bool {
        matches!(self.state, PetState::MenuOpen)
    }

    /// Menu visible and accepting clicks (not mid-close).
    pub fn is_menu_interactive(&self) -> bool {
        matches!(self.state, PetState::MenuOpen) && !self.menu_closing
    }

    pub fn end_drag(&mut self, now: Instant) {
        if !matches!(self.state, PetState::Dragging) {
            return;
        }
        self.go_idle(now);
        self.drag_scale = 1.0;
        debug!("pet end drag -> idle base");
    }

    pub fn tick(&mut self, now: Instant) -> bool {
        let mut changed = self.tick_crossfade(now);
        changed |= self.tick_face_dir(now);

        // Dragging: keep swing clip alive + subtle scale pulse (was frozen before).
        if matches!(self.state, PetState::Dragging) {
            let prev_f = self.display_frame_f;
            let (frame, _) = self.tick_continuous(now);
            let elapsed = now
                .duration_since(self.player.started_at_pub())
                .as_secs_f32();
            // Soft “held by scruff” bob — snappy but readable.
            self.drag_scale = 1.05 + 0.04 * (elapsed * 6.5).sin();
            if frame != self.current_frame || (self.display_frame_f - prev_f).abs() > 0.0005 {
                changed = true;
            }
            self.current_frame = frame;
            return changed;
        }

        let on_base = self.player.clip_name() == IDLE_BASE && self.state.is_idle();
        let on_action = self.state.is_idle()
            && self.player.clip_name() != IDLE_BASE
            && !self.player.is_looping();

        // Approaching: pose phase = hop progress (coherent pounce).
        // Dense clips (≥8 frames): continuous player / progress sampling at 30fps.
        // Tiny blink clips (≤4 frames): legacy hold-based blink.
        let (frame, finished) = if matches!(self.state, PetState::Approaching { .. })
            && self.movement.is_cursor_approach()
        {
            self.tick_pounce_synced(now)
        } else if on_base && self.player.frame_count_pub() <= 4 {
            self.tick_blink_hold(now)
        } else {
            self.tick_continuous(now)
        };

        if frame != self.current_frame
            || (self.display_frame_f - frame as f32).abs() > 0.001
        {
            changed = true;
        }
        self.current_frame = frame;

        // One-shot idle action finished (includes settle hold) → back to sit+blink.
        if on_action && finished {
            self.picker.mark_action_done(now);
            self.go_idle_with_settle(now);
            changed = true;
            info!("idle action finished -> base blink (settle)");
            return changed;
        }

        // Wall-clock cute actions: Idle(base) / Watching / HiddenAtEdge (peek must not starve timer).
        if self.try_start_scheduled_cute(now) {
            changed = true;
        }

        changed
    }

    /// Start a random one-shot cute action when the picker cooldown elapses.
    ///
    /// Eligible: calm idle base, Watching, or edge-peek (peek is restored first so the
    /// full stretch is visible). Heavy states (menu / drag / reminder / mid-action) skip.
    fn try_start_scheduled_cute(&mut self, now: Instant) -> bool {
        if self.is_playing_cute_action() {
            return false;
        }
        if matches!(
            self.state,
            PetState::Dragging
                | PetState::MenuOpen
                | PetState::Reminder(_)
                | PetState::Approaching { .. }
                | PetState::PlayingInteraction(_)
        ) {
            return false;
        }

        let eligible = self.is_on_base_idle()
            || matches!(self.state, PetState::Watching | PetState::HiddenAtEdge(_));
        if !eligible {
            return false;
        }
        if !self.picker.action_due(now) {
            return false;
        }

        // Peeking at edge: bring fully on-screen first so the action is readable.
        if matches!(self.state, PetState::HiddenAtEdge(_)) {
            if self.snap_restore_from_edge(now).is_none() {
                // Still edge-locked — don't burn the cooldown.
                warn!("cute action due but edge restore failed");
                return false;
            }
            info!("cute action due — restored from edge peek first");
        }

        let Some(action) = self.picker.take_action(now) else {
            return false;
        };
        if self.library.get(&action).is_none() {
            warn!(anim = %action, "cute action clip missing — will retry next interval");
            // Cooldown already consumed by take_action; keep going rather than tight-looping.
            return false;
        }

        // Watching / Idle(base) → Idle(action)
        match try_transition(&self.state, PetState::Idle(action.clone())) {
            Ok(s) => self.state = s,
            Err(_) => {
                // Force idle name if transition table rejects (should not happen for Watching/Idle).
                if self.state.is_idle() {
                    self.state = PetState::Idle(action.clone());
                } else {
                    warn!(
                        from = self.state.name(),
                        anim = %action,
                        "cute action transition rejected"
                    );
                    return false;
                }
            }
        }

        if let Some(clip) = self.library.get(&action) {
            info!(
                anim = %action,
                frames = clip.frame_count,
                fps = clip.fps,
                looping = clip.looping,
                next_in_s = self.picker.interval_secs(),
                "idle cute action started"
            );
            if clip.frame_count <= 1 {
                warn!(anim = %action, "cute clip has ≤1 frame — will look static");
            }
            if clip.looping {
                warn!(anim = %action, "cute clip is looping=true — oneshot finish path may never run");
            }
            // Seamless enter: oneshots begin on sit bookend (== idle_blink frame 0).
            // Do NOT skip bookend frames (that caused a hard pop into mid-pose).
            // Do NOT pose-crossfade (double mouth/nose ghosting).
            self.crossfade_from = None;
            self.crossfade_started = None;
            self.crossfade_t = 1.0;
            self.crossfade_display = false;
            self.player = AnimationPlayer::start(clip, now);
            self.current_frame = 0;
            self.display_frame_f = 0.0;
        } else {
            warn!(anim = %action, "cute action missing after check");
            return false;
        }
        true
    }

    /// `idle_blink` clip layout: [0]=open, [1]=half, [2]=closed (any extra frames ignored).
    /// Cycle ≈ 4.0s: mostly open, ~200ms blink. Returns `(frame, finished_cycle_unused)`.
    fn tick_blink_hold(&mut self, now: Instant) -> (u32, bool) {
        const CYCLE_SECS: f32 = 4.0;
        const BLINK_AT: f32 = 3.55;
        let n = self.player.frame_count_pub().max(1);
        let open = 0u32;
        let half = 1u32.min(n - 1);
        let closed = 2u32.min(n - 1);

        let elapsed = now
            .duration_since(self.player.started_at_pub())
            .as_secs_f32();
        let t = elapsed % CYCLE_SECS;
        let (frame, frac) = if t < BLINK_AT {
            (open, 0.0)
        } else if t < BLINK_AT + 0.07 {
            (open, (t - BLINK_AT) / 0.07)
        } else if t < BLINK_AT + 0.16 {
            (half, (t - BLINK_AT - 0.07) / 0.09)
        } else if t < BLINK_AT + 0.24 {
            (closed, (t - BLINK_AT - 0.16) / 0.08)
        } else {
            (open, 0.0)
        };
        // Map hold blink onto continuous index between open/half/closed.
        self.display_frame_f = match frame {
            f if f == open && t >= BLINK_AT && t < BLINK_AT + 0.07 => {
                open as f32 + frac * (half as f32 - open as f32)
            }
            f if f == half => half as f32 + frac * (closed as f32 - half as f32).max(0.0),
            f if f == closed => {
                closed as f32 + frac * (half as f32 - closed as f32)
            }
            _ => open as f32,
        };
        let _ = closed;
        (self.display_frame_f.floor() as u32, false)
    }

    /// Map hop progress `t∈[0,1]` onto continuous `approaching` frames (smooth 30fps).
    fn tick_pounce_synced(&mut self, now: Instant) -> (u32, bool) {
        let n = self.player.frame_count_pub().max(1);
        let t = self.movement.progress(now).unwrap_or(1.0);
        // Storyboard aligned with tools/build_coherent_30fps.py pounce phases.
        let phase = if t < 0.15 {
            (t / 0.15) * 0.15
        } else if t < 0.40 {
            0.15 + ((t - 0.15) / 0.25) * 0.25
        } else if t < 0.70 {
            0.40 + ((t - 0.40) / 0.30) * 0.30
        } else if t < 0.88 {
            0.70 + ((t - 0.70) / 0.18) * 0.18
        } else {
            0.88 + ((t - 0.88) / 0.12) * 0.12
        };
        let f = phase * ((n - 1) as f32);
        self.display_frame_f = f;
        let frame = f.floor() as u32;
        (frame.min(n - 1), false)
    }

    /// Time-based continuous sampling for dense clips (idle loop / one-shot actions).
    fn tick_continuous(&mut self, now: Instant) -> (u32, bool) {
        let n = self.player.frame_count_pub().max(1) as f32;
        let mut fps = self.active_clip().fps.max(1.0);
        // One-shot cute actions: if natural duration is short, slow slightly for readability.
        // Dense video clips (e.g. idle_stretch ~72f@30 ≈ 2.4s) play at authored fps.
        let oneshot_action = self.state.is_idle()
            && self.player.clip_name() != IDLE_BASE
            && !self.player.is_looping();
        if oneshot_action {
            let natural = n / fps;
            if natural < ACTION_MIN_SECS {
                fps = n / ACTION_MIN_SECS;
            }
        }
        let elapsed = now
            .duration_since(self.player.started_at_pub())
            .as_secs_f32();
        let frame_f = elapsed * fps;
        let finished = if self.player.is_looping() {
            false
        } else if oneshot_action {
            // Play through frames, then hold last pose briefly before settling to base.
            frame_f >= n + ACTION_SETTLE_SECS * fps
        } else {
            frame_f >= n
        };
        let f = if self.player.is_looping() {
            frame_f % n
        } else {
            frame_f.clamp(0.0, (n - 1.0).max(0.0))
        };
        self.display_frame_f = f;
        // Keep AnimationPlayer last_frame in sync for debug / finished edge cases.
        let (_pf, pfin) = self.player.tick(now);
        let _ = pfin;
        (f.floor() as u32, finished)
    }

    pub fn frame_uv(&self) -> [f32; 4] {
        self.active_clip().uv_for_frame(self.current_frame)
    }

    // ── Interaction (PET-06) ──

    /// Distance uses pet center; movement uses window top-left.
    ///
    /// **Pounce deferred**: near-range no longer starts `Approaching` (see
    /// [`ENABLE_MOUSE_POUNCE`]). Medium/near both use `Watching` for now.
    ///
    /// Interaction polish:
    /// - hysteresis + dwell before Watching (no threshold flicker)
    /// - never interrupt a one-shot cute action with Watching
    pub fn update_interaction(
        &mut self,
        cursor: Point,
        pet_center: Point,
        window_top_left: Point,
        win_w: f64,
        win_h: f64,
        now: Instant,
    ) -> bool {
        let _ = (window_top_left, win_w, win_h); // used when pounce re-enabled
        if matches!(
            self.state,
            PetState::Dragging
                | PetState::HiddenAtEdge(_)
                | PetState::PlayingInteraction(_)
                | PetState::Approaching { .. }
                | PetState::Reminder(_)
                | PetState::MenuOpen
        ) {
            self.interaction.reset_dwell();
            return false;
        }

        // One-shot cute action in progress: do not steal focus to Watching.
        if self.is_playing_cute_action() {
            self.interaction.reset_dwell();
            return false;
        }

        let level = self.interaction.compute_level_stable(pet_center, cursor);
        let prev = self.state.clone();
        let dwell_ok = self.interaction.watch_dwell_ready(level, now);

        // Facing is fixed (no cursor-driven horizontal flip).

        match level {
            DistanceLevel::Far => {
                if matches!(self.state, PetState::Watching) {
                    self.go_idle(now);
                }
            }
            DistanceLevel::Medium => {
                if dwell_ok {
                    self.enter_watching_if_base_idle(now);
                }
            }
            DistanceLevel::Near => {
                if ENABLE_MOUSE_POUNCE {
                    if self.is_on_base_idle() || matches!(self.state, PetState::Watching) {
                        if let Some(until) = self.next_approach_at {
                            if now < until {
                                return self.state != prev;
                            }
                        }
                        if can_interrupt(
                            &self.state,
                            &PetState::Approaching {
                                target: cursor,
                                started_at: now,
                            },
                        ) {
                            self.begin_approaching(cursor, window_top_left, win_w, win_h, now);
                        }
                    }
                } else if dwell_ok {
                    // Deferred: treat near like watch-only (no pounce).
                    self.enter_watching_if_base_idle(now);
                }
            }
        }

        self.state != prev
    }

    /// Sit+blink base only (not mid one-shot cute clip).
    pub fn is_on_base_idle(&self) -> bool {
        self.state.is_idle() && self.player.clip_name() == IDLE_BASE
    }

    /// Idle one-shot cute action (stretch/cute/sleep/…) still playing.
    pub fn is_playing_cute_action(&self) -> bool {
        self.state.is_idle()
            && self.player.clip_name() != IDLE_BASE
            && !self.player.is_looping()
    }

    fn enter_watching_if_base_idle(&mut self, now: Instant) {
        // Only interrupt base idle — never cut a cute one-shot short.
        if !self.is_on_base_idle() {
            return;
        }
        let proposed = PetState::Watching;
        if can_interrupt(&self.state, &proposed) {
            if let Ok(s) = try_transition(&self.state, proposed) {
                self.state = s;
                self.switch_clip_for_state(now);
            }
        }
    }

    fn begin_approaching(
        &mut self,
        cursor: Point,
        window_top_left: Point,
        win_w: f64,
        win_h: f64,
        now: Instant,
    ) {
        if !ENABLE_MOUSE_POUNCE {
            return;
        }
        // Store and move using window top-left (not center).
        self.home_position = Some(window_top_left);
        let dest = Point::new(cursor.x - win_w * 0.5, cursor.y - win_h * 0.5);
        let dist = InteractionDetector::compute_distance(window_top_left, dest);
        let dur = approach_duration(dist);
        if let Ok(s) = try_transition(
            &self.state,
            PetState::Approaching {
                target: dest,
                started_at: now,
            },
        ) {
            self.state = s;
            self.movement
                .start(window_top_left, MovementTarget::Cursor(dest), now, dur);
            self.switch_clip_for_state(now);
        }
    }

    pub fn begin_returning(&mut self, now: Instant) {
        let Some(home) = self.home_position else {
            self.go_idle(now);
            return;
        };
        let current = self.movement.last_position().unwrap_or(home);
        let dist = InteractionDetector::compute_distance(current, home);
        let dur = return_duration(dist);
        if let Ok(s) = try_transition(
            &self.state,
            PetState::Approaching {
                target: home,
                started_at: now,
            },
        ) {
            self.state = s;
            self.movement
                .start(current, MovementTarget::Home(home), now, dur);
            self.switch_clip_for_state(now);
        }
    }

    pub fn tick_interaction(&mut self, now: Instant) -> bool {
        if !matches!(self.state, PetState::PlayingInteraction(_)) {
            return false;
        }
        if let Some(started) = self.interaction_started {
            // Always use the fixed play duration for the interaction clip.
            if now.duration_since(started) >= INTERACTION_DURATION {
                self.interaction_started = None;
                return true;
            }
        }
        false
    }

    // ── Edge (PET-05) ──

    pub fn update_edge(&mut self, pet_rect: Rect, work_area: Rect, now: Instant) -> Option<Edge> {
        // Only auto-hide while calmly on base idle (not cute action / watch / menu).
        if !self.is_on_base_idle() {
            return None;
        }
        let edge = InteractionDetector::detect_edge(pet_rect, work_area)?;
        let current_pos = Point::new(pet_rect.x as f64, pet_rect.y as f64);
        let size = pet_rect.width.max(pet_rect.height).max(1) as u32;
        self.begin_edge_hide(edge, current_pos, size, now);
        Some(edge)
    }

    pub fn begin_edge_hide(
        &mut self,
        edge: Edge,
        current_pos: Point,
        window_size: u32,
        now: Instant,
    ) {
        self.pre_edge_position = Some(current_pos);
        let size = window_size.max(1);
        let hidden = InteractionDetector::compute_hidden_position(current_pos, edge, size);
        self.hidden_position = Some(hidden);
        if let Ok(s) = try_transition(&self.state, PetState::HiddenAtEdge(edge)) {
            self.state = s;
            self.movement.start(
                current_pos,
                MovementTarget::EdgeHide(hidden),
                now,
                EDGE_DURATION,
            );
            self.switch_clip_for_state(now);
            info!(?edge, "pet hiding at edge");
        }
    }

    pub fn restore_from_edge(&mut self, now: Instant) {
        let Some(home) = self.pre_edge_position else {
            return;
        };
        let current = self.hidden_position.unwrap_or(home);
        let name = IDLE_BASE.to_string();
        if let Ok(s) = try_transition(&self.state, PetState::Idle(name.clone())) {
            self.state = s;
            self.movement.start(
                current,
                MovementTarget::EdgeRestore(home),
                now,
                EDGE_DURATION,
            );
            self.pre_edge_position = None;
            self.hidden_position = None;
            self.picker.mark_base(now);
            if self.library.get(&name).is_some() {
                self.begin_crossfade(now);
                if let Some(clip) = self.library.get(&name) {
                    self.player = AnimationPlayer::start(clip, now);
                }
                self.current_frame = 0;
                self.display_frame_f = 0.0;
            }
        }
    }

    /// Instantly leave edge-hide (for launcher open). Returns home top-left if restored.
    pub fn snap_restore_from_edge(&mut self, now: Instant) -> Option<Point> {
        if !matches!(self.state, PetState::HiddenAtEdge(_)) {
            return None;
        }
        let home = self.pre_edge_position.or(self.hidden_position)?;
        self.movement.cancel();
        self.pre_edge_position = None;
        self.hidden_position = None;
        self.interaction.reset_dwell();
        let name = IDLE_BASE.to_string();
        if let Ok(s) = try_transition(&self.state, PetState::Idle(name.clone())) {
            self.state = s;
            self.picker.mark_base(now);
            // Instant for launcher placement — skip crossfade so dock places cleanly.
            self.crossfade_from = None;
            self.crossfade_started = None;
            self.crossfade_t = 1.0;
            if let Some(clip) = self.library.get(&name) {
                self.player = AnimationPlayer::start(clip, now);
                self.current_frame = 0;
                self.display_frame_f = 0.0;
            }
            info!("pet snap-restored from edge for launcher");
            Some(home)
        } else {
            None
        }
    }

    // ── Movement ──

    pub fn update_movement(&mut self, now: Instant) -> Option<Point> {
        if !self.movement.is_active() {
            return None;
        }
        self.movement.tick(now)
    }

    pub fn on_movement_complete(&mut self, now: Instant) -> bool {
        // Must take target after active=false; leave it cleared so we don't re-handle.
        let Some(target) = self.movement.take_target() else {
            return false;
        };

        match target {
            MovementTarget::Cursor(_) => {
                let anim = "playing_interaction".to_string();
                if let Ok(s) = try_transition(&self.state, PetState::PlayingInteraction(anim)) {
                    self.state = s;
                    self.interaction_started = Some(now);
                    self.switch_clip_for_state(now);
                    debug!("reached cursor -> playing interaction");
                    return true;
                }
            }
            MovementTarget::Home(_) | MovementTarget::EdgeRestore(_) => {
                self.go_idle(now);
                self.next_approach_at = Some(now + Duration::from_millis(600));
                return true;
            }
            MovementTarget::EdgeHide(_) => {
                return false;
            }
            MovementTarget::ReminderCenter(_) => {
                if let Ok(s) =
                    try_transition(&self.state, PetState::Reminder(ReminderStage::Showing))
                {
                    self.state = s;
                    self.layout_food_button();
                    self.switch_clip_for_state(now);
                    info!(msg = %self.reminder_message, "reminder showing");
                    return true;
                }
            }
            MovementTarget::ReminderHome(_) => {
                self.reminder_origin = None;
                self.food_button_rect = None;
                self.go_idle(now);
                return true;
            }
        }
        false
    }

    // ── Reminder (PET-09, RM-04/06/07) ──

    pub fn begin_reminder(
        &mut self,
        current_top_left: Point,
        center_top_left: Point,
        message: String,
        now: Instant,
    ) -> bool {
        // Cancel competing movement/interaction.
        self.movement.cancel();
        self.interaction_started = None;
        self.home_position = None;

        self.reminder_origin = Some(current_top_left);
        self.reminder_message = message;
        self.pending_reminder = false;
        self.feed_started = None;
        self.food_button_rect = None;

        if let Ok(s) = try_transition(
            &self.state,
            PetState::Reminder(ReminderStage::MovingToCenter),
        ) {
            self.state = s;
            self.movement.start(
                current_top_left,
                MovementTarget::ReminderCenter(center_top_left),
                now,
                REMINDER_MOVE_DURATION,
            );
            self.switch_clip_for_state(now);
            info!("reminder begin: moving to center");
            true
        } else {
            self.pending_reminder = true;
            warn!("could not enter reminder state; kept pending");
            false
        }
    }

    fn layout_food_button(&mut self) {
        // Bottom-center of reminder window.
        let w = REMINDER_WINDOW_W as f32;
        let h = REMINDER_WINDOW_H as f32;
        let s = FOOD_BUTTON_SIZE;
        let x = (w - s) * 0.5;
        let y = h - s - 24.0;
        self.food_button_rect = Some((x, y, s, s));
    }

    pub fn hit_food_button(&self, local_x: f64, local_y: f64) -> bool {
        let Some((x, y, w, h)) = self.food_button_rect else {
            return false;
        };
        // Pad hit box so the control is easy to click (includes "点击投喂" label).
        let pad = 12.0;
        local_x >= x as f64 - pad
            && local_y >= y as f64 - pad
            && local_x <= (x + w) as f64 + pad
            && local_y <= (y + h) as f64 + pad + 22.0
    }

    pub fn on_feed_click(&mut self, now: Instant) -> bool {
        if !matches!(self.state, PetState::Reminder(ReminderStage::Showing)) {
            return false;
        }
        if let Ok(s) = try_transition(&self.state, PetState::Reminder(ReminderStage::Feeding)) {
            self.state = s;
            self.feed_started = Some(now);
            self.food_button_rect = None;
            self.switch_clip_for_state(now);
            info!("feed clicked");
            true
        } else {
            false
        }
    }

    /// True when Feeding animation finished and App should shrink window + start return.
    pub fn feed_animation_done(&mut self, now: Instant) -> bool {
        if !matches!(self.state, PetState::Reminder(ReminderStage::Feeding)) {
            return false;
        }
        let Some(started) = self.feed_started else {
            return false;
        };
        if now.duration_since(started) < FEED_DURATION {
            return false;
        }
        self.feed_started = None;
        true
    }

    /// After feed feedback: return home from `current` window top-left.
    /// Caller should persist last_completed_at when this is first invoked after feed.
    pub fn start_reminder_return(&mut self, current: Point, now: Instant) -> bool {
        let home = self.reminder_origin.unwrap_or(current);
        if let Ok(s) = try_transition(&self.state, PetState::Reminder(ReminderStage::Returning)) {
            self.state = s;
            self.food_button_rect = None;
            self.movement.start(
                current,
                MovementTarget::ReminderHome(home),
                now,
                REMINDER_MOVE_DURATION,
            );
            self.switch_clip_for_state(now);
            debug!("reminder returning home");
            true
        } else {
            false
        }
    }

    pub fn defer_reminder_for_drag(&mut self) {
        self.pending_reminder = true;
        self.movement.cancel();
        self.feed_started = None;
        self.food_button_rect = None;
        debug!("reminder deferred for drag");
    }

    pub fn wants_reminder_window(&self) -> bool {
        matches!(
            self.state,
            PetState::Reminder(ReminderStage::Showing) | PetState::Reminder(ReminderStage::Feeding)
        )
    }

    pub fn is_reminder_moving(&self) -> bool {
        matches!(
            self.state,
            PetState::Reminder(ReminderStage::MovingToCenter)
                | PetState::Reminder(ReminderStage::Returning)
        )
    }

    // ── Helpers ──

    fn go_idle(&mut self, now: Instant) {
        self.go_idle_impl(now, false);
    }

    /// Return to base with a short captured-pose fade. One-shot tails are
    /// authored to end on idle_blink/000; the fade is a residual guard for any
    /// remaining pixel differences (docs F-AN-06).
    fn go_idle_with_settle(&mut self, now: Instant) {
        self.go_idle_impl(now, true);
    }

    fn go_idle_impl(&mut self, now: Instant, settle: bool) {
        let name = self.picker.pick_initial(now);
        let target_name = if self.library.get(&name).is_some() {
            name.clone()
        } else {
            IDLE_BASE.to_string()
        };

        // Allow Idle(any) -> Idle(base) by setting state directly when already idle.
        if self.state.is_idle() {
            self.state = PetState::Idle(target_name.clone());
            let target = self
                .library
                .get(&target_name)
                .or_else(|| self.library.get(IDLE_BASE))
                .map(|clip| clip.name.clone());
            if let Some(name) = target {
                if settle {
                    // Capture the oneshot's last displayed frame before swap.
                    self.begin_crossfade(now);
                    self.crossfade_display = true;
                } else {
                    self.crossfade_from = None;
                    self.crossfade_started = None;
                    self.crossfade_t = 1.0;
                    self.crossfade_display = false;
                }
                // Start base at frame 0 (sit identity) so exit from oneshot sit
                // bookend has no visible pose jump.
                if let Some(clip) = self.library.get(&name) {
                    self.player = AnimationPlayer::start(clip, now);
                }
                self.current_frame = 0;
                self.display_frame_f = 0.0;
            }
            self.home_position = None;
            self.drag_scale = 1.0;
            debug!("pet -> idle base {target_name}");
            return;
        }
        if let Ok(s) = try_transition(&self.state, PetState::Idle(target_name.clone())) {
            self.state = s;
            if let Some(clip) = self
                .library
                .get(&target_name)
                .or_else(|| self.library.get(IDLE_BASE))
            {
                self.crossfade_from = None;
                self.crossfade_started = None;
                self.crossfade_t = 1.0;
                self.crossfade_display = false;
                self.player = AnimationPlayer::start(clip, now);
                self.current_frame = 0;
                self.display_frame_f = 0.0;
            }
            self.home_position = None;
            self.drag_scale = 1.0;
            debug!("pet -> idle base {target_name}");
        }
    }

    fn switch_clip_for_state(&mut self, now: Instant) {
        let clip_name = match &self.state {
            PetState::Idle(name) => name.clone(),
            // Use base sit+blink (not warped watch) — head-sway clips caused double nose/mouth.
            PetState::Watching => IDLE_BASE.to_string(),
            PetState::Approaching { .. } => "approaching".to_string(),
            PetState::PlayingInteraction(name) => name.clone(),
            PetState::Dragging => "dragging".to_string(),
            PetState::HiddenAtEdge(_) => "edge_peek".to_string(),
            PetState::Reminder(ReminderStage::Feeding) => "reminder_feed".to_string(),
            PetState::Reminder(_) => "reminder_wave".to_string(),
            // Keep the current clip when possible so open doesn't crossfade-flash.
            PetState::MenuOpen => {
                let cur = self.player.clip_name().to_string();
                if cur.starts_with("idle_") || cur.is_empty() {
                    if cur.is_empty() {
                        IDLE_BASE.to_string()
                    } else {
                        cur
                    }
                } else {
                    IDLE_BASE.to_string()
                }
            }
        };

        if clip_name == self.player.clip_name() {
            return;
        }
        if self.library.get(&clip_name).is_none() {
            warn!(clip = %clip_name, "clip not found");
            return;
        }
        self.begin_crossfade(now);
        if let Some(clip) = self.library.get(&clip_name) {
            self.player = AnimationPlayer::start(clip, now);
            self.current_frame = 0;
            self.display_frame_f = 0.0;
            debug!(clip = %clip_name, "switched clip for state {}", self.state.name());
        }
    }
}
