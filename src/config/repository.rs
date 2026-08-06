//! Atomic config load/save with backup (tech §11).

use std::fs;
use std::path::{Path, PathBuf};

use tracing::{info, warn};

use super::{migrate_config, AppConfig, SCHEMA_VERSION};
use crate::error::AppError;

pub struct ConfigRepository {
    config_path: PathBuf,
    backup_path: PathBuf,
}

impl ConfigRepository {
    pub fn default_paths() -> Result<Self, AppError> {
        let appdata = std::env::var_os("APPDATA")
            .ok_or_else(|| AppError::Platform("APPDATA is not set".into()))?;
        let dir = PathBuf::from(appdata).join("PawDesk");
        let backups = dir.join("backups");
        Ok(Self {
            config_path: dir.join("config.json"),
            backup_path: backups.join("config.json.bak"),
        })
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub fn load(&self) -> AppConfig {
        match self.try_load(&self.config_path) {
            Ok(cfg) => {
                info!(
                    path = %self.config_path.display(),
                    pet_scale = cfg.pet.scale,
                    "config loaded"
                );
                return cfg;
            }
            Err(e) => warn!(
                path = %self.config_path.display(),
                error = %e,
                "primary config load failed"
            ),
        }

        match self.try_load(&self.backup_path) {
            Ok(cfg) => {
                warn!(
                    path = %self.backup_path.display(),
                    pet_scale = cfg.pet.scale,
                    "loaded config from backup; rewriting primary"
                );
                // Repair primary so next launch does not keep falling back to bak.
                if let Err(e) = self.save(&cfg) {
                    warn!(error = %e, "failed to rewrite primary config after backup load");
                }
                return cfg;
            }
            Err(e) => warn!(
                path = %self.backup_path.display(),
                error = %e,
                "backup config load failed; using defaults"
            ),
        }

        let cfg = AppConfig::default();
        if let Err(e) = self.save(&cfg) {
            warn!(error = %e, "failed to write default config");
        }
        cfg
    }

    fn try_load(&self, path: &Path) -> Result<AppConfig, AppError> {
        let bytes = fs::read(path).map_err(|source| AppError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        // Strip UTF-8 BOM (PowerShell Set-Content often writes one and breaks serde).
        let text = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
            String::from_utf8_lossy(&bytes[3..]).into_owned()
        } else {
            String::from_utf8_lossy(&bytes).into_owned()
        };
        let mut cfg: AppConfig = serde_json::from_str(text.trim_start_matches('\u{feff}'))
            .map_err(|e| AppError::Config(format!("parse {}: {e}", path.display())))?;
        if cfg.schema_version > SCHEMA_VERSION {
            warn!(
                found = cfg.schema_version,
                expected = SCHEMA_VERSION,
                "config schema newer than app; loading as-is"
            );
        }
        cfg = migrate_config(cfg);
        Ok(cfg)
    }

    /// Atomic write: tmp → replace, then update backup copy.
    pub fn save(&self, config: &AppConfig) -> Result<(), AppError> {
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent).map_err(|source| AppError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        if let Some(parent) = self.backup_path.parent() {
            fs::create_dir_all(parent).map_err(|source| AppError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let json = serde_json::to_string_pretty(config)
            .map_err(|e| AppError::Config(format!("serialize config: {e}")))?;

        let tmp = self.config_path.with_extension("json.tmp");
        fs::write(&tmp, &json).map_err(|source| AppError::Io {
            path: tmp.clone(),
            source,
        })?;

        // On Windows, rename over existing may fail; remove then rename.
        if self.config_path.exists() {
            let _ = fs::remove_file(&self.config_path);
        }
        fs::rename(&tmp, &self.config_path).map_err(|source| AppError::Io {
            path: self.config_path.clone(),
            source,
        })?;

        if let Err(e) = fs::write(&self.backup_path, &json) {
            warn!(
                path = %self.backup_path.display(),
                error = %e,
                "failed to write config backup"
            );
        }

        info!(path = %self.config_path.display(), "config saved");
        Ok(())
    }
}
