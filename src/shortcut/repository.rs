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
}
