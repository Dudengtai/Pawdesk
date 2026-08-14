//! Position interpolation for pet movement (PET-07).
//!
//! Drives smooth window-position transitions for approaching the cursor,
//! returning home, hiding at edge, and restoring from edge.

use std::time::{Duration, Instant};

use crate::event::Point;
use crate::render::easing::{ease_in_out_cubic, ease_smooth};

/// Reminder hop: stay put while gathering (matches clip 0.00–0.18).
pub const REMINDER_GATHER_END: f32 = 0.18;
/// Reminder hop: land and sit (matches clip 0.88–1.00).
pub const REMINDER_LAND_START: f32 = 0.88;
/// Already at destination — skip the hop.
pub const REMINDER_HOP_NEAR_PX: f64 = 56.0;

/// What the movement is heading towards, so the controller can react on completion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MovementTarget {
    /// Moving towards the cursor (will enter PlayingInteraction on arrival).
    Cursor(Point),
    /// Returning to the saved home position (will enter Idle on arrival).
    Home(Point),
    /// Sliding partially off-screen to hide at an edge.
    EdgeHide(Point),
    /// Sliding back from a hidden edge position.
    EdgeRestore(Point),
    /// Reminder: slide to work-area center (window top-left).
    ReminderCenter(Point),
    /// Reminder: return to origin after feed.
    ReminderHome(Point),
}

impl MovementTarget {
    /// The destination point for this target.
    pub fn destination(&self) -> Point {
        match self {
            MovementTarget::Cursor(p)
            | MovementTarget::Home(p)
            | MovementTarget::EdgeHide(p)
            | MovementTarget::EdgeRestore(p)
            | MovementTarget::ReminderCenter(p)
            | MovementTarget::ReminderHome(p) => *p,
        }
    }
}

/// Interpolates a window position from `start` to `target.destination()` over `duration`.
#[derive(Debug, Clone)]
pub struct MovementController {
    start: Point,
    target: Option<MovementTarget>,
    started_at: Instant,
    duration: Duration,
    active: bool,
    /// Last position returned by `tick()` (for querying after completion).
    last_position: Option<Point>,
}

impl Default for MovementController {
    fn default() -> Self {
        Self::idle()
    }
}

impl MovementController {
    fn idle() -> Self {
        Self {
            start: Point::new(0.0, 0.0),
            target: None,
            started_at: Instant::now(),
            duration: Duration::ZERO,
            active: false,
            last_position: None,
        }
    }

    /// Begin a new movement, replacing any in-progress one.
    pub fn start(
        &mut self,
        start: Point,
        target: MovementTarget,
        now: Instant,
        duration: Duration,
    ) {
        self.start = start;
        self.target = Some(target);
        self.started_at = now;
        self.duration = duration;
        self.active = true;
        self.last_position = Some(start);
    }

    /// Whether a movement is currently in progress.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Destination of the current (or just-finished) movement, if any.
    ///
    /// Kept after `active` flips to false so the app can call `on_movement_complete`
    /// on the finishing frame (previously `target_kind` returned `None` when
    /// inactive, leaving the pet stuck in `Approaching`).
    pub fn target_kind(&self) -> Option<&MovementTarget> {
        self.target.as_ref()
    }

    /// Take the finished target once (call after movement becomes inactive).
    pub fn take_target(&mut self) -> Option<MovementTarget> {
        self.target.take()
    }

    /// Cancel the current movement (pet stays wherever it is).
    pub fn cancel(&mut self) {
        self.active = false;
        self.target = None;
    }

    /// Advance the interpolation. Returns `Some(point)` with the new window position
    /// while moving, or the final position when the movement just completed (and
    /// `is_active` becomes `false`). Returns `None` when idle.
    ///
    /// Cursor approaches use a **parabolic hop arc** so the pet feels like it
    /// pounces rather than sliding.
    pub fn tick(&mut self, now: Instant) -> Option<Point> {
        if !self.active {
            return None;
        }
        let target = self.target?;
        let dest = target.destination();

        let elapsed = now.duration_since(self.started_at);
        let t = if self.duration.is_zero() {
            1.0
        } else {
            (elapsed.as_secs_f32() / self.duration.as_secs_f32()).min(1.0)
        };

        let pos = if matches!(
            target,
            MovementTarget::ReminderCenter(_) | MovementTarget::ReminderHome(_)
        ) {
            reminder_hop_pos(self.start, dest, t)
        } else {
            let eased = ease_smooth(t);
            let x = self.start.x + (dest.x - self.start.x) * eased as f64;
            let mut y = self.start.y + (dest.y - self.start.y) * eased as f64;

            // Parabolic hop only for pouncing toward the cursor.
            if matches!(target, MovementTarget::Cursor(_)) {
                let dist = ((dest.x - self.start.x).hypot(dest.y - self.start.y)).max(1.0);
                let arc = hop_arc_height(dist).min(56.0);
                let lift = arc * (4.0 * t as f64 * (1.0 - t as f64));
                y -= lift;
            }
            Point::new(x, y)
        };
        self.last_position = Some(pos);

        if t >= 1.0 {
            self.active = false;
        }

        Some(pos)
    }

    /// The last position computed by `tick()`, or `None` if `tick` was never called.
    pub fn last_position(&self) -> Option<Point> {
        self.last_position
    }

    /// Normalized progress of the current movement in `[0, 1]`.
    ///
    /// Returns `None` when no movement is active. Used to drive pounce sprite
    /// frames in lockstep with the hop arc (pose phase = motion phase).
    pub fn progress(&self, now: Instant) -> Option<f32> {
        if !self.active {
            return None;
        }
        if self.duration.is_zero() {
            return Some(1.0);
        }
        let elapsed = now.duration_since(self.started_at);
        Some((elapsed.as_secs_f32() / self.duration.as_secs_f32()).clamp(0.0, 1.0))
    }

    pub fn is_cursor_approach(&self) -> bool {
        matches!(self.target, Some(MovementTarget::Cursor(_)))
    }

    pub fn is_reminder_hop(&self) -> bool {
        matches!(
            self.target,
            Some(MovementTarget::ReminderCenter(_) | MovementTarget::ReminderHome(_))
        )
    }
}

/// Peak lift in screen px (up = smaller y). Shared by cursor pounce and reminder hop.
pub fn hop_arc_height(distance: f64) -> f64 {
    (28.0 + distance * 0.08).clamp(24.0, 64.0)
}

/// Window path for reminder travel: gather in place, ease-in-out flight, sit on land.
pub fn reminder_hop_pos(start: Point, dest: Point, t: f32) -> Point {
    let t = t.clamp(0.0, 1.0);
    if t <= REMINDER_GATHER_END {
        return start;
    }
    if t >= REMINDER_LAND_START {
        return dest;
    }
    let span = REMINDER_LAND_START - REMINDER_GATHER_END;
    let u = ((t - REMINDER_GATHER_END) / span).clamp(0.0, 1.0);
    let eased = ease_in_out_cubic(u) as f64;
    let x = start.x + (dest.x - start.x) * eased;
    let mut y = start.y + (dest.y - start.y) * eased;
    let dist = ((dest.x - start.x).hypot(dest.y - start.y)).max(1.0);
    let lift = hop_arc_height(dist) * (4.0 * u as f64 * (1.0 - u as f64));
    y -= lift;
    Point::new(x, y)
}

/// Suggested duration based on distance (PET-07).
/// Approaching: 400–600 ms. Returning: 500–800 ms. Edge hide/restore: 250 ms.
pub fn approach_duration(distance: f64) -> Duration {
    // Map 0–500 px to 400–600 ms.
    let ms = 400.0 + (distance / 500.0).clamp(0.0, 1.0) * 200.0;
    Duration::from_millis(ms as u64)
}

pub fn return_duration(distance: f64) -> Duration {
    let ms = 500.0 + (distance / 500.0).clamp(0.0, 1.0) * 300.0;
    Duration::from_millis(ms as u64)
}

pub const EDGE_DURATION: Duration = Duration::from_millis(250);
/// Legacy alias — prefer [`reminder_hop_duration`].
pub const REMINDER_MOVE_DURATION: Duration = Duration::from_millis(700);

/// Reminder hop length from window travel distance.
pub fn reminder_hop_duration(distance: f64) -> Duration {
    if distance < REMINDER_HOP_NEAR_PX {
        return Duration::ZERO;
    }
    let ms = if distance <= 400.0 {
        let u = ((distance - REMINDER_HOP_NEAR_PX) / (400.0 - REMINDER_HOP_NEAR_PX)).clamp(0.0, 1.0);
        550.0 + u * 250.0
    } else {
        let u = ((distance - 400.0) / 800.0).clamp(0.0, 1.0);
        800.0 + u * 300.0
    };
    Duration::from_millis(ms.round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_at_start_returns_start() {
        let now = Instant::now();
        let mut mc = MovementController::default();
        mc.start(
            Point::new(0.0, 0.0),
            MovementTarget::Cursor(Point::new(100.0, 0.0)),
            now,
            Duration::from_millis(100),
        );
        let p = mc.tick(now).unwrap();
        assert!((p.x - 0.0).abs() < 0.01);
        assert!((p.y - 0.0).abs() < 0.01);
    }

    #[test]
    fn tick_at_end_returns_target() {
        let now = Instant::now();
        let mut mc = MovementController::default();
        mc.start(
            Point::new(0.0, 0.0),
            MovementTarget::Home(Point::new(100.0, 200.0)),
            now,
            Duration::from_millis(100),
        );
        let later = now + Duration::from_millis(100);
        let p = mc.tick(later).unwrap();
        assert!((p.x - 100.0).abs() < 0.01);
        assert!((p.y - 200.0).abs() < 0.01);
        assert!(!mc.is_active());
        // Target must remain available after finish so completion handlers can run.
        assert_eq!(
            mc.target_kind(),
            Some(&MovementTarget::Home(Point::new(100.0, 200.0)))
        );
        assert_eq!(
            mc.take_target(),
            Some(MovementTarget::Home(Point::new(100.0, 200.0)))
        );
        assert!(mc.target_kind().is_none());
    }

    #[test]
    fn tick_midpoint_is_between_start_and_end() {
        let now = Instant::now();
        let mut mc = MovementController::default();
        mc.start(
            Point::new(0.0, 0.0),
            MovementTarget::Home(Point::new(100.0, 0.0)),
            now,
            Duration::from_millis(100),
        );
        let mid = now + Duration::from_millis(50);
        let p = mc.tick(mid).unwrap();
        // ease_smooth(0.5) ≈ 1 - (0.5)^3 = 0.875
        assert!(p.x > 0.0 && p.x < 100.0);
        assert!((p.x - 87.5).abs() < 1.0);
        assert!(mc.is_active());
    }

    #[test]
    fn idle_returns_none() {
        let mut mc = MovementController::default();
        assert!(mc.tick(Instant::now()).is_none());
        assert!(!mc.is_active());
    }

    #[test]
    fn cancel_stops_movement() {
        let now = Instant::now();
        let mut mc = MovementController::default();
        mc.start(
            Point::new(0.0, 0.0),
            MovementTarget::Cursor(Point::new(100.0, 0.0)),
            now,
            Duration::from_millis(100),
        );
        mc.cancel();
        assert!(!mc.is_active());
        assert!(mc.tick(now).is_none());
    }

    #[test]
    fn target_kind_returns_active_target() {
        let now = Instant::now();
        let mut mc = MovementController::default();
        mc.start(
            Point::new(0.0, 0.0),
            MovementTarget::EdgeHide(Point::new(50.0, 50.0)),
            now,
            Duration::from_millis(250),
        );
        assert_eq!(
            mc.target_kind(),
            Some(&MovementTarget::EdgeHide(Point::new(50.0, 50.0)))
        );
    }

    #[test]
    fn approach_duration_in_range() {
        assert!(approach_duration(0.0) >= Duration::from_millis(400));
        assert!(approach_duration(500.0) <= Duration::from_millis(600));
    }

    #[test]
    fn cursor_path_has_arc_lift() {
        let mut m = MovementController::default();
        let start = Point::new(0.0, 100.0);
        let dest = Point::new(200.0, 100.0);
        let t0 = Instant::now();
        m.start(start, MovementTarget::Cursor(dest), t0, Duration::from_millis(500));
        // Midpoint of ease_smooth is not exactly 0.5 time, but around 250ms still has lift.
        let mid = m.tick(t0 + Duration::from_millis(250)).expect("mid");
        // Without arc, y would stay 100; with arc, y must be lower (screen-up).
        assert!(mid.y < 100.0, "expected hop arc, y={}", mid.y);
        let end = m.tick(t0 + Duration::from_millis(500)).expect("end");
        assert!((end.y - 100.0).abs() < 1.0);
        assert!((end.x - 200.0).abs() < 1.0);
    }

    #[test]
    fn home_path_no_arc() {
        let mut m = MovementController::default();
        let start = Point::new(0.0, 100.0);
        let dest = Point::new(200.0, 100.0);
        let t0 = Instant::now();
        m.start(start, MovementTarget::Home(dest), t0, Duration::from_millis(500));
        let mid = m.tick(t0 + Duration::from_millis(250)).expect("mid");
        assert!((mid.y - 100.0).abs() < 0.5, "home should not hop, y={}", mid.y);
    }

    #[test]
    fn return_duration_in_range() {
        assert!(return_duration(0.0) >= Duration::from_millis(500));
        assert!(return_duration(500.0) <= Duration::from_millis(800));
    }

    #[test]
    fn reminder_gather_stays_at_start() {
        let start = Point::new(10.0, 100.0);
        let dest = Point::new(210.0, 100.0);
        let p = reminder_hop_pos(start, dest, 0.10);
        assert!((p.x - start.x).abs() < 0.01);
        assert!((p.y - start.y).abs() < 0.01);
    }

    #[test]
    fn reminder_flight_has_arc() {
        let start = Point::new(0.0, 200.0);
        let dest = Point::new(400.0, 200.0);
        let mid_t = (REMINDER_GATHER_END + REMINDER_LAND_START) * 0.5;
        let mid = reminder_hop_pos(start, dest, mid_t);
        assert!(mid.y < 200.0, "expected hop lift, y={}", mid.y);
        assert!(mid.x > 0.0 && mid.x < 400.0);
        let end = reminder_hop_pos(start, dest, 1.0);
        assert!((end.x - dest.x).abs() < 0.01);
        assert!((end.y - dest.y).abs() < 0.01);
    }

    #[test]
    fn reminder_land_pins_dest() {
        let start = Point::new(0.0, 50.0);
        let dest = Point::new(80.0, 90.0);
        let p = reminder_hop_pos(start, dest, 0.90);
        assert!((p.x - dest.x).abs() < 0.01);
        assert!((p.y - dest.y).abs() < 0.01);
    }

    #[test]
    fn reminder_hop_duration_near_is_zero() {
        assert_eq!(reminder_hop_duration(0.0), Duration::ZERO);
        assert_eq!(reminder_hop_duration(REMINDER_HOP_NEAR_PX - 1.0), Duration::ZERO);
        assert!(reminder_hop_duration(80.0) >= Duration::from_millis(550));
        assert!(reminder_hop_duration(2000.0) <= Duration::from_millis(1100));
    }

    #[test]
    fn reminder_home_path_has_arc() {
        let mut m = MovementController::default();
        let start = Point::new(0.0, 120.0);
        let dest = Point::new(300.0, 120.0);
        let t0 = Instant::now();
        m.start(
            start,
            MovementTarget::ReminderHome(dest),
            t0,
            Duration::from_millis(800),
        );
        let mid = m.tick(t0 + Duration::from_millis(400)).expect("mid");
        assert!(mid.y < 120.0, "return hop should arc, y={}", mid.y);
        assert!(m.is_reminder_hop());
    }
}
