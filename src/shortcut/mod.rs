//! Shortcut domain (M4): model, repository, launch, picker.

mod launcher;
mod model;
mod picker;
mod repository;

pub use launcher::launch;
pub use model::ShortcutItem;
pub use picker::pick_executable;
pub use repository::ShortcutRepository;
