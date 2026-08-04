//! Minimal wgpu 2D sprite renderer (M0/M1).

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use tracing::{debug, info, warn};
use winit::window::Window;

use super::{ButtonDrawCommand, PanelDrawCommand, Renderer, SpriteDrawCommand, TextDrawCommand};
use crate::error::AppError;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 2],
    uv: [f32; 2],
}

pub struct WgpuRenderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    size: winit::dpi::PhysicalSize<u32>,
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: wgpu::BindGroup,
    sampler: wgpu::Sampler,
    texture: wgpu::Texture,
    texture_size: (u32, u32),
    vertex_buffer: wgpu::Buffer,
    sprite_texture_id: u32,
    pending_sprites: Vec<SpriteDrawCommand>,
    stub_logged: bool,
}

impl WgpuRenderer {
    pub fn new(
        window: Arc<Window>,
        sprite_rgba: &[u8],
        sprite_size: (u32, u32),
    ) -> Result<Self, AppError> {
        let size = window.inner_size();
        let size = if size.width == 0 || size.height == 0 {
            winit::dpi::PhysicalSize::new(128, 128)
        } else {
            size
        };

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });

        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| AppError::Render(format!("create_surface failed: {e}")))?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .ok_or_else(|| {
            AppError::Render("request_adapter failed: no suitable GPU adapter".into())
        })?;

        info!(
            "wgpu adapter: {:?} backend={:?}",
            adapter.get_info().name,
            adapter.get_info().backend
        );

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("pawdesk-device"),
                required_features: wgpu::Features::empty(),
                required_limits:
                    wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::MemoryUsage,
            },
            None,
        ))
        .map_err(|e| AppError::Render(format!("request_device failed: {e}")))?;

        let caps = surface.get_capabilities(&adapter);
        // Prefer BGRA on Windows (DWM composition path); keep sRGB if available.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| *f == wgpu::TextureFormat::Bgra8UnormSrgb)
            .or_else(|| {
                caps.formats
                    .iter()
                    .copied()
                    .find(|f| *f == wgpu::TextureFormat::Bgra8Unorm)
            })
            .or_else(|| caps.formats.iter().copied().find(|f| f.is_srgb()))
            .unwrap_or(caps.formats[0]);

        // Transparent windows require Pre/Post multiplied alpha. Opaque => white/black square.
        let alpha_mode = if caps
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::PreMultiplied)
        {
            wgpu::CompositeAlphaMode::PreMultiplied
        } else if caps
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::PostMultiplied)
        {
            wgpu::CompositeAlphaMode::PostMultiplied
        } else if caps
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::Inherit)
        {
            warn!("using CompositeAlphaMode::Inherit (no explicit premult)");
            wgpu::CompositeAlphaMode::Inherit
        } else {
            warn!(
                "no pre/post multiplied alpha mode; using {:?}. window may show solid square!",
                caps.alpha_modes.first()
            );
            caps.alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Opaque)
        };

        info!(
            "surface format={format:?} alpha_mode={alpha_mode:?} alpha_modes={:?} present_modes={:?}",
            caps.alpha_modes, caps.present_modes
        );

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sprite-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let (texture, bind_group) = create_sprite_texture(
            &device,
            &queue,
            &bind_group_layout,
            &sampler,
            sprite_rgba,
            sprite_size,
        )?;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sprite-shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(SPRITE_SHADER)),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sprite-pipeline-layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sprite-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2],
                }],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    // Replace/over onto magenta key; fragment always writes opaque.
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sprite-vb"),
            size: (std::mem::size_of::<Vertex>() * 6 * 32) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Ok(Self {
            surface,
            device,
            queue,
            config,
            size,
            pipeline,
            bind_group_layout,
            bind_group,
            sampler,
            texture,
            texture_size: sprite_size,
            vertex_buffer,
            sprite_texture_id: 1,
            pending_sprites: Vec::new(),
            stub_logged: false,
        })
    }

    pub fn sprite_texture_id(&self) -> u32 {
        self.sprite_texture_id
    }

    /// Upload a single frame (or full sheet) RGBA into the GPU sprite texture.
    pub fn update_sprite_rgba(&mut self, rgba: &[u8], size: (u32, u32)) -> Result<(), AppError> {
        if size.0 == 0 || size.1 == 0 {
            return Err(AppError::Render("invalid sprite size".into()));
        }
        if size != self.texture_size {
            let (texture, bind_group) = create_sprite_texture(
                &self.device,
                &self.queue,
                &self.bind_group_layout,
                &self.sampler,
                rgba,
                size,
            )?;
            self.texture = texture;
            self.bind_group = bind_group;
            self.texture_size = size;
            return Ok(());
        }

        let extent = wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        };
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * size.0),
                rows_per_image: Some(size.1),
            },
            extent,
        );
        Ok(())
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width == 0 || new_size.height == 0 {
            return;
        }
        self.size = new_size;
        self.config.width = new_size.width;
        self.config.height = new_size.height;
        self.surface.configure(&self.device, &self.config);
        debug!("surface resized to {}x{}", new_size.width, new_size.height);
    }

    pub fn render_frame(&mut self) -> Result<(), AppError> {
        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                self.surface.get_current_texture().map_err(|e| {
                    AppError::Render(format!("get_current_texture retry failed: {e}"))
                })?
            }
            Err(e) => {
                return Err(AppError::Render(format!("get_current_texture failed: {e}")));
            }
        };

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut vertices: Vec<Vertex> = Vec::with_capacity(self.pending_sprites.len() * 6);
        let w = self.size.width.max(1) as f32;
        let h = self.size.height.max(1) as f32;

        for sprite in &self.pending_sprites {
            let x0 = (sprite.x / w) * 2.0 - 1.0;
            let y0 = 1.0 - (sprite.y / h) * 2.0;
            let x1 = ((sprite.x + sprite.width) / w) * 2.0 - 1.0;
            let y1 = 1.0 - ((sprite.y + sprite.height) / h) * 2.0;
            let [u0, v0, u1, v1] = sprite.uv;

            let quad = [
                Vertex {
                    position: [x0, y0],
                    uv: [u0, v0],
                },
                Vertex {
                    position: [x1, y0],
                    uv: [u1, v0],
                },
                Vertex {
                    position: [x0, y1],
                    uv: [u0, v1],
                },
                Vertex {
                    position: [x1, y0],
                    uv: [u1, v0],
                },
                Vertex {
                    position: [x1, y1],
                    uv: [u1, v1],
                },
                Vertex {
                    position: [x0, y1],
                    uv: [u0, v1],
                },
            ];
            vertices.extend_from_slice(&quad);
        }

        if !vertices.is_empty() {
            let bytes = bytemuck::cast_slice(&vertices);
            if bytes.len() as u64 <= self.vertex_buffer.size() {
                self.queue.write_buffer(&self.vertex_buffer, 0, bytes);
            }
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame-encoder"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // Magenta key color for Win32 LWA_COLORKEY (must match platform::TRANSPARENT_COLOR_KEY).
                        // Solid opaque clear — do NOT use alpha=0 (Windows often shows white then).
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 1.0,
                            g: 0.0,
                            b: 1.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            if !vertices.is_empty() {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                pass.draw(0..(vertices.len() as u32), 0..1);
            }
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
        self.pending_sprites.clear();
        Ok(())
    }
}

fn create_sprite_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    bind_group_layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    rgba: &[u8],
    sprite_size: (u32, u32),
) -> Result<(wgpu::Texture, wgpu::BindGroup), AppError> {
    let texture_size = wgpu::Extent3d {
        width: sprite_size.0.max(1),
        height: sprite_size.1.max(1),
        depth_or_array_layers: 1,
    };

    let expected = (texture_size.width * texture_size.height * 4) as usize;
    if rgba.len() < expected {
        return Err(AppError::Render(format!(
            "sprite rgba too small: {} < {}",
            rgba.len(),
            expected
        )));
    }

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("pet-sprite"),
        size: texture_size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &rgba[..expected],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * texture_size.width),
            rows_per_image: Some(texture_size.height),
        },
        texture_size,
    );

    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("sprite-bg"),
        layout: bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });

    Ok((texture, bind_group))
}

impl Renderer for WgpuRenderer {
    fn draw_sprite(&mut self, sprite: SpriteDrawCommand) {
        self.pending_sprites.push(sprite);
    }

    fn draw_text(&mut self, _text: TextDrawCommand) {
        if !self.stub_logged {
            debug!("draw_text stub (M3+)");
            self.stub_logged = true;
        }
    }

    fn draw_panel(&mut self, _panel: PanelDrawCommand) {}

    fn draw_button(&mut self, _button: ButtonDrawCommand) {}
}

const SPRITE_SHADER: &str = r#"
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(in.position, 0.0, 1.0);
    out.uv = in.uv;
    return out;
}

@group(0) @binding(0)
var t_sprite: texture_2d<f32>;
@group(0) @binding(1)
var s_sprite: sampler;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(t_sprite, s_sprite, in.uv);
    // Win32 color-key mode: low-alpha texels become pure magenta so the OS punches
    // them out of the layered window. High-alpha texels are drawn fully opaque so
    // only the pet silhouette remains floating over the desktop.
    if color.a < 0.40 {
        return vec4<f32>(1.0, 0.0, 1.0, 1.0);
    }
    return vec4<f32>(color.rgb, 1.0);
}
"#;
