//! Position interpolation for pet movement (PET-07).
//!
//! Drives smooth window-position transitions for hiding at an edge,
//! restoring from edge, and reminder hops to/from center.

use std::time::{Duration, Instant};

use crate::event::Point;
use crate::render::easing::{ease_in_out_cubic, ease_smooth};

/// Reminder hop: stay put while gathering (squash).
pub const REMINDER_GATHER_END: f32 = 0.16;
/// Reminder hop: land and sit (squash back).
pub const REMINDER_LAND_START: f32 = 0.84;
/// Already at destination — skip the hop.
pub const REMINDER_HOP_NEAR_PX: f64 = 56.0;

/// What the movement is heading towards, so the controller can react on completion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MovementTarget {
    /// Sliding partially off-screen to hide at an edge.
    EdgeHide(Point),
    /// Sliding back from a hidden edge position.
    EdgeRestore(Point),
    /// Reminder: hop to work-area center (window top-left).
    ReminderCenter(Point),
    /// Reminder: hop back to origin after feed.
    ReminderHome(Point),
}

impl MovementTarget {
    /// The destination point for this target.
    pub fn destination(&self) -> Point {
        match self {
            MovementTarget::EdgeHide(p)
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
    /// inactive, leaving the pet stuck in the finishing movement state).
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
    /// Reminder travel uses a **parabolic hop arc in overlay-local slots**;
    /// edge hide/restore still slides the HWND.
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
            let y = self.start.y + (dest.y - self.start.y) * eased as f64;
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
    /// Returns `None` when no movement is active. Used to drive reminder-hop
    /// sprite frames in lockstep with the hop arc (pose phase = motion phase).
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

    pub fn is_reminder_hop(&self) -> bool {
        matches!(
            self.target,
            Some(MovementTarget::ReminderCenter(_) | MovementTarget::ReminderHome(_))
        )
    }
}

/// Peak lift in the same units as `start`/`dest` (overlay-local px).
pub fn hop_arc_height(distance: f64) -> f64 {
    (32.0 + distance * 0.06).clamp(28.0, 72.0)
}

/// Overlay-local slot path: gather in place, ease-in-out flight, sit on land.
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

/// Sit squash / stretch for overlay travel. `(scale_x, scale_y)`; feet stay put.
pub fn reminder_squash_at(t: f32) -> (f32, f32) {
    let t = t.clamp(0.0, 1.0);
    if t <= REMINDER_GATHER_END {
        let u = if REMINDER_GATHER_END <= f32::EPSILON {
            1.0
        } else {
            t / REMINDER_GATHER_END
        };
        let k = ease_smooth(u);
        return (lerp(1.0, 1.08, k), lerp(1.0, 0.90, k));
    }
    if t >= REMINDER_LAND_START {
        let span = (1.0 - REMINDER_LAND_START).max(f32::EPSILON);
        let u = ((t - REMINDER_LAND_START) / span).clamp(0.0, 1.0);
        if u < 0.45 {
            let v = u / 0.45;
            return (lerp(0.92, 1.08, v), lerp(1.10, 0.90, v));
        }
        let v = ((u - 0.45) / 0.55).clamp(0.0, 1.0);
        return (lerp(1.08, 1.0, v), lerp(0.90, 1.0, v));
    }
    let span = REMINDER_LAND_START - REMINDER_GATHER_END;
    let u = ((t - REMINDER_GATHER_END) / span).clamp(0.0, 1.0);
    if u < 0.12 {
        let v = u / 0.12;
        return (lerp(1.08, 0.92, v), lerp(0.90, 1.10, v));
    }
    (0.92, 1.10)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

/// Suggested duration based on distance (PET-07).
/// Edge hide/restore: 250 ms.
pub const EDGE_DURATION: Duration = Duration::from_millis(250);
/// Legacy alias — prefer [`reminder_hop_duration`].
pub const REMINDER_MOVE_DURATION: Duration = Duration::from_millis(700);

/// Reminder hop length from overlay-slot travel distance.
pub fn reminder_hop_duration(distance: f64) -> Duration {
    if distance < REMINDER_HOP_NEAR_PX {
        return Duration::ZERO;
    }
    let ms = if distance <= 400.0 {
        let u = ((distance - REMINDER_HOP_NEAR_PX) / (400.0 - REMINDER_HOP_NEAR_PX)).clamp(0.0, 1.0);
        480.0 + u * 160.0
    } else {
        let u = ((distance - 400.0) / 800.0).clamp(0.0, 1.0);
        640.0 + u * 160.0
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
            MovementTarget::EdgeRestore(Point::new(100.0, 0.0)),
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
            MovementTarget::EdgeRestore(Point::new(100.0, 200.0)),
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
            Some(&MovementTarget::EdgeRestore(Point::new(100.0, 200.0)))
        );
        assert_eq!(
            mc.take_target(),
            Some(MovementTarget::EdgeRestore(Point::new(100.0, 200.0)))
        );
        assert!(mc.target_kind().is_none());
    }

    #[test]
    fn tick_midpoint_is_between_start_and_end() {
        let now = Instant::now();
        let mut mc = MovementController::default();
        mc.start(
            Point::new(0.0, 0.0),
            MovementTarget::EdgeRestore(Point::new(100.0, 0.0)),
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
            MovementTarget::EdgeRestore(Point::new(100.0, 0.0)),
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
    fn edge_restore_path_no_arc() {
        let mut m = MovementController::default();
        let start = Point::new(0.0, 100.0);
        let dest = Point::new(200.0, 100.0);
        let t0 = Instant::now();
        m.start(start, MovementTarget::EdgeRestore(dest), t0, Duration::from_millis(500));
        let mid = m.tick(t0 + Duration::from_millis(250)).expect("mid");
        assert!((mid.y - 100.0).abs() < 0.5, "edge restore should not hop, y={}", mid.y);
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
        assert!(reminder_hop_duration(80.0) >= Duration::from_millis(480));
        assert!(reminder_hop_duration(2000.0) <= Duration::from_millis(800));
    }

    #[test]
    fn reminder_squash_gather_then_stretch() {
        let (sx0, sy0) = reminder_squash_at(0.0);
        assert!((sx0 - 1.0).abs() < 0.02 && (sy0 - 1.0).abs() < 0.02);
        let (sx_g, sy_g) = reminder_squash_at(REMINDER_GATHER_END);
        assert!(sx_g > 1.02 && sy_g < 0.96, "gather squash sx={sx_g} sy={sy_g}");
        let mid = (REMINDER_GATHER_END + REMINDER_LAND_START) * 0.5;
        let (sx_a, sy_a) = reminder_squash_at(mid);
        assert!(sx_a < 0.96 && sy_a > 1.04, "air stretch sx={sx_a} sy={sy_a}");
        let (sx1, sy1) = reminder_squash_at(1.0);
        assert!((sx1 - 1.0).abs() < 0.02 && (sy1 - 1.0).abs() < 0.02);
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
