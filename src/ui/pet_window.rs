//! Pet main window interaction helpers (drag threshold + click).

use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, MouseButton as WinitMouseButton};
use winit::window::Window;

use crate::event::{MouseButton, Point};
use crate::platform;

/// Screen pixels of movement before a press becomes a drag (click vs drag).
/// Slightly higher than before so short jitter on click still opens the launcher.
pub const DRAG_THRESHOLD_PX: f64 = 10.0;

/// Tracks press / drag for click-to-menu vs drag-to-move.
#[derive(Debug, Default)]
pub struct DragState {
    /// True only after movement exceeds threshold.
    pub dragging: bool,
    /// Left button currently held (may not be dragging yet).
    pub press_active: bool,
    /// Cursor position relative to window top-left when pressed.
    pub grab_offset: Point,
    /// Screen cursor at press time.
    pub press_screen: Point,
}

impl DragState {
    pub fn on_mouse_input(
        &mut self,
        button: WinitMouseButton,
        state: ElementState,
        cursor_in_window: Point,
    ) -> Option<MouseButton> {
        let mapped = map_button(button);
        match (button, state) {
            (WinitMouseButton::Left, ElementState::Pressed) => {
                self.press_active = true;
                self.dragging = false;
                self.grab_offset = cursor_in_window;
                if let Ok((cx, cy)) = platform::cursor_pos() {
                    self.press_screen = Point::new(cx as f64, cy as f64);
                }
            }
            (WinitMouseButton::Left, ElementState::Released) => {
                // Callers should read `finish_press()` before/on release.
            }
            _ => {}
        }
        Some(mapped)
    }

    /// Call on cursor move while left is held. Returns true when drag just started.
    pub fn consider_drag_start(&mut self) -> bool {
        if !self.press_active || self.dragging {
            return false;
        }
        let Ok((cx, cy)) = platform::cursor_pos() else {
            return false;
        };
        let dx = cx as f64 - self.press_screen.x;
        let dy = cy as f64 - self.press_screen.y;
        if (dx * dx + dy * dy).sqrt() >= DRAG_THRESHOLD_PX {
            self.dragging = true;
            true
        } else {
            false
        }
    }

    /// Finish a left-button press. Returns true if it was a click (no drag).
    pub fn finish_press(&mut self) -> bool {
        let was_click = self.press_active && !self.dragging;
        self.press_active = false;
        self.dragging = false;
        was_click
    }

    /// Apply drag using current screen cursor position.
    pub fn apply_drag(&self, window: &Window) {
        if !self.dragging {
            return;
        }
        if let Ok((cx, cy)) = platform::cursor_pos() {
            let x = cx as f64 - self.grab_offset.x;
            let y = cy as f64 - self.grab_offset.y;
            window.set_outer_position(PhysicalPosition::new(x, y));
        }
    }
}

pub fn map_button(button: WinitMouseButton) -> MouseButton {
    match button {
        WinitMouseButton::Left => MouseButton::Left,
        WinitMouseButton::Right => MouseButton::Right,
        WinitMouseButton::Middle => MouseButton::Middle,
        WinitMouseButton::Back => MouseButton::Other(3),
        WinitMouseButton::Forward => MouseButton::Other(4),
        WinitMouseButton::Other(v) => MouseButton::Other(v),
    }
}

pub fn initial_window_size() -> PhysicalSize<u32> {
    PhysicalSize::new(
        platform::PET_WINDOW_LOGICAL_SIZE,
        platform::PET_WINDOW_LOGICAL_SIZE,
    )
}
