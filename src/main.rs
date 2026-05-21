use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes};

use clear_ui::color;
use winit::platform::wayland::WindowAttributesExtWayland;
use glyphon::{
    Attrs, Buffer, Cache, FontSystem, Metrics, Resolution, SwashCache, TextArea, TextAtlas,
    TextBounds, TextRenderer, Viewport,
};

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    color: [f32; 4],
}

impl Vertex {
    const ATTRIBS: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![
        0 => Float32x2,
        1 => Float32x4,
    ];

    fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

fn quad_vertices(x: f32, y: f32, w: f32, h: f32, sw: f32, sh: f32, c: [f32; 4]) -> [Vertex; 6] {
    let x0 = (x / sw) * 2.0 - 1.0;
    let y0 = 1.0 - (y / sh) * 2.0;
    let x1 = ((x + w) / sw) * 2.0 - 1.0;
    let y1 = 1.0 - ((y + h) / sh) * 2.0;
    [
        Vertex { position: [x0, y0], color: c },
        Vertex { position: [x1, y0], color: c },
        Vertex { position: [x0, y1], color: c },
        Vertex { position: [x1, y0], color: c },
        Vertex { position: [x1, y1], color: c },
        Vertex { position: [x0, y1], color: c },
    ]
}

fn make_text_buffer(fs: &mut FontSystem, text: &str, size: f32) -> Buffer {
    let metrics = Metrics::new(size, size * 1.4);
    let mut buf = Buffer::new(fs, metrics);
    buf.set_text(fs, text, Attrs::new(), glyphon::Shaping::Advanced);
    buf.shape_until_scroll(fs, true);
    buf
}

fn parse_hex(hex: &str) -> Option<(f32, f32, f32)> {
    let s = hex.trim_start_matches('#');
    if s.len() == 6 {
        u32::from_str_radix(s, 16).ok().map(|v| {
            let r = ((v >> 16) & 0xFF) as f32 / 255.0;
            let g = ((v >> 8) & 0xFF) as f32 / 255.0;
            let b = (v & 0xFF) as f32 / 255.0;
            (r, g, b)
        })
    } else {
        None
    }
}

const HEADER_H: f32 = 36.0;
const SLIDER_ROW_H: f32 = 36.0;
const SLIDER_START_Y: f32 = 48.0;
const SLIDER_LABEL_X: f32 = 12.0;
const SLIDER_TRACK_X: f32 = 32.0;
const SLIDER_TRACK_W: f32 = 280.0;
const SLIDER_TRACK_H: f32 = 20.0;
const SLIDER_VALUE_X: f32 = 320.0;
const PREVIEW_X: f32 = 12.0;
const PREVIEW_Y: f32 = 168.0;
const PREVIEW_W: f32 = 160.0;
const PREVIEW_H: f32 = 72.0;
const BUTTON_Y: f32 = 252.0;
const BUTTON_H: f32 = 32.0;
const BUTTON_W: f32 = 100.0;
const BUTTON_GAP: f32 = 12.0;
const WIN_W: f32 = 380.0;
const WIN_H: f32 = 320.0;

struct RectWidget {
    x: f32, y: f32, w: f32, h: f32,
    color: [f32; 4],
}

struct TextItem {
    buffer: Buffer,
    x: f32, y: f32,
    color: glyphon::Color,
}

#[derive(Clone, Copy, PartialEq)]
enum DragTarget { Red, Green, Blue }

#[derive(Clone, Copy, PartialEq)]
enum Action { Apply, Cancel }

struct HitButton {
    x: f32, y: f32, w: f32, h: f32,
    action: Action,
}

struct ColorApp {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    render_pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    vertex_count: u32,

    red: f32,
    green: f32,
    blue: f32,

    font_system: FontSystem,
    swash_cache: SwashCache,
    text_atlas: TextAtlas,
    text_renderer: TextRenderer,
    text_viewport: Viewport,

    rects: Vec<RectWidget>,
    text_items: Vec<TextItem>,
    action_buttons: Vec<HitButton>,

    cursor_x: f32,
    cursor_y: f32,
    dragging: Option<DragTarget>,
    action_requested: Option<Action>,

    scale_factor: f64,
    width: u32,
    height: u32,
    needs_rebuild: bool,
}

impl ColorApp {
    async fn new(window: Arc<Window>, red: f32, green: f32, blue: f32) -> Self {
        let size = window.inner_size();
        let sw = size.width as f32;
        let sh = size.height as f32;

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..Default::default()
        });
        let surface = instance.create_surface(window.clone()).expect("surface");
        let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }).await.expect("adapter");
        let (device, queue) = adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("GPU Device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
        }, None).await.expect("device");
        let config = surface.get_default_config(&adapter, size.width.max(1), size.height.max(1)).expect("config");
        surface.configure(&device, &config);

        let shader_code = clear_ui::SHADER;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(shader_code)),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Pipeline Layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });
        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Vertex::desc()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
                strip_index_format: None,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState { count: 1, mask: !0, alpha_to_coverage_enabled: false },
            multiview: None,
            cache: None,
        });

        let font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let cache = Cache::new(&device);
        let mut text_atlas = TextAtlas::new(&device, &queue, &cache, config.format);
        let text_renderer = TextRenderer::new(&mut text_atlas, &device, wgpu::MultisampleState::default(), None);
        let mut text_viewport = Viewport::new(&device, &cache);
        text_viewport.update(&queue, Resolution { width: size.width, height: size.height });

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Vertex Buffer"),
            size: 1,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let scale_factor = (window.scale_factor() as f32).max(2.0) as f64;

        let mut app = Self {
            window, surface, device, queue, config, render_pipeline,
            vertex_buffer, vertex_count: 0,
            red, green, blue,
            font_system, swash_cache, text_atlas, text_renderer, text_viewport,
            rects: Vec::new(), text_items: Vec::new(),
            action_buttons: Vec::new(),
            cursor_x: 0.0, cursor_y: 0.0,
            dragging: None,
            action_requested: None,
            scale_factor,
            width: size.width, height: size.height,
            needs_rebuild: true,
        };
        app.rebuild_layout(sw, sh);
        app
    }

    fn hex(&self) -> String {
        format!("#{:02X}{:02X}{:02X}",
            (self.red * 255.0) as u8,
            (self.green * 255.0) as u8,
            (self.blue * 255.0) as u8)
    }

    fn rebuild_layout(&mut self, sw: f32, sh: f32) {
        let s = self.scale_factor as f32;
        let mut rects = Vec::new();
        let mut text_items = Vec::new();
        let mut action_buttons = Vec::new();

        rects.push(RectWidget {
            x: 0.0, y: 0.0, w: sw, h: HEADER_H * s,
            color: color::HEADER_BG,
        });
        text_items.push(TextItem {
            buffer: make_text_buffer(&mut self.font_system, "Clear Colors", 14.0 * s),
            x: 12.0 * s, y: 10.0 * s,
            color: glyphon::Color::rgb(0xcc, 0xcc, 0xd4),
        });

        rects.push(RectWidget {
            x: 0.0, y: HEADER_H * s, w: sw, h: sh - HEADER_H * s,
            color: color::CONTENT_BG,
        });

        let channels = [self.red, self.green, self.blue];
        let labels = ['R', 'G', 'B'];
        let fill_colors = [
            [1.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0, 1.0],
            [0.0, 0.0, 1.0, 1.0],
        ];

        for i in 0..3 {
            let row_y = (SLIDER_START_Y + i as f32 * SLIDER_ROW_H) * s;
            let track_y = row_y + ((SLIDER_ROW_H - SLIDER_TRACK_H) / 2.0) * s;

            rects.push(RectWidget {
                x: SLIDER_TRACK_X * s, y: track_y,
                w: SLIDER_TRACK_W * s, h: SLIDER_TRACK_H * s,
                color: [0.18, 0.18, 0.22, 1.0],
            });

            let fill_w = SLIDER_TRACK_W * channels[i] * s;
            if fill_w > 0.0 {
                rects.push(RectWidget {
                    x: SLIDER_TRACK_X * s, y: track_y,
                    w: fill_w, h: SLIDER_TRACK_H * s,
                    color: fill_colors[i],
                });
            }

            text_items.push(TextItem {
                buffer: make_text_buffer(&mut self.font_system, &labels[i].to_string(), 12.0 * s),
                x: SLIDER_LABEL_X * s, y: row_y + 4.0 * s,
                color: glyphon::Color::rgb(0xaa, 0xaa, 0xbb),
            });

            let val = format!("{}", (channels[i] * 255.0) as u8);
            text_items.push(TextItem {
                buffer: make_text_buffer(&mut self.font_system, &val, 11.0 * s),
                x: SLIDER_VALUE_X * s, y: row_y + 4.0 * s,
                color: glyphon::Color::rgb(0xcc, 0xcc, 0xdd),
            });
        }

        rects.push(RectWidget {
            x: PREVIEW_X * s, y: PREVIEW_Y * s,
            w: PREVIEW_W * s, h: PREVIEW_H * s,
            color: [self.red, self.green, self.blue, 1.0],
        });

        let hex = self.hex();
        text_items.push(TextItem {
            buffer: make_text_buffer(&mut self.font_system, &hex, 16.0 * s),
            x: (PREVIEW_X + PREVIEW_W + 16.0) * s,
            y: (PREVIEW_Y + 26.0) * s,
            color: glyphon::Color::rgb(0xe0, 0xe0, 0xe8),
        });

        // Apply button
        let apply_x = PREVIEW_X;
        let cancel_x = PREVIEW_X + BUTTON_W + BUTTON_GAP;
        let btn_y = BUTTON_Y;
        let btn_bg = [0.20, 0.40, 0.65, 1.0];
        let cancel_bg = [0.40, 0.20, 0.20, 1.0];

        rects.push(RectWidget {
            x: apply_x * s, y: btn_y * s,
            w: BUTTON_W * s, h: BUTTON_H * s,
            color: btn_bg,
        });
        text_items.push(TextItem {
            buffer: make_text_buffer(&mut self.font_system, "Apply", 12.0 * s),
            x: (apply_x + 28.0) * s, y: (btn_y + 8.0) * s,
            color: glyphon::Color::rgb(0xee, 0xee, 0xf0),
        });
        action_buttons.push(HitButton {
            x: apply_x * s, y: btn_y * s,
            w: BUTTON_W * s, h: BUTTON_H * s,
            action: Action::Apply,
        });

        rects.push(RectWidget {
            x: cancel_x * s, y: btn_y * s,
            w: BUTTON_W * s, h: BUTTON_H * s,
            color: cancel_bg,
        });
        text_items.push(TextItem {
            buffer: make_text_buffer(&mut self.font_system, "Cancel", 12.0 * s),
            x: (cancel_x + 22.0) * s, y: (btn_y + 8.0) * s,
            color: glyphon::Color::rgb(0xee, 0xee, 0xf0),
        });
        action_buttons.push(HitButton {
            x: cancel_x * s, y: btn_y * s,
            w: BUTTON_W * s, h: BUTTON_H * s,
            action: Action::Cancel,
        });

        self.rects = rects;
        self.text_items = text_items;
        self.action_buttons = action_buttons;
        self.needs_rebuild = false;
    }

    fn slider_physical_rect(i: usize, s: f32) -> (f32, f32, f32, f32) {
        let row_y = (SLIDER_START_Y + i as f32 * SLIDER_ROW_H) * s;
        let track_y = row_y + ((SLIDER_ROW_H - SLIDER_TRACK_H) / 2.0) * s;
        (SLIDER_TRACK_X * s, track_y, SLIDER_TRACK_W * s, SLIDER_TRACK_H * s)
    }

    fn collect_vertices(&self) -> Vec<Vertex> {
        let sw = self.width as f32;
        let sh = self.height as f32;
        let mut verts = Vec::new();
        for r in &self.rects {
            verts.extend(quad_vertices(r.x, r.y, r.w, r.h, sw, sh, r.color));
        }
        verts
    }

    fn upload_vertices(&mut self) {
        let verts = self.collect_vertices();
        self.vertex_count = verts.len() as u32;
        let data = bytemuck::cast_slice(&verts);
        let needed = data.len() as wgpu::BufferAddress;
        if needed > self.vertex_buffer.size() {
            self.vertex_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Vertex Buffer"),
                size: needed,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        self.queue.write_buffer(&self.vertex_buffer, 0, data);
    }

    fn prepare_text(&mut self) {
        let Self {
            ref mut text_renderer, ref device, ref queue,
            ref mut font_system, ref mut text_atlas,
            ref mut text_viewport, ref mut swash_cache,
            ref text_items, width, height, ..
        } = self;

        let w = *width as f32;
        let h = *height as f32;
        let viewport = Resolution { width: w as u32, height: h as u32 };
        text_viewport.update(queue, viewport);
        let bounds = TextBounds { left: 0, top: 0, right: w as i32, bottom: h as i32 };
        let areas: Vec<TextArea> = text_items.iter().map(|ti| TextArea {
            buffer: &ti.buffer,
            left: ti.x, top: ti.y, scale: 1.0, bounds,
            default_color: ti.color,
            custom_glyphs: &[],
        }).collect();
        text_renderer.prepare(device, queue, font_system, text_atlas, text_viewport, areas, swash_cache).unwrap();
    }

    fn resize(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        if size.width > 0 && size.height > 0 {
            self.width = size.width; self.height = size.height;
            self.config.width = size.width; self.config.height = size.height;
            self.surface.configure(&self.device, &self.config);
            self.needs_rebuild = true;
        }
    }

    fn handle_event(&mut self, event: &WindowEvent) -> bool {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_x = position.x as f32;
                self.cursor_y = position.y as f32;
                if let Some(drag) = self.dragging {
                    let i = match drag {
                        DragTarget::Red => 0, DragTarget::Green => 1, DragTarget::Blue => 2,
                    };
                    let s = self.scale_factor as f32;
                    let (tx, _, tw, _) = Self::slider_physical_rect(i, s);
                    let new_val = ((self.cursor_x - tx) / tw).clamp(0.0, 1.0);
                    let old = match i { 0 => self.red, 1 => self.green, _ => self.blue };
                    if (new_val - old).abs() > 0.002 {
                        match i { 0 => self.red = new_val, 1 => self.green = new_val, _ => self.blue = new_val };
                        self.needs_rebuild = true;
                    }
                }
                false
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if *button != MouseButton::Left { return false; }
                match state {
                    ElementState::Pressed => {
                        let s = self.scale_factor as f32;
                        let (px, py) = (self.cursor_x, self.cursor_y);
                        for i in 0..3 {
                            let (tx, ty, tw, th) = Self::slider_physical_rect(i, s);
                            if px >= tx && px <= tx + tw && py >= ty && py <= ty + th {
                                let val = ((px - tx) / tw).clamp(0.0, 1.0);
                                match i { 0 => self.red = val, 1 => self.green = val, _ => self.blue = val };
                                self.dragging = Some(match i {
                                    0 => DragTarget::Red, 1 => DragTarget::Green, _ => DragTarget::Blue,
                                });
                                self.needs_rebuild = true;
                                return true;
                            }
                        }
                        for btn in &self.action_buttons {
                            if px >= btn.x && px <= btn.x + btn.w && py >= btn.y && py <= btn.y + btn.h {
                                self.action_requested = Some(btn.action);
                                self.needs_rebuild = true;
                                return true;
                            }
                        }
                    }
                    ElementState::Released => {
                        if self.dragging.is_some() {
                            self.dragging = None;
                            return true;
                        }
                    }
                }
                false
            }
            _ => false,
        }
    }

    fn render(&mut self) {
        let sw = self.width as f32;
        let sh = self.height as f32;

        if self.needs_rebuild {
            self.rebuild_layout(sw, sh);
            self.upload_vertices();
        }

        self.prepare_text();

        let output = match self.surface.get_current_texture() {
            Ok(t) => t,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                return;
            }
            Err(wgpu::SurfaceError::Timeout) => return,
            Err(e) => { eprintln!("Surface error: {e:?}"); return; }
        };

        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Encoder"),
        });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.06, g: 0.06, b: 0.08, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            pass.set_pipeline(&self.render_pipeline);
            pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            pass.draw(0..self.vertex_count, 0..1);

            self.text_renderer.render(&self.text_atlas, &self.text_viewport, &mut pass).unwrap();
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        self.window.pre_present_notify();
        output.present();
    }
}

struct AppWrapper {
    state: Option<ColorApp>,
    initial_red: f32,
    initial_green: f32,
    initial_blue: f32,
}

impl AppWrapper {
    fn new(red: f32, green: f32, blue: f32) -> Self {
        Self { state: None, initial_red: red, initial_green: green, initial_blue: blue }
    }
}

impl ApplicationHandler for AppWrapper {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() { return; }
        let window = Arc::new(event_loop.create_window(
            WindowAttributes::default()
                .with_name("clear-colors", "clear-colors")
                .with_title("Clear Colors")
                .with_inner_size(winit::dpi::LogicalSize::new(WIN_W, WIN_H)),
        ).unwrap());
        let state = pollster::block_on(ColorApp::new(
            window, self.initial_red, self.initial_green, self.initial_blue,
        ));
        self.state = Some(state);
        self.state.as_ref().unwrap().window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: winit::window::WindowId, event: WindowEvent) {
        let redraw = match &event {
            WindowEvent::CloseRequested => { event_loop.exit(); true }
            WindowEvent::Resized(s) => { if let Some(st) = &mut self.state { st.resize(*s); } true }
            WindowEvent::RedrawRequested => {
                if let Some(st) = &mut self.state { st.render(); st.window.request_redraw(); }
                true
            }
            _ => self.state.as_mut().map(|st| st.handle_event(&event)).unwrap_or(false)
        };
        if redraw { if let Some(st) = &mut self.state { st.window.request_redraw(); } }
        if let Some(st) = &self.state {
            if let Some(action) = st.action_requested {
                match action {
                    Action::Apply => { println!("{}", st.hex()); }
                    Action::Cancel => {}
                }
                event_loop.exit();
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (r, g, b) = if args.len() > 1 {
        parse_hex(&args[1]).unwrap_or((0.5, 0.5, 0.5))
    } else {
        (0.5, 0.5, 0.5)
    };

    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);
    event_loop.run_app(&mut AppWrapper::new(r, g, b)).unwrap();
}
