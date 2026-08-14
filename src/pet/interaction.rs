//! Mouse distance detection and edge behavior rules (PET-06, PET-05, PET-12).
//!
//! Interaction polish: hysteresis + dwell so Watching does not flicker when the
//! cursor sits on threshold boundaries or flies past quickly.

use std::time::{Duration, Instant};

use crate::event::Point;
use crate::pet::state::Edge;
use crate::platform::Rect;

// ── Distance thresholds (design §5.3) ──

/// Beyond this distance the pet ignores the cursor (Far).
pub const FAR_THRESHOLD: f64 = 300.0;
/// Within this distance the pet is in the near watch band.
pub const MEDIUM_THRESHOLD: f64 = 120.0;
/// Extra distance required to *leave* Watching (exit hysteresis, px).
pub const FAR_EXIT_HYSTERESIS: f64 = 48.0;
/// Extra distance required to leave Near → Medium (px).
pub const NEAR_EXIT_HYSTERESIS: f64 = 24.0;
/// Cursor must stay Medium/Near this long before entering Watching.
pub const WATCH_DWELL: Duration = Duration::from_millis(140);

// ── Edge thresholds (design §5.2) ──

/// Distance from work-area border that triggers edge-hide (px).
pub const EDGE_THRESHOLD: f64 = 24.0;
/// Fraction of the window hidden when at edge (0.6 = 60% off-screen).
pub const HIDE_RATIO: f64 = 0.6;
/// Size of the clickable peek area (px, square).
pub const PEEK_HIT_SIZE: f64 = 40.0;

/// Three-tier distance classification for mouse interaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistanceLevel {
    /// > 300 px — no reaction.
    Far,
    /// 120–300 px — Watching.
    Medium,
    /// < 120 px — closer watch band (same Watching state as Medium).
    Near,
}

/// Interaction helpers with hysteresis state (stable Watching enter/exit).
#[derive(Debug, Clone)]
pub struct InteractionDetector {
    pub far_threshold: f64,
    pub medium_threshold: f64,
    /// Last stable level after hysteresis (None = treat as Far).
    last_level: DistanceLevel,
    /// When Medium/Near first observed while not yet Watching.
    dwell_since: Option<Instant>,
}

impl Default for InteractionDetector {
    fn default() -> Self {
        Self {
            far_threshold: FAR_THRESHOLD,
            medium_threshold: MEDIUM_THRESHOLD,
            last_level: DistanceLevel::Far,
            dwell_since: None,
        }
    }
}

impl InteractionDetector {
    /// Euclidean distance between pet center and cursor.
    pub fn compute_distance(pet_center: Point, cursor: Point) -> f64 {
        let dx = cursor.x - pet_center.x;
        let dy = cursor.y - pet_center.y;
        (dx * dx + dy * dy).sqrt()
    }

    /// Instantaneous classify (no hysteresis) — used by tests and diagnostics.
    pub fn compute_level(&self, pet_center: Point, cursor: Point) -> DistanceLevel {
        self.level_from_distance(Self::compute_distance(pet_center, cursor))
    }

    fn level_from_distance(&self, d: f64) -> DistanceLevel {
        if d >= self.far_threshold {
            DistanceLevel::Far
        } else if d >= self.medium_threshold {
            DistanceLevel::Medium
        } else {
            DistanceLevel::Near
        }
    }

    /// Stable level with enter/exit hysteresis (reduces boundary flicker).
    pub fn compute_level_stable(&mut self, pet_center: Point, cursor: Point) -> DistanceLevel {
        let d = Self::compute_distance(pet_center, cursor);
        let raw = self.level_from_distance(d);
        let next = match self.last_level {
            DistanceLevel::Far => {
                // Enter interest band only when clearly inside far threshold.
                if d < self.far_threshold - 8.0 {
                    if d < self.medium_threshold {
                        DistanceLevel::Near
                    } else {
                        DistanceLevel::Medium
                    }
                } else {
                    DistanceLevel::Far
                }
            }
            DistanceLevel::Medium => {
                if d >= self.far_threshold + FAR_EXIT_HYSTERESIS {
                    DistanceLevel::Far
                } else if d < self.medium_threshold {
                    DistanceLevel::Near
                } else {
                    DistanceLevel::Medium
                }
            }
            DistanceLevel::Near => {
                if d >= self.far_threshold + FAR_EXIT_HYSTERESIS {
                    DistanceLevel::Far
                } else if d >= self.medium_threshold + NEAR_EXIT_HYSTERESIS {
                    DistanceLevel::Medium
                } else {
                    DistanceLevel::Near
                }
            }
        };
        let _ = raw;
        self.last_level = next;
        next
    }

    /// Whether dwell has elapsed so Idle → Watching is allowed.
    /// Resets when cursor is Far.
    pub fn watch_dwell_ready(&mut self, level: DistanceLevel, now: Instant) -> bool {
        match level {
            DistanceLevel::Far => {
                self.dwell_since = None;
                false
            }
            DistanceLevel::Medium | DistanceLevel::Near => {
                let start = *self.dwell_since.get_or_insert(now);
                now.duration_since(start) >= WATCH_DWELL
            }
        }
    }

    /// Reset dwell (e.g. after menu close / drag).
    pub fn reset_dwell(&mut self) {
        self.dwell_since = None;
    }

    /// Detect whether the pet window is within `EDGE_THRESHOLD` of a work-area edge.
    /// Returns the nearest `Edge` if so, or `None`.
    pub fn detect_edge(pet_rect: Rect, work_area: Rect) -> Option<Edge> {
        // Distance from each side of the pet window to the corresponding work-area border.
        let dist_left = (pet_rect.x - work_area.x) as f64;
        let dist_right = (work_area.x + work_area.width - (pet_rect.x + pet_rect.width)) as f64;
        let dist_top = (pet_rect.y - work_area.y) as f64;
        let dist_bottom = (work_area.y + work_area.height - (pet_rect.y + pet_rect.height)) as f64;

        let edges = [
            (Edge::Left, dist_left),
            (Edge::Right, dist_right),
            (Edge::Top, dist_top),
            (Edge::Bottom, dist_bottom),
        ];

        let mut best: Option<(Edge, f64)> = None;
        for (edge, dist) in edges {
            if dist <= EDGE_THRESHOLD {
                match best {
                    Some((_, bd)) if dist >= bd => {}
                    _ => best = Some((edge, dist)),
                }
            }
        }
        best.map(|(e, _)| e)
    }

    /// Calculate the window top-left position when hiding at an edge.
    /// Moves `HIDE_RATIO` of `window_size` off-screen in the edge direction.
    pub fn compute_hidden_position(window_pos: Point, edge: Edge, window_size: u32) -> Point {
        let offset = window_size as f64 * HIDE_RATIO;
        match edge {
            Edge::Left => Point::new(window_pos.x - offset, window_pos.y),
            Edge::Right => Point::new(window_pos.x + offset, window_pos.y),
            Edge::Top => Point::new(window_pos.x, window_pos.y - offset),
            Edge::Bottom => Point::new(window_pos.x, window_pos.y + offset),
        }
    }

    /// Check whether a click (in screen coordinates) falls inside the peek hit area.
    /// The peek area is a `PEEK_HIT_SIZE` square centred on the visible portion of the window.
    pub fn is_in_peek_area(click: Point, window_pos: Point, edge: Edge, window_size: u32) -> bool {
        let s = window_size as f64;
        let half_peek = PEEK_HIT_SIZE / 2.0;

        // The visible centre shifts towards the screen interior when hidden.
        let (cx, cy) = match edge {
            Edge::Left => (
                window_pos.x + s - HIDE_RATIO * s * 0.5,
                window_pos.y + s / 2.0,
            ),
            Edge::Right => (window_pos.x + HIDE_RATIO * s * 0.5, window_pos.y + s / 2.0),
            Edge::Top => (
                window_pos.x + s / 2.0,
                window_pos.y + s - HIDE_RATIO * s * 0.5,
            ),
            Edge::Bottom => (window_pos.x + s / 2.0, window_pos.y + HIDE_RATIO * s * 0.5),
        };

        (click.x - cx).abs() <= half_peek && (click.y - cy).abs() <= half_peek
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Distance computation (PET-12) ──

    #[test]
    fn distance_zero() {
        let p = Point::new(100.0, 100.0);
        assert_eq!(InteractionDetector::compute_distance(p, p), 0.0);
    }

    #[test]
    fn distance_3_4_5() {
        let a = Point::new(0.0, 0.0);
        let b = Point::new(3.0, 4.0);
        assert_eq!(InteractionDetector::compute_distance(a, b), 5.0);
    }

    // ── Distance level boundaries (PET-12) ──

    #[test]
    fn level_far_at_300() {
        let det = InteractionDetector::default();
        let pet = Point::new(0.0, 0.0);
        assert_eq!(
            det.compute_level(pet, Point::new(300.0, 0.0)),
            DistanceLevel::Far
        );
    }

    #[test]
    fn level_medium_at_299() {
        let det = InteractionDetector::default();
        let pet = Point::new(0.0, 0.0);
        assert_eq!(
            det.compute_level(pet, Point::new(299.0, 0.0)),
            DistanceLevel::Medium
        );
    }

    #[test]
    fn level_medium_at_120() {
        let det = InteractionDetector::default();
        let pet = Point::new(0.0, 0.0);
        assert_eq!(
            det.compute_level(pet, Point::new(120.0, 0.0)),
            DistanceLevel::Medium
        );
    }

    #[test]
    fn level_near_at_119() {
        let det = InteractionDetector::default();
        let pet = Point::new(0.0, 0.0);
        assert_eq!(
            det.compute_level(pet, Point::new(119.0, 0.0)),
            DistanceLevel::Near
        );
    }

    #[test]
    fn level_near_at_zero() {
        let det = InteractionDetector::default();
        let pet = Point::new(0.0, 0.0);
        assert_eq!(det.compute_level(pet, pet), DistanceLevel::Near);
    }

    #[test]
    fn hysteresis_stays_medium_past_far_threshold() {
        let mut det = InteractionDetector::default();
        let pet = Point::new(0.0, 0.0);
        // Enter medium
        assert_eq!(
            det.compute_level_stable(pet, Point::new(200.0, 0.0)),
            DistanceLevel::Medium
        );
        // Still medium just past 300 until hysteresis (+48)
        assert_eq!(
            det.compute_level_stable(pet, Point::new(320.0, 0.0)),
            DistanceLevel::Medium
        );
        // Far after exit band
        assert_eq!(
            det.compute_level_stable(pet, Point::new(360.0, 0.0)),
            DistanceLevel::Far
        );
    }

    #[test]
    fn dwell_requires_time_in_band() {
        let mut det = InteractionDetector::default();
        let t0 = Instant::now();
        assert!(!det.watch_dwell_ready(DistanceLevel::Medium, t0));
        assert!(det.watch_dwell_ready(DistanceLevel::Medium, t0 + WATCH_DWELL));
        assert!(!det.watch_dwell_ready(DistanceLevel::Far, t0 + WATCH_DWELL + Duration::from_millis(10)));
    }

    // ── Edge detection (PET-12) ──

    #[test]
    fn edge_detected_left() {
        let work = Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let pet = Rect {
            x: 0,
            y: 500,
            width: 128,
            height: 128,
        };
        assert_eq!(
            InteractionDetector::detect_edge(pet, work),
            Some(Edge::Left)
        );
    }

    #[test]
    fn edge_detected_right() {
        let work = Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let pet = Rect {
            x: 1920 - 128,
            y: 500,
            width: 128,
            height: 128,
        };
        assert_eq!(
            InteractionDetector::detect_edge(pet, work),
            Some(Edge::Right)
        );
    }

    #[test]
    fn edge_detected_top() {
        let work = Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let pet = Rect {
            x: 500,
            y: 0,
            width: 128,
            height: 128,
        };
        assert_eq!(InteractionDetector::detect_edge(pet, work), Some(Edge::Top));
    }

    #[test]
    fn edge_detected_bottom() {
        let work = Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let pet = Rect {
            x: 500,
            y: 1080 - 128,
            width: 128,
            height: 128,
        };
        assert_eq!(
            InteractionDetector::detect_edge(pet, work),
            Some(Edge::Bottom)
        );
    }

    #[test]
    fn no_edge_when_centered() {
        let work = Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let pet = Rect {
            x: 800,
            y: 400,
            width: 128,
            height: 128,
        };
        assert_eq!(InteractionDetector::detect_edge(pet, work), None);
    }

    // ── Hidden position computation (PET-12) ──

    #[test]
    fn hidden_position_left() {
        let pos = Point::new(100.0, 200.0);
        let hidden = InteractionDetector::compute_hidden_position(pos, Edge::Left, 128);
        // 60% of 128 = 76.8
        assert!((hidden.x - (100.0 - 76.8)).abs() < 0.01);
        assert!((hidden.y - 200.0).abs() < 0.01);
    }

    #[test]
    fn hidden_position_right() {
        let pos = Point::new(100.0, 200.0);
        let hidden = InteractionDetector::compute_hidden_position(pos, Edge::Right, 128);
        assert!((hidden.x - (100.0 + 76.8)).abs() < 0.01);
        assert!((hidden.y - 200.0).abs() < 0.01);
    }

    #[test]
    fn hidden_position_top() {
        let pos = Point::new(100.0, 200.0);
        let hidden = InteractionDetector::compute_hidden_position(pos, Edge::Top, 128);
        assert!((hidden.x - 100.0).abs() < 0.01);
        assert!((hidden.y - (200.0 - 76.8)).abs() < 0.01);
    }

    #[test]
    fn hidden_position_bottom() {
        let pos = Point::new(100.0, 200.0);
        let hidden = InteractionDetector::compute_hidden_position(pos, Edge::Bottom, 128);
        assert!((hidden.x - 100.0).abs() < 0.01);
        assert!((hidden.y - (200.0 + 76.8)).abs() < 0.01);
    }

    // ── Peek hit area (PET-12) ──

    #[test]
    fn peek_area_hit_left_edge() {
        // Window at x=0, hidden left, visible portion on right side
        let window_pos = Point::new(0.0, 500.0);
        let size = 128u32;
        // Visible centre is roughly at x = 0 + 128 - 38.4 ≈ 89.6, y = 500 + 64 = 564
        let click = Point::new(89.6, 564.0);
        assert!(InteractionDetector::is_in_peek_area(
            click,
            window_pos,
            Edge::Left,
            size
        ));
    }

    #[test]
    fn peek_area_miss_far_away() {
        let window_pos = Point::new(0.0, 500.0);
        let click = Point::new(500.0, 500.0);
        assert!(!InteractionDetector::is_in_peek_area(
            click,
            window_pos,
            Edge::Left,
            128
        ));
    }
}
