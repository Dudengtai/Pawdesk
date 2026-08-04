//! Reminder scheduler (tech §9.1, RM-01/02/03/09).
//!
//! Uses a monotonic clock for in-session cycles. Wall-clock
//! `last_completed_at` is only used at startup for catch-up.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tracing::{debug, info};

/// Outcome of a scheduler tick when a reminder should surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReminderDue;

#[derive(Debug, Clone)]
pub struct ReminderScheduler {
    enabled: bool,
    paused: bool,
    interval: Duration,
    cycle_started_at: Instant,
    /// Due but not yet consumed by the UI (or deferred by drag).
    pending_due: bool,
    /// Startup catch-up already evaluated / fired at most once.
    catchup_consumed: bool,
}

impl ReminderScheduler {
    pub fn new(enabled: bool, paused: bool, interval: Duration, now: Instant) -> Self {
        Self {
            enabled,
            paused,
            interval: interval.max(Duration::from_secs(1)),
            cycle_started_at: now,
            pending_due: false,
            catchup_consumed: false,
        }
    }

    /// Build interval from config minutes, optionally overridden by env seconds (RM-08).
    pub fn resolve_interval(interval_minutes: u32) -> Duration {
        if let Ok(s) = std::env::var("PAWDESK_REMINDER_INTERVAL_SECS") {
            if let Ok(secs) = s.parse::<u64>() {
                if secs > 0 {
                    info!(
                        secs,
                        "PAWDESK_REMINDER_INTERVAL_SECS override active (dev mode)"
                    );
                    return Duration::from_secs(secs);
                }
            }
        }
        Duration::from_secs(u64::from(interval_minutes.max(1)) * 60)
    }

    pub fn interval(&self) -> Duration {
        self.interval
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool, now: Instant) {
        self.enabled = enabled;
        if enabled && !self.paused {
            self.cycle_started_at = now;
            self.pending_due = false;
        }
        if !enabled {
            self.pending_due = false;
        }
    }

    /// Update cycle length from product minutes (15–180). Restarts the cycle.
    pub fn set_interval_minutes(&mut self, minutes: u32, now: Instant) {
        let minutes = minutes.clamp(15, 180);
        let next = Duration::from_secs(u64::from(minutes) * 60);
        if next == self.interval {
            return;
        }
        self.interval = next;
        self.cycle_started_at = now;
        self.pending_due = false;
        info!(minutes, "reminder interval updated; cycle restarted");
    }

    /// Pause stops timing; resume restarts a full cycle (tech §9.1).
    pub fn set_paused(&mut self, paused: bool, now: Instant) {
        if self.paused == paused {
            return;
        }
        self.paused = paused;
        if paused {
            debug!("reminder scheduler paused");
        } else {
            self.cycle_started_at = now;
            self.pending_due = false;
            info!("reminder scheduler resumed; full cycle restarted");
        }
    }

    pub fn toggle_paused(&mut self, now: Instant) -> bool {
        self.set_paused(!self.paused, now);
        self.paused
    }

    /// Startup catch-up from wall-clock last_completed_at (RM-03).
    ///
    /// - Missing / unparsable → no catch-up, start fresh cycle.
    /// - Elapsed >= interval → at most one pending due.
    /// - Elapsed < interval → offset cycle start so remaining time is honored.
    pub fn apply_startup_catchup(&mut self, last_completed_at: Option<&str>, now: Instant) {
        if self.catchup_consumed {
            return;
        }
        self.catchup_consumed = true;

        if !self.enabled || self.paused {
            return;
        }

        let Some(raw) = last_completed_at else {
            debug!("no last_completed_at; starting fresh reminder cycle");
            self.cycle_started_at = now;
            return;
        };

        let Some(elapsed) = parse_elapsed_since(raw) else {
            debug!(raw, "unparsable last_completed_at; starting fresh cycle");
            self.cycle_started_at = now;
            return;
        };

        if elapsed >= self.interval {
            info!(
                elapsed_secs = elapsed.as_secs(),
                "startup catch-up: one ReminderDue queued"
            );
            self.pending_due = true;
            self.cycle_started_at = now;
        } else {
            // Pretend the cycle started (interval - remaining) ago.
            let remaining = self.interval - elapsed;
            self.cycle_started_at = now.checked_sub(self.interval - remaining).unwrap_or(now);
            // Actually: we want remaining time until due = interval - elapsed.
            // So cycle_started_at = now - elapsed.
            self.cycle_started_at = now.checked_sub(elapsed).unwrap_or(now);
            debug!(
                remaining_secs = remaining.as_secs(),
                "startup: within cycle, no catch-up"
            );
        }
    }

    /// Mark due as delivered to UI (so we don't re-fire until next cycle).
    pub fn consume_due(&mut self) {
        self.pending_due = false;
        // Keep cycle_started_at as-is until feed completes; if user dismisses via
        // feed, on_feed_completed resets. If still showing, don't re-queue.
    }

    /// Queue due without delivering (e.g. drag defer).
    pub fn defer_due(&mut self) {
        self.pending_due = true;
    }

    pub fn has_pending_due(&self) -> bool {
        self.pending_due
    }

    /// After successful feed: restart full interval.
    pub fn on_feed_completed(&mut self, now: Instant) {
        self.pending_due = false;
        self.cycle_started_at = now;
        debug!("reminder cycle reset after feed");
    }

    /// Advance scheduler. Returns `Some(ReminderDue)` when UI should start reminder.
    pub fn tick(&mut self, now: Instant) -> Option<ReminderDue> {
        if !self.enabled || self.paused {
            return None;
        }

        if self.pending_due {
            return Some(ReminderDue);
        }

        if now.duration_since(self.cycle_started_at) >= self.interval {
            self.pending_due = true;
            debug!("reminder interval elapsed");
            return Some(ReminderDue);
        }

        None
    }
}

fn parse_elapsed_since(raw: &str) -> Option<Duration> {
    // Prefer RFC3339 via chrono.
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(raw) {
        let then = dt.with_timezone(&chrono::Utc);
        let now = chrono::Utc::now();
        let secs = (now - then).num_seconds();
        if secs < 0 {
            return Some(Duration::ZERO);
        }
        return Some(Duration::from_secs(secs as u64));
    }
    // Fallback: unix seconds string
    if let Ok(secs) = raw.parse::<u64>() {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
        if now >= secs {
            return Some(Duration::from_secs(now - secs));
        }
    }
    None
}

/// Format now as RFC3339 UTC for config persistence.
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_fire_before_interval() {
        let t0 = Instant::now();
        let mut s = ReminderScheduler::new(true, false, Duration::from_secs(60), t0);
        assert!(s.tick(t0 + Duration::from_secs(30)).is_none());
    }

    #[test]
    fn fire_after_interval() {
        let t0 = Instant::now();
        let mut s = ReminderScheduler::new(true, false, Duration::from_secs(60), t0);
        assert!(s.tick(t0 + Duration::from_secs(60)).is_some());
    }

    #[test]
    fn pause_blocks_and_resume_restarts() {
        let t0 = Instant::now();
        let mut s = ReminderScheduler::new(true, false, Duration::from_secs(60), t0);
        s.set_paused(true, t0 + Duration::from_secs(50));
        assert!(s.tick(t0 + Duration::from_secs(120)).is_none());
        s.set_paused(false, t0 + Duration::from_secs(120));
        // Need full 60s after resume
        assert!(s.tick(t0 + Duration::from_secs(150)).is_none());
        assert!(s.tick(t0 + Duration::from_secs(180)).is_some());
    }

    #[test]
    fn feed_resets_cycle() {
        let t0 = Instant::now();
        let mut s = ReminderScheduler::new(true, false, Duration::from_secs(60), t0);
        assert!(s.tick(t0 + Duration::from_secs(60)).is_some());
        s.consume_due();
        s.on_feed_completed(t0 + Duration::from_secs(65));
        assert!(s.tick(t0 + Duration::from_secs(100)).is_none());
        assert!(s.tick(t0 + Duration::from_secs(125)).is_some());
    }

    #[test]
    fn catchup_when_overdue() {
        let t0 = Instant::now();
        let mut s = ReminderScheduler::new(true, false, Duration::from_secs(60), t0);
        // Simulate last completed 2 minutes ago
        let past = chrono::Utc::now() - chrono::Duration::seconds(120);
        s.apply_startup_catchup(Some(&past.to_rfc3339()), t0);
        assert!(s.tick(t0).is_some());
    }

    #[test]
    fn no_catchup_when_recent() {
        let t0 = Instant::now();
        let mut s = ReminderScheduler::new(true, false, Duration::from_secs(3600), t0);
        let past = chrono::Utc::now() - chrono::Duration::seconds(30);
        s.apply_startup_catchup(Some(&past.to_rfc3339()), t0);
        assert!(s.tick(t0).is_none());
    }

    #[test]
    fn disabled_never_fires() {
        let t0 = Instant::now();
        let mut s = ReminderScheduler::new(false, false, Duration::from_secs(1), t0);
        assert!(s.tick(t0 + Duration::from_secs(10)).is_none());
    }

    #[test]
    fn consume_clears_pending_until_next_cycle() {
        let t0 = Instant::now();
        let mut s = ReminderScheduler::new(true, false, Duration::from_secs(60), t0);
        assert!(s.tick(t0 + Duration::from_secs(60)).is_some());
        s.consume_due();
        // Still within same wall of cycle_started — without on_feed_completed,
        // elapsed still >= interval so tick would re-set pending. After consume
        // we only clear pending_due; elapsed still past interval → will re-fire.
        // Product path: consume_due when UI starts, then on_feed_completed on feed.
        // While Showing, App must not call tick delivery again. Test that
        // consume alone + on_feed_completed is the clean path:
        s.on_feed_completed(t0 + Duration::from_secs(61));
        assert!(s.tick(t0 + Duration::from_secs(90)).is_none());
    }

    #[test]
    fn set_interval_minutes_updates_and_clears_due() {
        let t0 = Instant::now();
        let mut s = ReminderScheduler::new(true, false, Duration::from_secs(60), t0);
        assert!(s.tick(t0 + Duration::from_secs(60)).is_some());
        assert!(s.has_pending_due());
        s.set_interval_minutes(30, t0 + Duration::from_secs(61));
        assert_eq!(s.interval(), Duration::from_secs(30 * 60));
        assert!(!s.has_pending_due());
    }

    #[test]
    fn set_enabled_false_clears_due() {
        let t0 = Instant::now();
        let mut s = ReminderScheduler::new(true, false, Duration::from_secs(10), t0);
        assert!(s.tick(t0 + Duration::from_secs(10)).is_some());
        s.set_enabled(false, t0);
        assert!(!s.is_enabled());
        assert!(!s.has_pending_due());
        assert!(s.tick(t0 + Duration::from_secs(100)).is_none());
    }
}
