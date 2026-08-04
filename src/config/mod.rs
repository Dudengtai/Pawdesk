//! Configuration model, persistence, and debounced saves (M1).

mod debounce;
mod repository;

pub use debounce::DebouncedSaver;
pub use repository::ConfigRepository;

use serde::{Deserialize, Deserializer, Serialize};

use crate::shortcut::ShortcutItem;

pub const SCHEMA_VERSION: u32 = 2;

/// Product range for reminder interval (minutes).
pub const REMINDER_INTERVAL_MIN: u32 = 15;
pub const REMINDER_INTERVAL_MAX: u32 = 180;

/// Clamp reminder interval to the allowed product range.
pub fn clamp_interval_minutes(v: u32) -> u32 {
    v.clamp(REMINDER_INTERVAL_MIN, REMINDER_INTERVAL_MAX)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub schema_version: u32,
    pub pet: PetConfig,
    pub reminder: ReminderConfig,
    /// User shortcuts (CFG-08). Skips invalid legacy entries instead of failing the whole file.
    #[serde(default, deserialize_with = "deserialize_shortcuts")]
    pub shortcuts: Vec<ShortcutItem>,
    pub window: WindowConfig,
}

fn deserialize_shortcuts<'de, D>(deserializer: D) -> Result<Vec<ShortcutItem>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    let Some(arr) = value.as_array() else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for v in arr {
        if let Ok(item) = serde_json::from_value::<ShortcutItem>(v.clone()) {
            out.push(item.sanitize());
        }
    }
    Ok(out)
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            pet: PetConfig::default(),
            reminder: ReminderConfig::default(),
            shortcuts: Vec::new(),
            window: WindowConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PetConfig {
    /// Relative scale (1.0 = 128 logical px baseline).
    #[serde(default = "default_scale")]
    pub scale: f32,
    /// 0.0–1.0 visual opacity (reserved; M1 always draws opaque sprite alpha).
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    /// Auto edge-hide when near work-area border (SET-04).
    #[serde(default = "default_true")]
    pub edge_hide_enabled: bool,
}

fn default_scale() -> f32 {
    1.0
}
fn default_opacity() -> f32 {
    1.0
}

impl Default for PetConfig {
    fn default() -> Self {
        Self {
            scale: 1.0,
            opacity: 1.0,
            edge_hide_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReminderConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Default 60 minutes (product default). Debug override via env
    /// `PAWDESK_REMINDER_INTERVAL_SECS` (not stored here).
    #[serde(default = "default_interval")]
    pub interval_minutes: u32,
    /// Tray pause flag (persisted).
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub custom_messages: Vec<String>,
    /// RFC3339 UTC timestamp of last successful feed (CFG-07).
    #[serde(default)]
    pub last_completed_at: Option<String>,
}

fn default_true() -> bool {
    true
}
fn default_interval() -> u32 {
    60
}

impl Default for ReminderConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_minutes: 60,
            paused: false,
            custom_messages: Vec::new(),
            last_completed_at: None,
        }
    }
}

impl ReminderConfig {
    /// Normalize fields after load or UI edit.
    pub fn sanitize(&mut self) {
        self.interval_minutes = clamp_interval_minutes(self.interval_minutes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_interval_bounds() {
        assert_eq!(clamp_interval_minutes(1), 15);
        assert_eq!(clamp_interval_minutes(60), 60);
        assert_eq!(clamp_interval_minutes(999), 180);
    }

    #[test]
    fn migrate_bumps_schema_and_sanitizes() {
        let mut cfg = AppConfig::default();
        cfg.schema_version = 1;
        cfg.reminder.interval_minutes = 5;
        let m = migrate_config(cfg);
        assert_eq!(m.schema_version, SCHEMA_VERSION);
        assert_eq!(m.reminder.interval_minutes, 15);
        assert!(m.pet.edge_hide_enabled);
    }
}

/// CFG-06: bring older configs up to current schema.
pub fn migrate_config(mut cfg: AppConfig) -> AppConfig {
    if cfg.schema_version == 0 {
        cfg.schema_version = 1;
    }
    // v1 → v2: pet.edge_hide_enabled defaulted via serde; just bump version.
    if cfg.schema_version < SCHEMA_VERSION {
        cfg.schema_version = SCHEMA_VERSION;
    }
    cfg.reminder.sanitize();
    if cfg.pet.scale < 0.5 {
        cfg.pet.scale = 0.5;
    }
    if cfg.pet.scale > 2.0 {
        cfg.pet.scale = 2.0;
    }
    cfg.pet.opacity = cfg.pet.opacity.clamp(0.2, 1.0);
    cfg
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WindowConfig {
    /// Outer position X in screen physical pixels (optional until first drag).
    pub x: Option<i32>,
    pub y: Option<i32>,
    /// Optional monitor hint string for future multi-monitor restore.
    #[serde(default)]
    pub monitor_hint: Option<String>,
}
