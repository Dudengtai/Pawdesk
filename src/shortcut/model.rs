//! ShortcutItem model (tech §6.2, SC-01).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const MAX_NAME_LEN: usize = 64;
pub const MAX_PATH_LEN: usize = 512;
pub const MAX_ARG_LEN: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShortcutItem {
    pub id: Uuid,
    pub name: String,
    pub target_path: PathBuf,
    #[serde(default)]
    pub arguments: Vec<String>,
    #[serde(default)]
    pub working_directory: Option<PathBuf>,
    #[serde(default)]
    pub icon_path: Option<PathBuf>,
    pub sort_order: u32,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Successful launches from the dock (used by the frequent-icon strip).
    #[serde(default)]
    pub launch_count: u32,
    /// UNIX epoch milliseconds of the last successful dock launch.
    #[serde(default)]
    pub last_launched_at_ms: Option<u64>,
}

fn default_true() -> bool {
    true
}

impl ShortcutItem {
    pub fn new(name: impl Into<String>, target_path: PathBuf, sort_order: u32) -> Self {
        let name = truncate(name.into(), MAX_NAME_LEN);
        let target_path = truncate_path(target_path, MAX_PATH_LEN);
        Self {
            id: Uuid::new_v4(),
            name,
            target_path,
            arguments: Vec::new(),
            working_directory: None,
            icon_path: None,
            sort_order,
            enabled: true,
            launch_count: 0,
            last_launched_at_ms: None,
        }
    }

    /// Create from a user-selected path (.exe / .lnk / other).
    pub fn from_path(path: &Path, sort_order: u32) -> Self {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("未命名")
            .to_string();
        Self::new(name, path.to_path_buf(), sort_order)
    }

    pub fn is_path_valid(&self) -> bool {
        self.target_path.exists()
    }

    pub fn sanitize(mut self) -> Self {
        self.name = truncate(self.name, MAX_NAME_LEN);
        self.target_path = truncate_path(self.target_path, MAX_PATH_LEN);
        self.arguments = self
            .arguments
            .into_iter()
            .map(|a| truncate(a, MAX_ARG_LEN))
            .collect();
        self
    }
}

fn truncate(s: String, max: usize) -> String {
    if s.chars().count() <= max {
        s
    } else {
        s.chars().take(max).collect()
    }
}

fn truncate_path(p: PathBuf, max: usize) -> PathBuf {
    let s = p.to_string_lossy();
    if s.chars().count() <= max {
        p
    } else {
        PathBuf::from(s.chars().take(max).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_path_uses_stem() {
        let item = ShortcutItem::from_path(Path::new(r"C:\Windows\System32\notepad.exe"), 0);
        assert_eq!(item.name, "notepad");
        assert_eq!(item.sort_order, 0);
        assert!(item.enabled);
    }

    #[test]
    fn name_truncated() {
        let long = "啊".repeat(100);
        let item = ShortcutItem::new(long, PathBuf::from("a.exe"), 1);
        assert!(item.name.chars().count() <= MAX_NAME_LEN);
    }

    #[test]
    fn missing_launch_stats_default_to_zero() {
        let json = r#"{
            "id": "00000000-0000-0000-0000-000000000000",
            "name": "x",
            "target_path": "x.exe",
            "sort_order": 0
        }"#;
        let item: ShortcutItem = serde_json::from_str(json).unwrap();
        assert_eq!(item.launch_count, 0);
        assert_eq!(item.last_launched_at_ms, None);
        assert!(item.enabled);
    }
}
