//! Shortcut domain (M4): model, repository, launch, picker, icon extraction.

mod icon;
mod launcher;
mod model;
mod picker;
mod repository;

pub use icon::{extract_icon, IconRgba, IconShape};
pub(crate) use icon::scale_icon_rgba;
pub use launcher::launch;
pub use model::ShortcutItem;
pub use picker::{build_pick_context, pick_executable};
pub use repository::ShortcutRepository;
