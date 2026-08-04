//! Debounced config persistence (tech §4.2).

use std::time::{Duration, Instant};

use super::{AppConfig, ConfigRepository};
use crate::error::AppError;

/// Schedules config saves so rapid drag events do not thrash disk.
pub struct DebouncedSaver {
    repo: ConfigRepository,
    dirty: bool,
    last_mark: Option<Instant>,
    delay: Duration,
}

impl DebouncedSaver {
    pub fn new(repo: ConfigRepository) -> Self {
        Self {
            repo,
            dirty: false,
            last_mark: None,
            delay: Duration::from_millis(500),
        }
    }

    pub fn repository(&self) -> &ConfigRepository {
        &self.repo
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        self.last_mark = Some(Instant::now());
    }

    /// Save if dirty and delay elapsed.
    pub fn tick(&mut self, config: &AppConfig) -> Result<(), AppError> {
        if !self.dirty {
            return Ok(());
        }
        let Some(t) = self.last_mark else {
            return Ok(());
        };
        if t.elapsed() < self.delay {
            return Ok(());
        }
        self.repo.save(config)?;
        self.dirty = false;
        Ok(())
    }

    /// Force immediate save (e.g. on exit).
    pub fn flush(&mut self, config: &AppConfig) -> Result<(), AppError> {
        if self.dirty || true {
            // Always flush on exit so latest in-memory config is persisted.
            self.repo.save(config)?;
            self.dirty = false;
        }
        Ok(())
    }
}
