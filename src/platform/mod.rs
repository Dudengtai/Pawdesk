//! Platform abstraction layer.

#[cfg(windows)]
pub mod windows;

#[cfg(windows)]
pub use windows::*;

/// Axis-aligned rectangle in screen coordinates (physical pixels unless noted).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }

    pub fn center(&self) -> (i32, i32) {
        (self.x + self.width / 2, self.y + self.height / 2)
    }
}

/// Monitor / work-area description.
#[derive(Debug, Clone)]
pub struct MonitorInfo {
    pub name: String,
    pub bounds: Rect,
    pub work_area: Rect,
    pub is_primary: bool,
}
