//! Rendering abstraction (tech §4.1).

pub mod easing;
pub mod menu_ui;
pub mod reminder_ui;
pub mod text;
pub mod yawn_bubble;
// Kept for future GPU overlays; pet present path is CPU UpdateLayeredWindow only.
#[allow(dead_code)]
mod wgpu_renderer;
#[allow(unused_imports)]
pub use wgpu_renderer::WgpuRenderer;

/// Sprite draw command.
#[derive(Debug, Clone)]
pub struct SpriteDrawCommand {
    pub texture_id: u32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub uv: [f32; 4], // min_u, min_v, max_u, max_v
}

/// Text draw command (composed into textures for M3; trait kept for interface).
#[derive(Debug, Clone)]
pub struct TextDrawCommand {
    pub text: String,
    pub x: f32,
    pub y: f32,
}

/// Panel draw command.
#[derive(Debug, Clone)]
pub struct PanelDrawCommand {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Button draw command.
#[derive(Debug, Clone)]
pub struct ButtonDrawCommand {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub label: String,
}

/// High-level renderer interface used by business modules.
pub trait Renderer {
    fn draw_sprite(&mut self, sprite: SpriteDrawCommand);
    fn draw_text(&mut self, text: TextDrawCommand);
    fn draw_panel(&mut self, panel: PanelDrawCommand);
    fn draw_button(&mut self, button: ButtonDrawCommand);
}
