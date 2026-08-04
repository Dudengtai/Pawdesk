//! Reminder service: scheduler + message pool (M3).

mod messages;
mod scheduler;

pub use messages::pick_message;
pub use scheduler::{now_rfc3339, ReminderScheduler};
