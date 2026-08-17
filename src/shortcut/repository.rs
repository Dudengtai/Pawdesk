//! Shortcut list operations (SC-02/06/07). Does not touch disk files outside config.

use uuid::Uuid;

use super::model::ShortcutItem;

/// In-memory operations on a shortcut list (sort_order is source of truth).
#[derive(Debug, Clone, Default)]
pub struct ShortcutRepository {
    items: Vec<ShortcutItem>,
}

impl ShortcutRepository {
    pub fn from_items(items: Vec<ShortcutItem>) -> Self {
        let mut items: Vec<_> = items.into_iter().map(|i| i.sanitize()).collect();
        items.sort_by_key(|i| i.sort_order);
        renumber(&mut items);
        Self { items }
    }

    pub fn items(&self) -> &[ShortcutItem] {
        &self.items
    }

    pub fn into_items(self) -> Vec<ShortcutItem> {
        self.items
    }

    pub fn list_sorted(&self) -> Vec<ShortcutItem> {
        let mut v = self.items.clone();
        v.sort_by_key(|i| i.sort_order);
        v
    }

    pub fn list_enabled_sorted(&self) -> Vec<ShortcutItem> {
        self.list_sorted()
            .into_iter()
            .filter(|i| i.enabled)
            .collect()
    }

    pub fn get(&self, id: Uuid) -> Option<&ShortcutItem> {
        self.items.iter().find(|i| i.id == id)
    }

    pub fn add(&mut self, item: ShortcutItem) {
        let mut item = item.sanitize();
        item.sort_order = self.items.len() as u32;
        self.items.push(item);
        renumber(&mut self.items);
    }

    /// Remove config entry only (SC-06). Returns removed item if found.
    pub fn remove(&mut self, id: Uuid) -> Option<ShortcutItem> {
        let pos = self.items.iter().position(|i| i.id == id)?;
        let removed = self.items.remove(pos);
        renumber(&mut self.items);
        Some(removed)
    }

    pub fn set_enabled(&mut self, id: Uuid, enabled: bool) -> bool {
        if let Some(i) = self.items.iter_mut().find(|i| i.id == id) {
            i.enabled = enabled;
            true
        } else {
            false
        }
    }

    pub fn move_up(&mut self, id: Uuid) -> bool {
        let Some(pos) = self.items.iter().position(|i| i.id == id) else {
            return false;
        };
        if pos == 0 {
            return false;
        }
        self.items.swap(pos, pos - 1);
        renumber(&mut self.items);
        true
    }

    pub fn move_down(&mut self, id: Uuid) -> bool {
        let Some(pos) = self.items.iter().position(|i| i.id == id) else {
            return false;
        };
        if pos + 1 >= self.items.len() {
            return false;
        }
        self.items.swap(pos, pos + 1);
        renumber(&mut self.items);
        true
    }

    /// Record a successful dock launch. Returns false if `id` is unknown.
    pub fn record_launch(&mut self, id: Uuid) -> bool {
        let now = unix_now_ms();
        if let Some(i) = self.items.iter_mut().find(|i| i.id == id) {
            i.launch_count = i.launch_count.saturating_add(1);
            i.last_launched_at_ms = Some(now);
            true
        } else {
            false
        }
    }

    /// Enabled items that have been launched, most-used first.
    ///
    /// Sort: `launch_count` desc, then `last_launched_at_ms` desc. Missing
    /// paths and never-launched items are omitted.
    pub fn list_frequent(&self, limit: usize) -> Vec<ShortcutItem> {
        rank_frequent(self.items.iter(), limit)
    }

    pub fn reorder(&mut self, ordered_ids: &[Uuid]) {
        let mut next = Vec::new();
        for id in ordered_ids {
            if let Some(item) = self.items.iter().find(|i| i.id == *id).cloned() {
                next.push(item);
            }
        }
        // Append any missing (not in ordered_ids)
        for item in &self.items {
            if !next.iter().any(|i| i.id == item.id) {
                next.push(item.clone());
            }
        }
        renumber(&mut next);
        self.items = next;
    }
}

fn renumber(items: &mut [ShortcutItem]) {
    for (i, item) in items.iter_mut().enumerate() {
        item.sort_order = i as u32;
    }
}

fn unix_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Rank enabled, launched, still-valid shortcuts for the frequent strip.
pub fn rank_frequent<'a>(
    items: impl Iterator<Item = &'a ShortcutItem>,
    limit: usize,
) -> Vec<ShortcutItem> {
    let mut v: Vec<&ShortcutItem> = items
        .filter(|s| s.enabled && s.launch_count > 0 && s.is_path_valid())
        .collect();
    v.sort_by(|a, b| {
        b.launch_count
            .cmp(&a.launch_count)
            .then_with(|| b.last_launched_at_ms.cmp(&a.last_launched_at_ms))
    });
    v.into_iter().take(limit).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn item(name: &str, order: u32) -> ShortcutItem {
        ShortcutItem::new(name, PathBuf::from(format!("{name}.exe")), order)
    }

    #[test]
    fn add_assigns_order() {
        let mut repo = ShortcutRepository::default();
        repo.add(item("a", 99));
        repo.add(item("b", 0));
        let list = repo.list_sorted();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "a");
        assert_eq!(list[0].sort_order, 0);
        assert_eq!(list[1].name, "b");
        assert_eq!(list[1].sort_order, 1);
    }

    #[test]
    fn remove_only_config_entry() {
        let mut repo = ShortcutRepository::default();
        repo.add(item("a", 0));
        let id = repo.items()[0].id;
        let path = repo.items()[0].target_path.clone();
        let removed = repo.remove(id).unwrap();
        assert_eq!(removed.target_path, path);
        assert!(repo.items().is_empty());
        // Path string still present on removed value; we never delete files.
        assert_eq!(removed.target_path, PathBuf::from("a.exe"));
    }

    #[test]
    fn move_up_down() {
        let mut repo = ShortcutRepository::default();
        repo.add(item("a", 0));
        repo.add(item("b", 1));
        repo.add(item("c", 2));
        let b = repo.items()[1].id;
        assert!(repo.move_up(b));
        assert_eq!(repo.list_sorted()[0].name, "b");
        assert!(repo.move_down(b));
        assert_eq!(repo.list_sorted()[1].name, "b");
    }

    #[test]
    fn reorder_by_ids() {
        let mut repo = ShortcutRepository::default();
        repo.add(item("a", 0));
        repo.add(item("b", 1));
        repo.add(item("c", 2));
        let ids: Vec<_> = repo.items().iter().map(|i| i.id).collect();
        repo.reorder(&[ids[2], ids[0], ids[1]]);
        let names: Vec<_> = repo.list_sorted().iter().map(|i| i.name.clone()).collect();
        assert_eq!(names, vec!["c", "a", "b"]);
    }

    #[test]
    fn set_enabled() {
        let mut repo = ShortcutRepository::default();
        repo.add(item("a", 0));
        let id = repo.items()[0].id;
        assert!(repo.set_enabled(id, false));
        assert!(!repo.get(id).unwrap().enabled);
        assert!(repo.list_enabled_sorted().is_empty());
    }

    fn existing_item(name: &str, order: u32) -> ShortcutItem {
        ShortcutItem::new(name, PathBuf::from("."), order)
    }

    #[test]
    fn record_launch_increments_and_stamps_time() {
        let mut repo = ShortcutRepository::default();
        repo.add(existing_item("a", 0));
        let id = repo.items()[0].id;
        assert_eq!(repo.get(id).unwrap().launch_count, 0);
        assert!(repo.record_launch(id));
        let a = repo.get(id).unwrap();
        assert_eq!(a.launch_count, 1);
        assert!(a.last_launched_at_ms.is_some());
        let first_ts = a.last_launched_at_ms;
        assert!(repo.record_launch(id));
        let a = repo.get(id).unwrap();
        assert_eq!(a.launch_count, 2);
        assert!(a.last_launched_at_ms >= first_ts);
    }

    #[test]
    fn record_launch_unknown_id_is_false() {
        let mut repo = ShortcutRepository::default();
        assert!(!repo.record_launch(Uuid::nil()));
    }

    #[test]
    fn frequent_orders_by_count_then_recency() {
        let mut repo = ShortcutRepository::default();
        repo.add(existing_item("low", 0));
        repo.add(existing_item("high", 1));
        repo.add(existing_item("tie-old", 2));
        repo.add(existing_item("tie-new", 3));
        // high: 3, both ties: 2 (newer stamp first), low: 1
        repo.items[0].launch_count = 1;
        repo.items[0].last_launched_at_ms = Some(100);
        repo.items[1].launch_count = 3;
        repo.items[1].last_launched_at_ms = Some(200);
        repo.items[2].launch_count = 2;
        repo.items[2].last_launched_at_ms = Some(300);
        repo.items[3].launch_count = 2;
        repo.items[3].last_launched_at_ms = Some(400);

        let names: Vec<_> = repo
            .list_frequent(8)
            .iter()
            .map(|i| i.name.clone())
            .collect();
        assert_eq!(names, vec!["high", "tie-new", "tie-old", "low"]);
    }

    #[test]
    fn frequent_excludes_disabled_zero_and_missing() {
        let mut repo = ShortcutRepository::default();
        repo.add(existing_item("used", 0));
        repo.add(existing_item("never", 1));
        repo.add(existing_item("off", 2));
        repo.add(item("gone", 3)); // fake path → invalid
        let ids: Vec<_> = repo.items().iter().map(|i| i.id).collect();
        assert!(repo.record_launch(ids[0]));
        assert!(repo.record_launch(ids[2]));
        assert!(repo.record_launch(ids[3]));
        assert!(repo.set_enabled(ids[2], false));

        let names: Vec<_> = repo
            .list_frequent(8)
            .iter()
            .map(|i| i.name.clone())
            .collect();
        assert_eq!(names, vec!["used"]);
    }

    #[test]
    fn frequent_caps_at_limit() {
        let mut repo = ShortcutRepository::default();
        for i in 0..8 {
            repo.add(existing_item(&format!("a{i}"), i as u32));
            let id = repo.items()[i].id;
            for _ in 0..(8 - i) {
                assert!(repo.record_launch(id));
            }
        }
        assert_eq!(repo.list_frequent(6).len(), 6);
        assert_eq!(repo.list_frequent(6)[0].name, "a0");
        assert_eq!(repo.list_frequent(6)[5].name, "a5");
    }
}
