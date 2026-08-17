use std::time::Instant;

/// Logical 2D point in window or screen coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// Mouse button identifiers used by the app event layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Other(u16),
}

/// Domain / UI events (tech §12, M0 subset + forward-compatible variants).
#[derive(Debug, Clone)]
pub enum AppEvent {
    Tick(Instant),
    MouseMoved(Point),
    MousePressed(MouseButton),
    MouseReleased(MouseButton),
    WindowMoved(Point),
    ReminderDue,
    FeedCompleted,
    ShortcutSelected(String),
    ConfigChanged,
    TrayCommand(TrayCommand),
    RequestExit,
}

/// System tray commands (prd F-TR-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    ShowPet,
    HidePet,
    /// Increase pet display scale by one step.
    PetScaleUp,
    /// Decrease pet display scale by one step.
    PetScaleDown,
    Exit,
}
