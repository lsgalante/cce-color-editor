use std::sync::Arc;
use glyphon::{
    Attrs, Buffer, Cache, FontSystem, Metrics, Resolution, SwashCache, TextArea, TextAtlas,
    TextBounds, TextRenderer, Viewport,
};
use clear_ui::color;

use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_keyboard, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm, delegate_xdg_shell, delegate_xdg_window, delegate_output,
    registry::{ProvidesRegistryState, RegistryState},
    output::{OutputHandler, OutputState},
    seat::{
        keyboard::KeyboardHandler,
        pointer::PointerHandler,
        Capability, SeatHandler, SeatState,
    },
    shell::{
        xdg::{
            window::{Window as XdgWindow, WindowConfigure, WindowHandler, WindowDecorations},
            XdgShell,
        },
        WaylandSurface,
    },
    shm::{Shm, ShmHandler},
};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
    Connection, QueueHandle, Proxy,
};
use calloop::EventLoop;
use calloop_wayland_source::WaylandSource;

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    color: [f32; 4],
    clip_circle: [f32; 3],
}

impl Vertex {
    const ATTRIBS: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![
        0 => Float32x2,
        1 => Float32x4,
        2 => Float32x3,
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
        Vertex { position: [x0, y0], color: c, clip_circle: [0.0, 0.0, 0.0] },
        Vertex { position: [x1, y0], color: c, clip_circle: [0.0, 0.0, 0.0] },
        Vertex { position: [x0, y1], color: c, clip_circle: [0.0, 0.0, 0.0] },
        Vertex { position: [x1, y0], color: c, clip_circle: [0.0, 0.0, 0.0] },
        Vertex { position: [x1, y1], color: c, clip_circle: [0.0, 0.0, 0.0] },
        Vertex { position: [x0, y1], color: c, clip_circle: [0.0, 0.0, 0.0] },
    ]
}

fn gradient_quad_vertices(x: f32, y: f32, w: f32, h: f32, sw: f32, sh: f32, c0: [f32; 4], c1: [f32; 4]) -> [Vertex; 6] {
    let x0 = (x / sw) * 2.0 - 1.0;
    let y0 = 1.0 - (y / sh) * 2.0;
    let x1 = ((x + w) / sw) * 2.0 - 1.0;
    let y1 = 1.0 - ((y + h) / sh) * 2.0;
    [
        Vertex { position: [x0, y0], color: c0, clip_circle: [0.0, 0.0, 0.0] },
        Vertex { position: [x1, y0], color: c1, clip_circle: [0.0, 0.0, 0.0] },
        Vertex { position: [x0, y1], color: c0, clip_circle: [0.0, 0.0, 0.0] },
        Vertex { position: [x1, y0], color: c1, clip_circle: [0.0, 0.0, 0.0] },
        Vertex { position: [x1, y1], color: c1, clip_circle: [0.0, 0.0, 0.0] },
        Vertex { position: [x0, y1], color: c0, clip_circle: [0.0, 0.0, 0.0] },
    ]
}

fn make_text_buffer(fs: &mut FontSystem, text: &str, size: f32) -> Buffer {
    let metrics = Metrics::new(size, size * 1.4);
    let mut buf = Buffer::new(fs, metrics);
    buf.set_text(fs, text, Attrs::new(), glyphon::Shaping::Advanced);
    buf.shape_until_scroll(fs, true);
    buf
}

fn parse_hex(hex: &str) -> Option<(f32, f32, f32, Option<f32>)> {
    let s = hex.trim_start_matches('#');
    if s.len() == 6 {
        u32::from_str_radix(s, 16).ok().map(|v| {
            let r = ((v >> 16) & 0xFF) as f32 / 255.0;
            let g = ((v >> 8) & 0xFF) as f32 / 255.0;
            let b = (v & 0xFF) as f32 / 255.0;
            (r, g, b, None)
        })
    } else if s.len() == 8 {
        u32::from_str_radix(s, 16).ok().map(|v| {
            let r = ((v >> 24) & 0xFF) as f32 / 255.0;
            let g = ((v >> 16) & 0xFF) as f32 / 255.0;
            let b = ((v >> 8) & 0xFF) as f32 / 255.0;
            let a = (v & 0xFF) as f32 / 255.0;
            (r, g, b, Some(a))
        })
    } else {
        None
    }
}

fn rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g.max(b));
    let min = r.min(g.min(b));
    let mut h = 0.0;
    let mut s = 0.0;
    let l = (max + min) / 2.0;

    if max != min {
        let d = max - min;
        s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
        if max == r {
            h = (g - b) / d + (if g < b { 6.0 } else { 0.0 });
        } else if max == g {
            h = (b - r) / d + 2.0;
        } else if max == b {
            h = (r - g) / d + 4.0;
        }
        h /= 6.0;
    }

    (h, s, l)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    if s == 0.0 {
        return (l, l, l);
    }

    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;

    let r = hue_to_rgb(p, q, h + 1.0 / 3.0);
    let g = hue_to_rgb(p, q, h);
    let b = hue_to_rgb(p, q, h - 1.0 / 3.0);

    (r, g, b)
}

fn hue_to_rgb(p: f32, q: f32, mut t: f32) -> f32 {
    if t < 0.0 { t += 1.0; }
    if t > 1.0 { t -= 1.0; }
    if t < 1.0 / 6.0 { return p + (q - p) * 6.0 * t; }
    if t < 1.0 / 2.0 { return q; }
    if t < 2.0 / 3.0 { return p + (q - p) * (2.0 / 3.0 - t) * 6.0; }
    p
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
const PREVIEW_Y: f32 = 276.0;
const PREVIEW_W: f32 = 160.0;
const PREVIEW_H: f32 = 72.0;
const BUTTON_Y: f32 = 360.0;
const BUTTON_H: f32 = 32.0;
const BUTTON_W: f32 = 100.0;
const BUTTON_GAP: f32 = 12.0;
const WIN_W: f32 = 380.0;
const WIN_H: f32 = 428.0;

struct RectWidget {
    x: f32, y: f32, w: f32, h: f32,
    color: [f32; 4],
}

struct GradientRectWidget {
    x: f32, y: f32, w: f32, h: f32,
    c0: [f32; 4],
    c1: [f32; 4],
}

struct TextItem {
    buffer: Buffer,
    x: f32, y: f32,
    color: glyphon::Color,
}

#[derive(Clone, Copy, PartialEq)]
enum DragTarget { Red, Green, Blue, Hue, Saturation, Lightness, Alpha }

#[derive(Clone, Copy, PartialEq)]
enum Action { Apply, Cancel }

struct HitButton {
    x: f32, y: f32, w: f32, h: f32,
    action: Action,
}

struct ColorApp {
    window: XdgWindow,
    surface: wl_surface::WlSurface,
    wgpu_surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    render_pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    vertex_count: u32,

    red: f32,
    green: f32,
    blue: f32,
    hue: f32,
    saturation: f32,
    lightness: f32,
    alpha: f32,
    with_alpha: bool,

    font_system: FontSystem,
    swash_cache: SwashCache,
    text_atlas: TextAtlas,
    text_renderer: TextRenderer,
    text_viewport: Viewport,

    rects: Vec<RectWidget>,
    gradient_rects: Vec<GradientRectWidget>,
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
    async fn new(
        conn: &Connection,
        qh: &QueueHandle<AppState>,
        compositor_state: &CompositorState,
        xdg_shell_state: &XdgShell,
        red: f32,
        green: f32,
        blue: f32,
        alpha: f32,
        with_alpha: bool,
        scale: f64,
    ) -> Self {
        let surface = compositor_state.create_surface(qh);
        surface.set_buffer_scale(scale as i32);
        let window = xdg_shell_state.create_window(surface.clone(), WindowDecorations::None, qh);
        window.set_title("Clear Color Interface");
        window.set_app_id("cce-color-interface");
        let win_h = if with_alpha { WIN_H + SLIDER_ROW_H } else { WIN_H };
        window.set_min_size(Some((WIN_W as u32, win_h as u32)));
        window.commit();

        let wayland_handle = Box::leak(Box::new(clear_ui::wayland::WaylandSurfaceHandle {
            display_ptr: conn.backend().display_id().as_ptr() as *mut std::ffi::c_void,
            surface_ptr: surface.id().as_ptr() as *mut std::ffi::c_void,
        }));

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..Default::default()
        });
        let wgpu_surface = instance.create_surface(wayland_handle).expect("surface");
        let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&wgpu_surface),
            force_fallback_adapter: false,
        }).await.expect("adapter");
        let (device, queue) = adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("GPU Device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
            memory_hints: wgpu::MemoryHints::MemoryUsage,
        }, None).await.expect("device");

        let scale_factor = scale;
        let width = (WIN_W * scale_factor as f32) as u32;
        let height = (win_h * scale_factor as f32) as u32;

        let mut config = wgpu_surface.get_default_config(&adapter, width, height).expect("config");
        config.width = width;
        config.height = height;
        wgpu_surface.configure(&device, &config);

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
        text_viewport.update(&queue, Resolution { width, height });

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Vertex Buffer"),
            size: 1,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let (hue, saturation, lightness) = rgb_to_hsl(red, green, blue);
        let mut app = Self {
            window, surface, wgpu_surface, device, queue, config, render_pipeline,
            vertex_buffer, vertex_count: 0,
            red, green, blue,
            hue, saturation, lightness,
            alpha,
            with_alpha,
            font_system, swash_cache, text_atlas, text_renderer, text_viewport,
            rects: Vec::new(),
            gradient_rects: Vec::new(),
            text_items: Vec::new(),
            action_buttons: Vec::new(),
            cursor_x: 0.0, cursor_y: 0.0,
            dragging: None,
            action_requested: None,
            scale_factor,
            width, height,
            needs_rebuild: true,
        };
        app.rebuild_layout(width as f32, height as f32);
        app
    }

    fn hex(&self) -> String {
        if self.with_alpha {
            format!("#{:02X}{:02X}{:02X}{:02X}",
                (self.red * 255.0) as u8,
                (self.green * 255.0) as u8,
                (self.blue * 255.0) as u8,
                (self.alpha * 255.0) as u8)
        } else {
            format!("#{:02X}{:02X}{:02X}",
                (self.red * 255.0) as u8,
                (self.green * 255.0) as u8,
                (self.blue * 255.0) as u8)
        }
    }

    fn rebuild_layout(&mut self, sw: f32, sh: f32) {
        let s = self.scale_factor as f32;
        let mut rects = Vec::new();
        let mut gradient_rects = Vec::new();
        let mut text_items = Vec::new();
        let mut action_buttons = Vec::new();

        rects.push(RectWidget {
            x: 0.0, y: 0.0, w: sw, h: HEADER_H * s,
            color: color::HEADER_BG,
        });
        text_items.push(TextItem {
            buffer: make_text_buffer(&mut self.font_system, "Clear Color Interface", 14.0 * s),
            x: 12.0 * s, y: 10.0 * s,
            color: glyphon::Color::rgb(0xcc, 0xcc, 0xd4),
        });

        rects.push(RectWidget {
            x: 0.0, y: HEADER_H * s, w: sw, h: sh - HEADER_H * s,
            color: color::CONTENT_BG,
        });

        let mut channels = vec![self.red, self.green, self.blue, self.hue, self.saturation, self.lightness];
        let mut labels = vec!['R', 'G', 'B', 'H', 'S', 'L'];
        if self.with_alpha {
            channels.push(self.alpha);
            labels.push('A');
        }

        let red = self.red;
        let green = self.green;
        let blue = self.blue;
        let hue = self.hue;
        let saturation = self.saturation;
        let lightness = self.lightness;
        let alpha = self.alpha;

        let get_color_at = |i: usize, t: f32| -> [f32; 4] {
            match i {
                0 => [t, green, blue, alpha],
                1 => [red, t, blue, alpha],
                2 => [red, green, t, alpha],
                3 => {
                    let (r, g, b) = hsl_to_rgb(t, saturation, lightness);
                    [r, g, b, alpha]
                }
                4 => {
                    let (r, g, b) = hsl_to_rgb(hue, t, lightness);
                    [r, g, b, alpha]
                }
                5 => {
                    let (r, g, b) = hsl_to_rgb(hue, saturation, t);
                    [r, g, b, alpha]
                }
                _ => [red, green, blue, t],
            }
        };

        for i in 0..channels.len() {
            let row_y = (SLIDER_START_Y + i as f32 * SLIDER_ROW_H) * s;
            let track_y = row_y + ((SLIDER_ROW_H - SLIDER_TRACK_H) / 2.0) * s;

            // Draw track border
            rects.push(RectWidget {
                x: (SLIDER_TRACK_X - 1.0) * s,
                y: track_y - 1.0 * s,
                w: (SLIDER_TRACK_W + 2.0) * s,
                h: (SLIDER_TRACK_H + 2.0) * s,
                color: [0.08, 0.08, 0.10, 1.0],
            });

            if i == 6 {
                // Draw checkerboard behind the alpha slider track
                let track_x = SLIDER_TRACK_X * s;
                let track_w = SLIDER_TRACK_W * s;
                let track_h = SLIDER_TRACK_H * s;
                let grid_size = track_h / 2.0; // Two rows of checkers
                let cols = (track_w / grid_size).ceil() as i32;
                
                // First draw a solid light gray background
                rects.push(RectWidget {
                    x: track_x,
                    y: track_y,
                    w: track_w,
                    h: track_h,
                    color: [0.8, 0.8, 0.8, 1.0],
                });
                
                for r in 0..2 {
                    for c in 0..cols {
                        if (r + c) % 2 == 1 {
                            let qx = track_x + c as f32 * grid_size;
                            let qy = track_y + r as f32 * grid_size;
                            let qw = grid_size.min(track_x + track_w - qx);
                            let qh = grid_size.min(track_y + track_h - qy);
                            if qw > 0.0 && qh > 0.0 {
                                rects.push(RectWidget {
                                    x: qx,
                                    y: qy,
                                    w: qw,
                                    h: qh,
                                    color: [1.0, 1.0, 1.0, 1.0],
                                });
                            }
                        }
                    }
                }
            }

            // Draw gradient track
            let n_segments = if i == 3 { 30 } else { 10 };
            for j in 0..n_segments {
                let t0 = j as f32 / n_segments as f32;
                let t1 = (j + 1) as f32 / n_segments as f32;
                let c0 = color::to_linear(get_color_at(i, t0));
                let c1 = color::to_linear(get_color_at(i, t1));
                gradient_rects.push(GradientRectWidget {
                    x: (SLIDER_TRACK_X + t0 * SLIDER_TRACK_W) * s,
                    y: track_y,
                    w: ((t1 - t0) * SLIDER_TRACK_W) * s,
                    h: SLIDER_TRACK_H * s,
                    c0,
                    c1,
                });
            }

            // Draw vertical bar indicator at current position
            let indicator_w = 4.0;
            let indicator_h = SLIDER_TRACK_H + 4.0;
            let indicator_x = SLIDER_TRACK_X + channels[i] * SLIDER_TRACK_W - indicator_w / 2.0;
            let indicator_y = (SLIDER_START_Y + i as f32 * SLIDER_ROW_H) + ((SLIDER_ROW_H - indicator_h) / 2.0);

            // Indicator border/shadow
            rects.push(RectWidget {
                x: (indicator_x - 1.0) * s,
                y: (indicator_y - 1.0) * s,
                w: (indicator_w + 2.0) * s,
                h: (indicator_h + 2.0) * s,
                color: [0.05, 0.05, 0.05, 0.95],
            });

            // Indicator body
            rects.push(RectWidget {
                x: indicator_x * s,
                y: indicator_y * s,
                w: indicator_w * s,
                h: indicator_h * s,
                color: [1.0, 1.0, 1.0, 1.0],
            });

            text_items.push(TextItem {
                buffer: make_text_buffer(&mut self.font_system, &labels[i].to_string(), 12.0 * s),
                x: SLIDER_LABEL_X * s, y: row_y + 4.0 * s,
                color: glyphon::Color::rgb(0xaa, 0xaa, 0xbb),
            });

            let val = if i < 3 {
                format!("{}", (channels[i] * 255.0) as u8)
            } else if i == 3 {
                format!("{}°", (channels[i] * 360.0).round() as u16)
            } else if i < 6 {
                format!("{}%", (channels[i] * 100.0).round() as u8)
            } else {
                format!("{}", (channels[i] * 255.0).round() as u8)
            };
            text_items.push(TextItem {
                buffer: make_text_buffer(&mut self.font_system, &val, 11.0 * s),
                x: SLIDER_VALUE_X * s, y: row_y + 4.0 * s,
                color: glyphon::Color::rgb(0xcc, 0xcc, 0xdd),
            });
        }

        let preview_y_offset = if self.with_alpha { SLIDER_ROW_H } else { 0.0 };
        let preview_y = PREVIEW_Y + preview_y_offset;
        let button_y = BUTTON_Y + preview_y_offset;

        if self.with_alpha {
            // Draw checkerboard behind the preview box
            let px = PREVIEW_X * s;
            let py = preview_y * s;
            let pw = PREVIEW_W * s;
            let ph = PREVIEW_H * s;
            
            // Draw base light gray
            rects.push(RectWidget {
                x: px, y: py, w: pw, h: ph,
                color: [0.8, 0.8, 0.8, 1.0],
            });
            
            let grid_size = 12.0 * s;
            let cols = (pw / grid_size).ceil() as i32;
            let rows = (ph / grid_size).ceil() as i32;
            for r in 0..rows {
                for c in 0..cols {
                    if (r + c) % 2 == 1 {
                        let qx = px + c as f32 * grid_size;
                        let qy = py + r as f32 * grid_size;
                        let qw = grid_size.min(px + pw - qx);
                        let qh = grid_size.min(py + ph - qy);
                        if qw > 0.0 && qh > 0.0 {
                            rects.push(RectWidget {
                                x: qx, y: qy, w: qw, h: qh,
                                color: [1.0, 1.0, 1.0, 1.0],
                            });
                        }
                    }
                }
            }
        }

        rects.push(RectWidget {
            x: PREVIEW_X * s, y: preview_y * s,
            w: PREVIEW_W * s, h: PREVIEW_H * s,
            color: color::to_linear([self.red, self.green, self.blue, if self.with_alpha { self.alpha } else { 1.0 }]),
        });

        let hex = self.hex();
        text_items.push(TextItem {
            buffer: make_text_buffer(&mut self.font_system, &hex, 16.0 * s),
            x: (PREVIEW_X + PREVIEW_W + 16.0) * s,
            y: (preview_y + 26.0) * s,
            color: glyphon::Color::rgb(0xe0, 0xe0, 0xe8),
        });

        // Apply button
        let apply_x = PREVIEW_X;
        let cancel_x = PREVIEW_X + BUTTON_W + BUTTON_GAP;
        let btn_y = button_y;
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
        self.gradient_rects = gradient_rects;
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
        for g in &self.gradient_rects {
            verts.extend(gradient_quad_vertices(g.x, g.y, g.w, g.h, sw, sh, g.c0, g.c1));
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
            left: ti.x.round(), top: ti.y.round(), scale: 1.0, bounds,
            default_color: ti.color,
            custom_glyphs: &[],
        }).collect();
        text_renderer.prepare(device, queue, font_system, text_atlas, text_viewport, areas, swash_cache).unwrap();
    }

    fn handle_cursor_moved(&mut self, cx: f32, cy: f32) {
        self.cursor_x = cx;
        self.cursor_y = cy;
        if let Some(drag) = self.dragging {
            let i = match drag {
                DragTarget::Red => 0,
                DragTarget::Green => 1,
                DragTarget::Blue => 2,
                DragTarget::Hue => 3,
                DragTarget::Saturation => 4,
                DragTarget::Lightness => 5,
                DragTarget::Alpha => 6,
            };
            let s = self.scale_factor as f32;
            let (tx, _, tw, _) = Self::slider_physical_rect(i, s);
            let new_val = ((self.cursor_x - tx) / tw).clamp(0.0, 1.0);
            let old = match i {
                0 => self.red,
                1 => self.green,
                2 => self.blue,
                3 => self.hue,
                4 => self.saturation,
                5 => self.lightness,
                _ => self.alpha,
            };
            if (new_val - old).abs() > 0.002 {
                match i {
                    0 => {
                        self.red = new_val;
                        let (h, sat, l) = rgb_to_hsl(self.red, self.green, self.blue);
                        self.saturation = sat;
                        self.lightness = l;
                        if sat > 0.001 && l > 0.001 && l < 0.999 {
                            self.hue = h;
                        }
                    }
                    1 => {
                        self.green = new_val;
                        let (h, sat, l) = rgb_to_hsl(self.red, self.green, self.blue);
                        self.saturation = sat;
                        self.lightness = l;
                        if sat > 0.001 && l > 0.001 && l < 0.999 {
                            self.hue = h;
                        }
                    }
                    2 => {
                        self.blue = new_val;
                        let (h, sat, l) = rgb_to_hsl(self.red, self.green, self.blue);
                        self.saturation = sat;
                        self.lightness = l;
                        if sat > 0.001 && l > 0.001 && l < 0.999 {
                            self.hue = h;
                        }
                    }
                    3 => {
                        self.hue = new_val;
                        let (r, g, b) = hsl_to_rgb(self.hue, self.saturation, self.lightness);
                        self.red = r;
                        self.green = g;
                        self.blue = b;
                    }
                    4 => {
                        self.saturation = new_val;
                        let (r, g, b) = hsl_to_rgb(self.hue, self.saturation, self.lightness);
                        self.red = r;
                        self.green = g;
                        self.blue = b;
                    }
                    5 => {
                        self.lightness = new_val;
                        let (r, g, b) = hsl_to_rgb(self.hue, self.saturation, self.lightness);
                        self.red = r;
                        self.green = g;
                        self.blue = b;
                    }
                    _ => {
                        self.alpha = new_val;
                    }
                }
                self.needs_rebuild = true;
            }
        }
    }

    fn handle_mouse_input(&mut self, state: clear_ui::widget::ElementState) {
        match state {
            clear_ui::widget::ElementState::Pressed => {
                let s = self.scale_factor as f32;
                let (px, py) = (self.cursor_x, self.cursor_y);
                let num_sliders = if self.with_alpha { 7 } else { 6 };
                for i in 0..num_sliders {
                    let (tx, ty, tw, th) = Self::slider_physical_rect(i, s);
                    if px >= tx && px <= tx + tw && py >= ty && py <= ty + th {
                        let val = ((px - tx) / tw).clamp(0.0, 1.0);
                        match i {
                            0 => {
                                self.red = val;
                                let (h, sat, l) = rgb_to_hsl(self.red, self.green, self.blue);
                                self.saturation = sat;
                                self.lightness = l;
                                if sat > 0.001 && l > 0.001 && l < 0.999 {
                                    self.hue = h;
                                }
                                self.dragging = Some(DragTarget::Red);
                            }
                            1 => {
                                self.green = val;
                                let (h, sat, l) = rgb_to_hsl(self.red, self.green, self.blue);
                                self.saturation = sat;
                                self.lightness = l;
                                if sat > 0.001 && l > 0.001 && l < 0.999 {
                                    self.hue = h;
                                }
                                self.dragging = Some(DragTarget::Green);
                            }
                            2 => {
                                self.blue = val;
                                let (h, sat, l) = rgb_to_hsl(self.red, self.green, self.blue);
                                self.saturation = sat;
                                self.lightness = l;
                                if sat > 0.001 && l > 0.001 && l < 0.999 {
                                    self.hue = h;
                                }
                                self.dragging = Some(DragTarget::Blue);
                            }
                            3 => {
                                self.hue = val;
                                let (r, g, b) = hsl_to_rgb(self.hue, self.saturation, self.lightness);
                                self.red = r;
                                self.green = g;
                                self.blue = b;
                                self.dragging = Some(DragTarget::Hue);
                            }
                            4 => {
                                self.saturation = val;
                                let (r, g, b) = hsl_to_rgb(self.hue, self.saturation, self.lightness);
                                self.red = r;
                                self.green = g;
                                self.blue = b;
                                self.dragging = Some(DragTarget::Saturation);
                            }
                            5 => {
                                self.lightness = val;
                                let (r, g, b) = hsl_to_rgb(self.hue, self.saturation, self.lightness);
                                self.red = r;
                                self.green = g;
                                self.blue = b;
                                self.dragging = Some(DragTarget::Lightness);
                            }
                            _ => {
                                self.alpha = val;
                                self.dragging = Some(DragTarget::Alpha);
                            }
                        }
                        self.needs_rebuild = true;
                        return;
                    }
                }
                for btn in &self.action_buttons {
                    if px >= btn.x && px <= btn.x + btn.w && py >= btn.y && py <= btn.y + btn.h {
                        self.action_requested = Some(btn.action);
                        self.needs_rebuild = true;
                        return;
                    }
                }
            }
            clear_ui::widget::ElementState::Released => {
                if self.dragging.is_some() {
                    self.dragging = None;
                }
            }
        }
    }

    fn handle_scroll(&mut self, _scroll_amount_x: f32, scroll_amount_y: f32) {
        let s = self.scale_factor as f32;
        let (px, py) = (self.cursor_x, self.cursor_y);
        let scroll_amount = scroll_amount_y;
        if scroll_amount.abs() > 0.0001 {
            let num_sliders = if self.with_alpha { 7 } else { 6 };
            for i in 0..num_sliders {
                let (tx, ty, tw, th) = Self::slider_physical_rect(i, s);
                if px >= tx && px <= tx + tw && py >= ty - 4.0 * s && py <= ty + th + 4.0 * s {
                    let step = 0.02;
                    let old_val = match i {
                        0 => self.red,
                        1 => self.green,
                        2 => self.blue,
                        3 => self.hue,
                        4 => self.saturation,
                        5 => self.lightness,
                        _ => self.alpha,
                    };
                    let new_val = (old_val + scroll_amount * step).clamp(0.0, 1.0);
                    if (new_val - old_val).abs() > 0.0001 {
                        match i {
                            0 => {
                                self.red = new_val;
                                let (h, sat, l) = rgb_to_hsl(self.red, self.green, self.blue);
                                self.saturation = sat;
                                self.lightness = l;
                                if sat > 0.001 && l > 0.001 && l < 0.999 {
                                    self.hue = h;
                                }
                            }
                            1 => {
                                self.green = new_val;
                                let (h, sat, l) = rgb_to_hsl(self.red, self.green, self.blue);
                                self.saturation = sat;
                                self.lightness = l;
                                if sat > 0.001 && l > 0.001 && l < 0.999 {
                                    self.hue = h;
                                }
                            }
                            2 => {
                                self.blue = new_val;
                                let (h, sat, l) = rgb_to_hsl(self.red, self.green, self.blue);
                                self.saturation = sat;
                                self.lightness = l;
                                if sat > 0.001 && l > 0.001 && l < 0.999 {
                                    self.hue = h;
                                }
                            }
                            3 => {
                                self.hue = new_val;
                                let (r, g, b) = hsl_to_rgb(self.hue, self.saturation, self.lightness);
                                self.red = r;
                                self.green = g;
                                self.blue = b;
                            }
                            4 => {
                                self.saturation = new_val;
                                let (r, g, b) = hsl_to_rgb(self.hue, self.saturation, self.lightness);
                                self.red = r;
                                self.green = g;
                                self.blue = b;
                            }
                            5 => {
                                self.lightness = new_val;
                                let (r, g, b) = hsl_to_rgb(self.hue, self.saturation, self.lightness);
                                self.red = r;
                                self.green = g;
                                self.blue = b;
                            }
                            _ => {
                                self.alpha = new_val;
                            }
                        }
                        self.needs_rebuild = true;
                    }
                }
            }
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.width = width;
            self.height = height;
            self.config.width = width;
            self.config.height = height;
            self.wgpu_surface.configure(&self.device, &self.config);
            self.needs_rebuild = true;
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

        let output = match self.wgpu_surface.get_current_texture() {
            Ok(t) => t,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.wgpu_surface.configure(&self.device, &self.config);
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
        output.present();
    }
}

struct AppState {
    registry_state: RegistryState,
    compositor_state: CompositorState,
    xdg_shell_state: XdgShell,
    shm_state: Shm,
    seat_state: SeatState,
    output_state: OutputState,

    seats: Vec<wl_seat::WlSeat>,
    pointer: Option<wl_pointer::WlPointer>,
    keyboard: Option<wl_keyboard::WlKeyboard>,

    window: Option<XdgWindow>,
    surface: Option<wl_surface::WlSurface>,

    state: Option<ColorApp>,
    exit: bool,
    redraw: bool,
}

impl CompositorHandler for AppState {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        scale_factor: i32,
    ) {
        _surface.set_buffer_scale(scale_factor);
        if let Some(state) = &mut self.state {
            state.scale_factor = scale_factor as f64;
            let win_h = if state.with_alpha { WIN_H + SLIDER_ROW_H } else { WIN_H };
            state.resize((WIN_W * state.scale_factor as f32) as u32, (win_h * state.scale_factor as f32) as u32);
            self.redraw = true;
        }
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for AppState {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl SeatHandler for AppState {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }

    fn new_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        self.seats.push(seat);
    }

    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer && self.pointer.is_none() {
            let pointer = self.seat_state.get_pointer(qh, &seat).unwrap();
            self.pointer = Some(pointer);
        }
        if capability == Capability::Keyboard && self.keyboard.is_none() {
            let keyboard = self
                .seat_state
                .get_keyboard(qh, &seat, None)
                .unwrap();
            self.keyboard = Some(keyboard);
        }
    }

    fn remove_capability(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        if capability == Capability::Pointer {
            self.pointer = None;
        }
        if capability == Capability::Keyboard {
            self.keyboard = None;
        }
    }

    fn remove_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, seat: wl_seat::WlSeat) {
        self.seats.retain(|s| s != &seat);
    }
}

impl ShmHandler for AppState {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm_state
    }
}

impl PointerHandler for AppState {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _pointer: &wl_pointer::WlPointer,
        events: &[smithay_client_toolkit::seat::pointer::PointerEvent],
    ) {
        use smithay_client_toolkit::seat::pointer::PointerEventKind;
        for event in events {
            if let Some(st) = &mut self.state {
                let (cx, cy) = clear_ui::wayland::scale_pointer_pos(event.position, st.scale_factor);
                match &event.kind {
                    PointerEventKind::Motion { .. } => {
                        st.handle_cursor_moved(cx, cy);
                        self.redraw = true;
                    }
                    PointerEventKind::Press { button, .. } => {
                        if *button == 272 {
                            st.cursor_x = cx;
                            st.cursor_y = cy;
                            st.handle_mouse_input(clear_ui::widget::ElementState::Pressed);
                            self.redraw = true;
                        }
                    }
                    PointerEventKind::Release { button, .. } => {
                        if *button == 272 {
                            st.cursor_x = cx;
                            st.cursor_y = cy;
                            st.handle_mouse_input(clear_ui::widget::ElementState::Released);
                            self.redraw = true;
                        }
                    }
                    PointerEventKind::Axis { horizontal, vertical, .. } => {
                        let h_scroll = horizontal.absolute as f32;
                        let v_scroll = vertical.absolute as f32;
                        st.handle_scroll(-h_scroll / 10.0, -v_scroll / 10.0);
                        self.redraw = true;
                    }
                    _ => {}
                }
            }
        }
    }
}

impl KeyboardHandler for AppState {
    fn enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _surface: &wl_surface::WlSurface,
        _serial: u32,
        _raw_modifiers: &[u32],
        _keysyms: &[xkeysym::Keysym],
    ) {}

    fn leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _surface: &wl_surface::WlSurface,
        _serial: u32,
    ) {}

    fn press_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        _event: smithay_client_toolkit::seat::keyboard::KeyEvent,
    ) {}

    fn release_key(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        _event: smithay_client_toolkit::seat::keyboard::KeyEvent,
    ) {}

    fn update_modifiers(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        _modifiers: smithay_client_toolkit::seat::keyboard::Modifiers,
        _layout: u32,
    ) {}
}

impl WindowHandler for AppState {
    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _window: &XdgWindow,
        configure: WindowConfigure,
        _serial: u32,
    ) {
        let (w, h) = configure.new_size;
        if let (Some(w), Some(h)) = (w, h) {
            let width = w.get();
            let height = h.get();
            if let Some(state) = &mut self.state {
                let pw = (width as f64 * state.scale_factor) as u32;
                let ph = (height as f64 * state.scale_factor) as u32;
                state.resize(pw, ph);
            }
        }
        self.redraw = true;
    }

    fn request_close(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _window: &XdgWindow) {
        self.exit = true;
    }
}

impl ProvidesRegistryState for AppState {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    
    fn runtime_add_global(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _name: u32,
        _interface: &str,
        _version: u32,
    ) {}
    
    fn runtime_remove_global(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _name: u32,
        _interface: &str,
    ) {}
}

delegate_compositor!(AppState);
delegate_xdg_shell!(AppState);
delegate_xdg_window!(AppState);
delegate_shm!(AppState);
delegate_seat!(AppState);
delegate_pointer!(AppState);
delegate_keyboard!(AppState);
delegate_registry!(AppState);
delegate_output!(AppState);

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut with_alpha = false;
    let mut hex_arg = None;
    for arg in args.iter().skip(1) {
        if arg == "--alpha" || arg == "-a" {
            with_alpha = true;
        } else {
            hex_arg = Some(arg.as_str());
        }
    }

    let (r, g, b, a) = if let Some(hex) = hex_arg {
        if let Some((r_parsed, g_parsed, b_parsed, parsed_a)) = parse_hex(hex) {
            if parsed_a.is_some() {
                with_alpha = true;
            }
            (r_parsed, g_parsed, b_parsed, parsed_a.unwrap_or(1.0))
        } else {
            (0.5, 0.5, 0.5, 1.0)
        }
    } else {
        (0.5, 0.5, 0.5, 1.0)
    };

    let conn = Connection::connect_to_env().unwrap();
    let (globals, mut event_queue) = registry_queue_init(&conn).unwrap();
    let qh = event_queue.handle();

    let compositor_state = CompositorState::bind(&globals, &qh).unwrap();
    let xdg_shell_state = XdgShell::bind(&globals, &qh).unwrap();
    let shm_state = Shm::bind(&globals, &qh).unwrap();
    let seat_state = SeatState::new(&globals, &qh);
    let output_state = OutputState::new(&globals, &qh);

    let mut app = AppState {
        registry_state: RegistryState::new(&globals),
        compositor_state,
        xdg_shell_state,
        shm_state,
        seat_state,
        output_state,
        seats: Vec::new(),
        pointer: None,
        keyboard: None,
        window: None,
        surface: None,
        state: None,
        exit: false,
        redraw: true,
    };

    // Perform a roundtrip to populate output_state with active output scales
    event_queue.roundtrip(&mut app).unwrap();

    let scale = clear_ui::wayland::detect_scale_factor(&app.output_state);

    let state = pollster::block_on(ColorApp::new(
        &conn,
        &qh,
        &app.compositor_state,
        &app.xdg_shell_state,
        r, g, b, a,
        with_alpha,
        scale,
    ));

    app.window = Some(state.window.clone());
    app.surface = Some(state.surface.clone());
    app.state = Some(state);

    let mut event_loop = EventLoop::try_new().unwrap();
    let loop_handle = event_loop.handle();
    WaylandSource::new(conn, event_queue).insert(loop_handle).unwrap();

    loop {
        event_loop
            .dispatch(std::time::Duration::from_millis(16), &mut app)
            .unwrap();
        if app.exit {
            break;
        }
        if let Some(ref state) = app.state {
            if let Some(action) = state.action_requested {
                match action {
                    Action::Apply => {
                        println!("{}", state.hex());
                    }
                    Action::Cancel => {}
                }
                break;
            }
        }
        if app.redraw {
            app.redraw = false;
            if let Some(state) = &mut app.state {
                state.render();
            }
        }
    }
    drop(app);
}
