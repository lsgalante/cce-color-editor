use cce_ui::engine::{Application, WindowSettings, LogicalSize, LogicalPosition, EngineState};
use cce_ui::widget::{
    Adapted, Button, WidgetHost, EventCtx, UiContext, MouseButton, ElementState, KeyEvent,
    MouseScrollDelta, Event,
};
use cce_ui::layout::RenderTarget;
use cce_ui::scene::layout::{Rect, Size};
use cce_ui::scene::paint::PaintCtx;
use wayland_client::QueueHandle;
use std::io::IsTerminal;


const SLIDER_ROW_H: f32 = 36.0;
const SLIDER_START_Y: f32 = 12.0;
const SLIDER_LABEL_X: f32 = 12.0;
const SLIDER_TRACK_X: f32 = 32.0;
const SLIDER_TRACK_H: f32 = 20.0;
const PREVIEW_X: f32 = 12.0;
const PREVIEW_Y: f32 = 240.0;
const PREVIEW_W: f32 = 160.0;
const PREVIEW_H: f32 = 72.0;
const BUTTON_Y: f32 = 324.0;
const BUTTON_H: f32 = 32.0;
const BUTTON_W: f32 = 100.0;
const BUTTON_GAP: f32 = 12.0;

// ── PageContent for custom Target Rendering ────────────────────────

pub struct PageContent {
    pub rects: Vec<([f32; 4], f32, f32, f32, f32, f32, (bool, bool, bool, bool))>,
    pub texts: Vec<(String, f32, f32, f32, [f32; 4], Option<String>, Option<[f32; 4]>)>,
}

impl PageContent {
    pub fn new() -> Self {
        Self {
            rects: Vec::new(),
            texts: Vec::new(),
        }
    }
}

impl cce_ui::layout::RenderTarget for PageContent {
    fn rect(&mut self, color: [f32; 4], x: f32, y: f32, w: f32, h: f32) {
        self.rects.push((color, x, y, w, h, 0.0, (true, true, true, true)));
    }

    fn rect_with_radius(&mut self, color: [f32; 4], x: f32, y: f32, w: f32, h: f32, radius: f32) {
        self.rects.push((color, x, y, w, h, radius, (true, true, true, true)));
    }

    fn rect_with_radius_corners(&mut self, color: [f32; 4], x: f32, y: f32, w: f32, h: f32, radius: f32, corners: (bool, bool, bool, bool)) {
        self.rects.push((color, x, y, w, h, radius, corners));
    }

    fn text(&mut self, content: &str, x: f32, y: f32, size: f32, color: [f32; 4]) {
        self.texts.push((content.to_string(), size, x, y, color, None, None));
    }

    fn text_with_font(&mut self, content: &str, x: f32, y: f32, size: f32, color: [f32; 4], font: &str) {
        self.texts.push((content.to_string(), size, x, y, color, Some(font.to_string()), None));
    }

    fn text_with_bounds(&mut self, content: &str, x: f32, y: f32, size: f32, color: [f32; 4], bounds: Option<[f32; 4]>) {
        self.texts.push((content.to_string(), size, x, y, color, None, bounds));
    }

    fn text_with_font_and_bounds(&mut self, content: &str, x: f32, y: f32, size: f32, color: [f32; 4], font: &str, bounds: Option<[f32; 4]>) {
        self.texts.push((content.to_string(), size, x, y, color, Some(font.to_string()), bounds));
    }
}

// ── Custom ColorSlider Widget (narrow traits, wrapped in Adapted) ────

#[derive(Debug, Clone)]
struct ColorSlider {
    value: f32,
    channel_index: usize,
    dragging: bool,
    label: String,
    just_changed: bool,

    r: f32, g: f32, b: f32,
    h: f32, s: f32, l: f32,
    a: f32,
}

impl ColorSlider {
    pub fn new(label: &str, channel_index: usize) -> Adapted<ColorSlider> {
        Adapted::new(Self {
            value: 0.5,
            channel_index,
            dragging: false,
            label: label.to_string(),
            just_changed: false,
            r: 0.5, g: 0.5, b: 0.5,
            h: 0.0, s: 0.0, l: 0.5,
            a: 1.0,
        })
    }
}

/// The track's geometry within the slider's laid-out row rect (shared by paint and events).
fn track_rect(rect: Rect) -> (f32, f32, f32, f32) {
    let track_x = rect.x + SLIDER_TRACK_X;
    let track_w = rect.width - SLIDER_TRACK_X - 68.0;
    let track_h = SLIDER_TRACK_H;
    let track_y = rect.y + (rect.height - track_h) / 2.0;
    (track_x, track_y, track_w, track_h)
}

impl cce_ui::widget::Layout for ColorSlider {
    fn intrinsic_size(&self) -> Option<Size> {
        Some(Size { width: 0.0, height: SLIDER_ROW_H })
    }
}

impl cce_ui::widget::Paint for ColorSlider {
    fn color(&self) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn paint(&self, rect: Rect, ctx: &mut PaintCtx) {
        let (track_x, track_y, track_w, track_h) = track_rect(rect);

        // 1. Track border
        ctx.quad(
            Rect { x: track_x - 1.0, y: track_y - 1.0, width: track_w + 2.0, height: track_h + 2.0 },
            cce_ui::color::color_borders_color(),
        );

        // 2. Checkerboard behind alpha track (channel_index == 6)
        if self.channel_index == 6 {
            // base background
            ctx.quad(Rect { x: track_x, y: track_y, width: track_w, height: track_h }, [0.8, 0.8, 0.8, 1.0]);

            let grid_size = track_h / 2.0;
            let cols = (track_w / grid_size).ceil() as i32;
            for r in 0..2 {
                for c in 0..cols {
                    if (r + c) % 2 == 1 {
                        let qx = track_x + c as f32 * grid_size;
                        let qy = track_y + r as f32 * grid_size;
                        let qw = grid_size.min(track_x + track_w - qx);
                        let qh = grid_size.min(track_y + track_h - qy);
                        if qw > 0.0 && qh > 0.0 {
                            ctx.quad(Rect { x: qx, y: qy, width: qw, height: qh }, [1.0, 1.0, 1.0, 1.0]);
                        }
                    }
                }
            }
        }

        // 3. Gradient track segments
        let n_segments = (track_w as usize).max(1);
        let get_color_at = |t: f32| -> [f32; 4] {
            match self.channel_index {
                0 => [t, self.g, self.b, self.a],
                1 => [self.r, t, self.b, self.a],
                2 => [self.r, self.g, t, self.a],
                3 => {
                    let (r, g, b) = hsl_to_rgb(t, self.s, self.l);
                    [r, g, b, self.a]
                }
                4 => {
                    let (r, g, b) = hsl_to_rgb(self.h, t, self.l);
                    [r, g, b, self.a]
                }
                5 => {
                    let (r, g, b) = hsl_to_rgb(self.h, self.s, t);
                    [r, g, b, self.a]
                }
                _ => [self.r, self.g, self.b, t],
            }
        };

        for j in 0..n_segments {
            let t0 = j as f32 / n_segments as f32;
            let t1 = (j + 1) as f32 / n_segments as f32;
            let mid = (t0 + t1) / 2.0;
            let c = cce_ui::color::to_linear(get_color_at(mid));
            ctx.quad(
                Rect { x: track_x + t0 * track_w, y: track_y, width: (t1 - t0) * track_w, height: track_h },
                c,
            );
        }

        // 4. Indicator (thumb)
        let indicator_w = 4.0;
        let indicator_h = track_h + 4.0;
        let indicator_x = track_x + self.value * track_w - indicator_w / 2.0;
        let indicator_y = track_y - 2.0;

        ctx.quad(
            Rect { x: indicator_x - 1.0, y: indicator_y - 1.0, width: indicator_w + 2.0, height: indicator_h + 2.0 },
            [0.05, 0.05, 0.05, 0.95],
        );
        ctx.quad(
            Rect { x: indicator_x, y: indicator_y, width: indicator_w, height: indicator_h },
            [1.0, 1.0, 1.0, 1.0],
        );

        // 5. Own labels: channel letter + value readout
        let text_y = rect.y + 8.0;
        ctx.text(self.label.clone(), rect.x + SLIDER_LABEL_X, text_y, 12.0, [0xaa, 0xaa, 0xbb]);

        let val_str = if self.channel_index < 3 {
            format!("{}", (self.value * 255.0) as u8)
        } else if self.channel_index == 3 {
            format!("{}°", (self.value * 360.0).round() as u16)
        } else if self.channel_index < 6 {
            format!("{}%", (self.value * 100.0).round() as u8)
        } else {
            format!("{}", (self.value * 255.0).round() as u8)
        };
        ctx.text(val_str, rect.x + rect.width - 60.0, text_y, 11.0, [0xcc, 0xcc, 0xdd]);
    }
}

impl cce_ui::widget::Input for ColorSlider {
    fn on_event(&mut self, event: &Event, ectx: &mut EventCtx) -> bool {
        match event {
            // Presses arrive hit-gated to the row rect; the track is narrower — re-check it.
            // Releases arrive ungated (commit/cancel contract): same track gate as legacy.
            Event::MouseButton { button, state, x, y, .. } => {
                if *button != MouseButton::Left {
                    return false;
                }
                let (track_x, track_y, track_w, track_h) = track_rect(ectx.rect);
                if *x >= track_x && *x <= track_x + track_w && *y >= track_y && *y <= track_y + track_h {
                    if *state == ElementState::Pressed {
                        self.dragging = true;
                        self.value = ((*x - track_x) / track_w).clamp(0.0, 1.0);
                        self.just_changed = true;
                    } else {
                        self.dragging = false;
                    }
                    return true;
                }
                false
            }
            Event::MouseWheel { delta, x, y, .. } => {
                let Some(ui) = ectx.ui.as_deref_mut() else {
                    return false;
                };
                // Scroll-gesture gating: only the widget that initiated the gesture keeps it.
                if !ui.scroll_gesture_new && ui.scroll_initiate_widget_id != Some(ectx.id) {
                    return false;
                }
                let r = ectx.rect;
                if *x >= r.x && *x <= r.x + r.width && *y >= r.y && *y <= r.y + r.height {
                    if ui.scroll_gesture_new {
                        ui.scroll_initiate_widget_id = Some(ectx.id);
                    }
                    let scroll_amount = match delta {
                        MouseScrollDelta::LineDelta(_x, y) => *y,
                        MouseScrollDelta::PixelDelta(pos) => pos.y as f32 / 120.0,
                    };

                    let step = if self.channel_index == 3 {
                        5.0 / 360.0
                    } else if self.channel_index == 6 {
                        0.05
                    } else if self.channel_index >= 4 && self.channel_index <= 5 {
                        0.01
                    } else {
                        1.0 / 255.0
                    };

                    let new_value = (self.value + scroll_amount * step).clamp(0.0, 1.0);
                    if (new_value - self.value).abs() > 0.0001 {
                        self.value = new_value;
                        self.just_changed = true;
                    }
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    fn draggable(&self, _rect: Rect) -> bool {
        true
    }

    fn is_dragging(&self) -> bool {
        self.dragging
    }

    fn drag_begin(&mut self, _px: f32, _py: f32, _rect: Rect) {
        self.dragging = true;
    }

    fn drag_update(&mut self, px: f32, _py: f32, rect: Rect) -> bool {
        let (track_x, _, track_w, _) = track_rect(rect);
        let val = ((px - track_x) / track_w).clamp(0.0, 1.0);
        if (val - self.value).abs() > 0.001 {
            self.value = val;
            self.just_changed = true;
            return true;
        }
        false
    }

    fn drag_end(&mut self) {
        self.dragging = false;
    }
}

// ── AppWidget for flat/rounded rect rendering ────────────────────────

struct AppWidget {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: [f32; 4],
    radius: f32,
    corners: (bool, bool, bool, bool),
}

// ── Message definition ──────────────────────────────────────────────

#[derive(Debug, Clone)]
enum Message {
    Apply,
    Cancel,
}

// ── ColorApp State ───────────────────────────────────────────────────

struct ColorApp {
    apply_btn: cce_ui::widget::Adapted<cce_ui::widget::Button>,
    cancel_btn: cce_ui::widget::Adapted<cce_ui::widget::Button>,
    sliders: Vec<Adapted<ColorSlider>>,

    red: f32,
    green: f32,
    blue: f32,
    hue: f32,
    saturation: f32,
    lightness: f32,
    alpha: f32,
    with_alpha: bool,
    expecting_output: bool,
    // --stream: print every color change (flushed) so the launching widget
    // applies it live while this picker stays open; Cancel prints `cancel`
    // so the caller can restore the launch value.
    stream: bool,
    last_streamed: String,

    cursor_x: f32,
    cursor_y: f32,
    width: u32,
    height: u32,
    scale_factor: f64,
    needs_rebuild: bool,
    ui_context: UiContext,

    widgets: Vec<AppWidget>,
    // (content, font_size, x, y, color, font, bounds) — the PageContent text tuples,
    // emitted as display-list Text prims.
    texts: Vec<(String, f32, f32, f32, [f32; 4], Option<String>, Option<[f32; 4]>)>,
}

impl ColorApp {
    fn hex(&self) -> String {
        if self.with_alpha {
            format!(
                "#{:02X}{:02X}{:02X}{:02X}",
                (self.red * 255.0) as u8,
                (self.green * 255.0) as u8,
                (self.blue * 255.0) as u8,
                (self.alpha * 255.0) as u8
            )
        } else {
            format!(
                "#{:02X}{:02X}{:02X}",
                (self.red * 255.0) as u8,
                (self.green * 255.0) as u8,
                (self.blue * 255.0) as u8
            )
        }
    }

    fn update_sliders_color_state(&mut self) {
        for slider in &mut self.sliders {
            slider.r = self.red;
            slider.g = self.green;
            slider.b = self.blue;
            slider.h = self.hue;
            slider.s = self.saturation;
            slider.l = self.lightness;
            slider.a = self.alpha;
        }
    }

    fn update_color_from_slider(&mut self, i: usize, val: f32) {
        match i {
            0 => {
                self.red = val;
                let (h, sat, l) = rgb_to_hsl(self.red, self.green, self.blue);
                self.saturation = sat;
                self.lightness = l;
                if sat > 0.001 && l > 0.001 && l < 0.999 {
                    self.hue = h;
                }
            }
            1 => {
                self.green = val;
                let (h, sat, l) = rgb_to_hsl(self.red, self.green, self.blue);
                self.saturation = sat;
                self.lightness = l;
                if sat > 0.001 && l > 0.001 && l < 0.999 {
                    self.hue = h;
                }
            }
            2 => {
                self.blue = val;
                let (h, sat, l) = rgb_to_hsl(self.red, self.green, self.blue);
                self.saturation = sat;
                self.lightness = l;
                if sat > 0.001 && l > 0.001 && l < 0.999 {
                    self.hue = h;
                }
            }
            3 => {
                self.hue = val;
                let (r, g, b) = hsl_to_rgb(self.hue, self.saturation, self.lightness);
                self.red = r;
                self.green = g;
                self.blue = b;
            }
            4 => {
                self.saturation = val;
                let (r, g, b) = hsl_to_rgb(self.hue, self.saturation, self.lightness);
                self.red = r;
                self.green = g;
                self.blue = b;
            }
            5 => {
                self.lightness = val;
                let (r, g, b) = hsl_to_rgb(self.hue, self.saturation, self.lightness);
                self.red = r;
                self.green = g;
                self.blue = b;
            }
            _ => {
                self.alpha = val;
            }
        }
        self.sync_slider_values();
        self.update_sliders_color_state();
    }

    fn sync_slider_values(&mut self) {
        self.sliders[0].value = self.red;
        self.sliders[1].value = self.green;
        self.sliders[2].value = self.blue;
        self.sliders[3].value = self.hue;
        self.sliders[4].value = self.saturation;
        self.sliders[5].value = self.lightness;
        if self.with_alpha {
            self.sliders[6].value = self.alpha;
        }
    }

    fn rebuild_layout(&mut self) {
        self.ui_context.clear_hierarchy();
        let mut widgets = Vec::new();
        let mut texts = Vec::new();

        // 1. Layout elements (root Backplate DISSOLVED: widgets are top-level; its plate
        // is emitted below as the first tuple)
        let preview_y_offset = if self.with_alpha { SLIDER_ROW_H } else { 0.0 };
        let preview_y = PREVIEW_Y + preview_y_offset;
        let button_y = BUTTON_Y + preview_y_offset;

        let apply_x = PREVIEW_X;
        let cancel_x = PREVIEW_X + BUTTON_W + BUTTON_GAP;

        if self.expecting_output {
            self.apply_btn.set_rect(apply_x, button_y, BUTTON_W, BUTTON_H);
            self.cancel_btn.set_rect(cancel_x, button_y, BUTTON_W, BUTTON_H);
        }

        for (i, slider) in self.sliders.iter_mut().enumerate() {
            let row_y = SLIDER_START_Y + i as f32 * SLIDER_ROW_H;
            slider.set_rect(0.0, row_y, self.width as f32, SLIDER_ROW_H);
        }

        // 2. Each top-level widget rendered through the same immediate-mode path the root
        // recursion used — replicating the legacy TUPLE ORDER exactly: the sliders' plain
        // gradient quads first, then the dissolved root Backplate's translucent plate OVER
        // them (the legacy aggregate emitted all plain quads, then the rounded root bg —
        // the app's muted pastel look depends on that wash), then the rounded buttons.
        let mut window_pc = PageContent::new();
        {
            let self_ptr = self as *mut Self;
            unsafe {
                for slider in (*self_ptr).sliders.iter_mut() {
                    let (x, y, w, h) = slider.rect();
                    cce_ui::layout::render_widget(&mut window_pc, slider, x, y, w, h, &mut self.ui_context);
                }
            }
            // Backplate::color() default: page-low at the active backplate opacity.
            let mut c = cce_ui::color::page_low_color();
            if c[3] > 0.001 {
                c[3] = cce_ui::color::active_backplate_opacity();
            }
            let radius = cce_ui::colors::backplate_corner_radius();
            window_pc.rects.push((c, 0.0, 0.0, self.width as f32, self.height as f32, radius.max(0.0), (radius > 0.1, radius > 0.1, radius > 0.1, radius > 0.1)));
            unsafe {
                if self.expecting_output {
                    let (x, y, w, h) = (*self_ptr).apply_btn.rect();
                    cce_ui::layout::render_widget(&mut window_pc, &mut (*self_ptr).apply_btn, x, y, w, h, &mut self.ui_context);
                    let (x, y, w, h) = (*self_ptr).cancel_btn.rect();
                    cce_ui::layout::render_widget(&mut window_pc, &mut (*self_ptr).cancel_btn, x, y, w, h, &mut self.ui_context);
                }
            }
        }

        // 5. Render custom elements
        let mut custom_pc = PageContent::new();



        if self.with_alpha {
            // Draw checkerboard behind the preview box
            let px = PREVIEW_X;
            let py = preview_y;
            let pw = PREVIEW_W;
            let ph = PREVIEW_H;

            custom_pc.rect([0.8, 0.8, 0.8, 1.0], px, py, pw, ph);

            let grid_size = 12.0;
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
                            custom_pc.rect([1.0, 1.0, 1.0, 1.0], qx, qy, qw, qh);
                        }
                    }
                }
            }
        }

        // Color block preview
        let linear_col = cce_ui::color::to_linear([
            self.red,
            self.green,
            self.blue,
            if self.with_alpha { self.alpha } else { 1.0 },
        ]);
        custom_pc.rect(linear_col, PREVIEW_X, preview_y, PREVIEW_W, PREVIEW_H);

        // Hex string readout
        let hex = self.hex();
        custom_pc.text(
            &hex,
            PREVIEW_X + PREVIEW_W + 16.0,
            preview_y + 26.0,
            16.0,
            [0.88, 0.88, 0.91, 1.0],
        );

        // 6. Gather all quads and text labels
        for pc_part in [window_pc, custom_pc] {
            for (c, x, y, w, h, r, corners) in &pc_part.rects {
                widgets.push(AppWidget {
                    x: *x,
                    y: *y,
                    w: *w,
                    h: *h,
                    color: *c,
                    radius: *r,
                    corners: *corners,
                });
            }
            texts.extend(pc_part.texts);
        }

        self.widgets = widgets;
        self.texts = texts;
        self.ui_context.clear_dirty();
        self.needs_rebuild = false;
    }
}

// ── Application Trait Implementation ────────────────────────────────

impl Application for ColorApp {
    type Message = Message;

    fn ui_context(&self) -> Option<&cce_ui::context::UiContext> {
        Some(&self.ui_context)
    }

    fn new(_qh: &QueueHandle<EngineState<Self>>, _sender: calloop::channel::Sender<Self::Message>) -> Self {
        let args: Vec<String> = std::env::args().collect();
        let mut with_alpha = false;
        let mut stream = false;
        let mut hex_arg = None;
        for arg in args.iter().skip(1) {
            if arg == "--alpha" || arg == "-a" {
                with_alpha = true;
            } else if arg == "--stream" {
                stream = true;
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

        let (hue, saturation, lightness) = rgb_to_hsl(r, g, b);

        let expecting_output = !std::io::stdout().is_terminal();

        let initial_w = 380;
        let initial_h = if expecting_output {
            if with_alpha { 464 } else { 428 }
        } else {
            if with_alpha { 360 } else { 324 }
        };


        let apply_btn = Button::new(0.0, 0.0, BUTTON_W, BUTTON_H)
            .with_label("Apply")
            .with_bg([0.20, 0.40, 0.65, 1.0])
            .with_hover_bg([0.30, 0.52, 0.78, 1.0])
            .with_label_color([0.93, 0.93, 0.94, 1.0]);

        let cancel_btn = Button::new(0.0, 0.0, BUTTON_W, BUTTON_H)
            .with_label("Cancel")
            .with_bg([0.40, 0.20, 0.20, 1.0])
            .with_hover_bg([0.55, 0.20, 0.20, 1.0])
            .with_label_color([0.93, 0.93, 0.94, 1.0]);

        let mut sliders = vec![
            ColorSlider::new("R", 0),
            ColorSlider::new("G", 1),
            ColorSlider::new("B", 2),
            ColorSlider::new("H", 3),
            ColorSlider::new("S", 4),
            ColorSlider::new("L", 5),
        ];
        if with_alpha {
            sliders.push(ColorSlider::new("A", 6));
        }

        let mut app = Self {
            apply_btn,
            cancel_btn,
            sliders,
            red: r,
            green: g,
            blue: b,
            hue,
            saturation,
            lightness,
            alpha: a,
            with_alpha,
            expecting_output,
            stream,
            last_streamed: String::new(),
            cursor_x: 0.0,
            cursor_y: 0.0,
            width: initial_w,
            height: initial_h,
            scale_factor: 1.0,
            needs_rebuild: true,
            ui_context: UiContext::new(),
            widgets: Vec::new(),
            texts: Vec::new(),
        };

        app.sync_slider_values();
        app.update_sliders_color_state();
        app.rebuild_layout();
        app
    }

    fn settings(&self) -> WindowSettings {
        let win_h = if self.expecting_output {
            if self.with_alpha { 464 } else { 428 }
        } else {
            if self.with_alpha { 360 } else { 324 }
        };
        WindowSettings {
            title: "Color Interface".to_string(),
            app_id: "cce-colors".to_string(),
            width: 380,
            height: win_h,
            fullscreen: false,
            min_size: Some((380, win_h)),
        }
    }

    fn update(&mut self, msg: Self::Message, _needs_rebuild: &mut bool, exit: &mut bool) {
        match msg {
            Message::Apply => {
                println!("{}", self.hex());
                *exit = true;
            }
            Message::Cancel => {
                if self.stream {
                    use std::io::Write;
                    println!("cancel");
                    let _ = std::io::stdout().flush();
                }
                *exit = true;
            }
        }
    }

    fn tick(&mut self, _dt: f32, _needs_rebuild: &mut bool) {
        // Stream the live color to the launching widget on every change.
        if self.stream {
            let hex = self.hex();
            if hex != self.last_streamed {
                use std::io::Write;
                println!("{}", hex);
                let _ = std::io::stdout().flush();
                self.last_streamed = hex;
            }
        }
    }

    fn handle_mouse_wheel(&mut self, delta: &MouseScrollDelta, pos: LogicalPosition, needs_rebuild: &mut bool) {
        let px = pos.x;
        let py = pos.y;
        let event = Event::MouseWheel {
            delta: *delta,
            x: px,
            y: py,
            local_x: px,
            local_y: py,
        };
        // Root Backplate dissolved: propagate to each slider directly (they own
        // mouse_wheel; the buttons never scrolled).
        let mut changed_slider = None;
        let mut any = false;
        {
            let self_ptr = self as *mut Self;
            unsafe {
                for slider in (*self_ptr).sliders.iter_mut() {
                    if self.ui_context.propagate_event(&event, slider.id()) {
                        any = true;
                        break;
                    }
                }
            }
        }
        if any {
            for (i, slider) in self.sliders.iter_mut().enumerate() {
                if slider.just_changed {
                    slider.just_changed = false;
                    changed_slider = Some((i, slider.value));
                    break;
                }
            }
        }
        if let Some((i, val)) = changed_slider {
            self.update_color_from_slider(i, val);
            *needs_rebuild = true;
            self.needs_rebuild = true;
        }
    }

    fn display_list(&mut self, size: LogicalSize, scale: f64) -> Option<cce_ui::scene::paint::DisplayList> {
        // Phase 6 single paint path: the whole frame — geometry and text — is this one list.
        // rebuild_layout flattens the UI (incl. color ramps/gradients) into self.widgets/self.texts.
        if self.needs_rebuild || self.width != size.width as u32 || self.height != size.height as u32 || self.scale_factor != scale {
            self.width = size.width as u32;
            self.height = size.height as u32;
            self.scale_factor = scale;
            cce_ui::scale::set_scale_factor(scale as f32);
            self.rebuild_layout();
        }
        use cce_ui::scene::layout::Rect;
        let mut pc = cce_ui::scene::paint::PaintCtx::new();
        for w in &self.widgets {
            let rect = Rect { x: w.x, y: w.y, width: w.w, height: w.h };
            if w.radius > 0.1 {
                pc.rounded_rect(rect, w.radius, w.corners, w.color);
            } else {
                pc.quad(rect, w.color);
            }
        }
        for (text, font_size, x, y, col, font, bounds) in &self.texts {
            pc.text_with(
                text.clone(),
                *x,
                *y,
                *font_size,
                [
                    (col[0] * 255.0) as u8,
                    (col[1] * 255.0) as u8,
                    (col[2] * 255.0) as u8,
                ],
                font.clone(),
                *bounds,
            );
        }
        Some(pc.finish())
    }

    fn display_list_text(&self) -> bool {
        true
    }

    fn is_movable_backplate_at(&self, px: f32, py: f32) -> bool {
        // Root Backplate dissolved: the surface itself is the movable plate.
        self.ui_context.drag_allowed_at(px, py)
    }

    fn clear_color(&self) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn handle_pointer_move(&mut self, pos: LogicalPosition, needs_rebuild: &mut bool) {
        self.cursor_x = pos.x;
        self.cursor_y = pos.y;
        cce_ui::widget::hover_animation::set_cursor_pos(pos.x, pos.y);

        // Routed dispatch (6bd shrink): one PointerMove through the router per root —
        // hover bookkeeping plus the router's drag forwarding (replaces the app-held
        // dragging index; DragUpdate reaches the drag target even off-rect).
        let ev = Event::PointerMove { x: pos.x, y: pos.y, local_x: pos.x, local_y: pos.y };
        if self.expecting_output {
            let apply = self.apply_btn.id();
            self.ui_context.propagate_event(&ev, apply);
            let cancel = self.cancel_btn.id();
            self.ui_context.propagate_event(&ev, cancel);
        }
        let slider_roots: Vec<_> = self.sliders.iter().map(|s| s.id()).collect();
        for root in slider_roots {
            self.ui_context.propagate_event(&ev, root);
        }
        // Drain the drag's value change like the wheel path does.
        let mut changed_slider = None;
        for (i, slider) in self.sliders.iter_mut().enumerate() {
            if slider.just_changed {
                slider.just_changed = false;
                changed_slider = Some((i, slider.value));
                break;
            }
        }
        if let Some((i, val)) = changed_slider {
            self.update_color_from_slider(i, val);
        }
        // Legacy parity: every pointer move rebuilt (hover fades ride the rebuild).
        *needs_rebuild = true;
        self.needs_rebuild = true;
    }

    fn handle_mouse_input(&mut self, button: MouseButton, state: ElementState, pos: LogicalPosition, needs_rebuild: &mut bool) -> Option<Self::Message> {
        if button != MouseButton::Left {
            return None;
        }

        // Routed dispatch (6bd shrink): the router hit-gates presses, records the drag
        // target, and delivers DragEnd on release; the app keeps the take_click /
        // just_changed drains.
        let ev = Event::MouseButton { button, state, x: pos.x, y: pos.y, local_x: pos.x, local_y: pos.y };
        let was_dragging = self.ui_context.is_dragging;
        let mut handled = false;

        if self.expecting_output {
            let apply = self.apply_btn.id();
            if self.ui_context.propagate_event(&ev, apply) {
                handled = true;
                *needs_rebuild = true;
                self.needs_rebuild = true;
            }
            let cancel = self.cancel_btn.id();
            if self.ui_context.propagate_event(&ev, cancel) {
                handled = true;
                *needs_rebuild = true;
                self.needs_rebuild = true;
            }

            if self.apply_btn.take_click() {
                return Some(Message::Apply);
            }
            if self.cancel_btn.take_click() {
                return Some(Message::Cancel);
            }
        }

        if !handled {
            let slider_roots: Vec<_> = self.sliders.iter().map(|s| s.id()).collect();
            for root in slider_roots {
                if self.ui_context.propagate_event(&ev, root) {
                    break;
                }
            }
            let mut changed_slider = None;
            for (i, slider) in self.sliders.iter_mut().enumerate() {
                if slider.just_changed {
                    slider.just_changed = false;
                    changed_slider = Some((i, slider.value));
                    break;
                }
            }
            if let Some((i, val)) = changed_slider {
                self.update_color_from_slider(i, val);
                *needs_rebuild = true;
                self.needs_rebuild = true;
            }
        }

        // The router delivered DragEnd on the first propagate call of a release; rebuild
        // so the thumb sheds its dragging state, as the legacy drag_end path did.
        if state == ElementState::Released && was_dragging {
            *needs_rebuild = true;
            self.needs_rebuild = true;
        }

        None
    }

    fn handle_key_input(&mut self, event: &KeyEvent, _needs_rebuild: &mut bool) -> Option<Self::Message> {
        if event.state == ElementState::Pressed {
            match event.logical_key {
                cce_ui::widget::Key::Named(cce_ui::widget::NamedKey::Escape) => {
                    return Some(Message::Cancel);
                }
                cce_ui::widget::Key::Named(cce_ui::widget::NamedKey::Enter) => {
                    return Some(Message::Apply);
                }
                _ => {}
            }
        }
        None
    }
}

// ── Color Utilities ──────────────────────────────────────────────────

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

fn parse_hex(hex: &str) -> Option<(f32, f32, f32, Option<f32>)> {
    let has_alpha = hex
        .trim_matches(|c| c == '"' || c == '\'' || c == ' ')
        .trim_start_matches('#')
        .len()
        >= 8;
    cce_ui::color::parse_hex_rgba(hex)
        .map(|[r, g, b, a]| (r, g, b, if has_alpha { Some(a) } else { None }))
}

// ── Main Entrypoint ──────────────────────────────────────────────────

fn main() {
    cce_ui::engine::run::<ColorApp>();
}
