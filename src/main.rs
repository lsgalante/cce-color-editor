use cce_ui::engine::{Application, WindowSettings, LogicalSize, LogicalPosition, EngineState};
use cce_ui::widget::{
    Backplate, Button, Element, UiContext, MouseButton, ElementState, KeyEvent, MouseScrollDelta,
    Widget, display::TextLabel, Event,
};
use cce_ui::widget::focus::link_parent_child;
use cce_ui::layout::RenderTarget;
use glyphon::FontSystem;
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

// ── Custom ColorSlider Widget ────────────────────────────────────────

#[derive(Debug, Clone)]
struct ColorSlider {
    base: Widget,
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
    pub fn new(label: &str, channel_index: usize) -> Self {
        Self {
            base: Widget::new_rect(0.0, 0.0, 0.0, 0.0),
            value: 0.5,
            channel_index,
            dragging: false,
            label: label.to_string(),
            just_changed: false,
            r: 0.5, g: 0.5, b: 0.5,
            h: 0.0, s: 0.0, l: 0.5,
            a: 1.0,
        }
    }
}

impl Element for ColorSlider {
    cce_ui::impl_widget_base!(ColorSlider);

    fn color(&self) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn preferred_height(&self) -> Option<f32> {
        Some(SLIDER_ROW_H)
    }

    fn draggable(&self) -> bool {
        true
    }

    fn is_dragging(&self) -> bool {
        self.dragging
    }

    fn drag_begin(&mut self, _px: f32, _py: f32) {
        self.dragging = true;
    }

    fn drag_end(&mut self) {
        self.dragging = false;
    }

    fn mouse_input(&mut self, button: MouseButton, state: ElementState, px: f32, py: f32, _ctx: &mut UiContext) -> bool {
        if button != MouseButton::Left {
            return false;
        }
        let (x, y, w, h) = self.rect();
        let track_x = x + SLIDER_TRACK_X;
        let track_w = w - SLIDER_TRACK_X - 68.0;
        let track_h = SLIDER_TRACK_H;
        let track_y = y + (h - track_h) / 2.0;

        if px >= track_x && px <= track_x + track_w && py >= track_y && py <= track_y + track_h {
            if state == ElementState::Pressed {
                self.dragging = true;
                let val = ((px - track_x) / track_w).clamp(0.0, 1.0);
                self.value = val;
                self.just_changed = true;
                return true;
            } else {
                self.dragging = false;
                return true;
            }
        }
        false
    }

    fn drag_update(&mut self, px: f32, _py: f32) -> bool {
        let (x, _, w, _) = self.rect();
        let track_x = x + SLIDER_TRACK_X;
        let track_w = w - SLIDER_TRACK_X - 68.0;
        let val = ((px - track_x) / track_w).clamp(0.0, 1.0);
        if (val - self.value).abs() > 0.001 {
            self.value = val;
            self.just_changed = true;
            return true;
        }
        false
    }

    fn extra_quads(&self) -> Vec<(f32, f32, f32, f32, [f32; 4])> {
        let mut quads = Vec::new();
        let (x, y, w, h) = self.rect();

        let track_x = x + SLIDER_TRACK_X;
        let track_w = w - SLIDER_TRACK_X - 68.0;
        let track_h = SLIDER_TRACK_H;
        let track_y = y + (h - track_h) / 2.0;

        // 1. Draw track border
        quads.push((
            track_x - 1.0,
            track_y - 1.0,
            track_w + 2.0,
            track_h + 2.0,
            cce_ui::color::color_borders_color(),
        ));

        // 2. Draw checkerboard behind alpha track (channel_index == 6)
        if self.channel_index == 6 {
            // base background
            quads.push((track_x, track_y, track_w, track_h, [0.8, 0.8, 0.8, 1.0]));

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
                            quads.push((qx, qy, qw, qh, [1.0, 1.0, 1.0, 1.0]));
                        }
                    }
                }
            }
        }

        // 3. Draw gradient track segments
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
            quads.push((
                track_x + t0 * track_w,
                track_y,
                (t1 - t0) * track_w,
                track_h,
                c,
            ));
        }

        // 4. Draw indicator (thumb)
        let indicator_w = 4.0;
        let indicator_h = track_h + 4.0;
        let indicator_x = track_x + self.value * track_w - indicator_w / 2.0;
        let indicator_y = track_y - 2.0;

        quads.push((
            indicator_x - 1.0,
            indicator_y - 1.0,
            indicator_w + 2.0,
            indicator_h + 2.0,
            [0.05, 0.05, 0.05, 0.95],
        ));
        quads.push((
            indicator_x,
            indicator_y,
            indicator_w,
            indicator_h,
            [1.0, 1.0, 1.0, 1.0],
        ));

        quads
    }

    fn text_labels(&self) -> Vec<TextLabel> {
        let mut labels = Vec::new();
        let (x, y, w, _) = self.rect();
        let text_y = y + 8.0;

        // Label
        labels.push(TextLabel {
            text: self.label.clone(),
            x: x + SLIDER_LABEL_X,
            y: text_y,
            font_size: 12.0,
            color: [0xaa, 0xaa, 0xbb],
        });

        // Value readout
        let val_str = if self.channel_index < 3 {
            format!("{}", (self.value * 255.0) as u8)
        } else if self.channel_index == 3 {
            format!("{}°", (self.value * 360.0).round() as u16)
        } else if self.channel_index < 6 {
            format!("{}%", (self.value * 100.0).round() as u8)
        } else {
            format!("{}", (self.value * 255.0).round() as u8)
        };

        labels.push(TextLabel {
            text: val_str,
            x: x + w - 60.0,
            y: text_y,
            font_size: 11.0,
            color: [0xcc, 0xcc, 0xdd],
        });

        labels
    }

    fn mouse_wheel(&mut self, delta: &MouseScrollDelta, px: f32, py: f32, ctx: &mut UiContext) -> bool {
        let my_id = self.base.id();
        if !ctx.scroll_gesture_new {
            if ctx.scroll_initiate_widget_id != Some(my_id) {
                return false;
            }
        }
        let (x, y, w, h) = self.rect();

        if px >= x && px <= x + w && py >= y && py <= y + h {
            if ctx.scroll_gesture_new {
                ctx.scroll_initiate_widget_id = Some(my_id);
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
    root_window: Backplate,
    apply_btn: cce_ui::widget::Adapted<cce_ui::widget::Button>,
    cancel_btn: cce_ui::widget::Adapted<cce_ui::widget::Button>,
    sliders: Vec<ColorSlider>,

    red: f32,
    green: f32,
    blue: f32,
    hue: f32,
    saturation: f32,
    lightness: f32,
    alpha: f32,
    with_alpha: bool,
    expecting_output: bool,

    font_system: FontSystem,
    cursor_x: f32,
    cursor_y: f32,
    dragging: Option<usize>,
    width: u32,
    height: u32,
    scale_factor: f64,
    needs_rebuild: bool,
    ui_context: UiContext,

    widgets: Vec<AppWidget>,
    text_items: Vec<cce_ui::widget::TextItem>,
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
        let mut text_items = Vec::new();

        // 1. Setup root window
        self.root_window.set_rect(0.0, 0.0, self.width as f32, self.height as f32);

        self.root_window.clear_children(&mut self.ui_context);

        // 2. Link child widgets
        if self.expecting_output {
            link_parent_child(&mut self.root_window, &mut self.apply_btn, &mut self.ui_context);
            link_parent_child(&mut self.root_window, &mut self.cancel_btn, &mut self.ui_context);
        }
        for slider in &mut self.sliders {
            link_parent_child(&mut self.root_window, slider, &mut self.ui_context);
        }

        // 3. Layout elements
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

        // 4. Render window widget recursively
        let mut window_pc = PageContent::new();
        cce_ui::layout::render_widget(
            &mut window_pc,
            &mut self.root_window,
            0.0,
            0.0,
            self.width as f32,
            self.height as f32,
            &mut self.ui_context,
        );

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
        for pc_part in &[window_pc, custom_pc] {
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
            for (text, size, x, y, col, font, bounds) in &pc_part.texts {
                text_items.push(cce_ui::widget::TextItem::new(
                    &mut self.font_system,
                    text,
                    *size,
                    *x,
                    *y,
                    glyphon::Color::rgb(
                        (col[0] * 255.0) as u8,
                        (col[1] * 255.0) as u8,
                        (col[2] * 255.0) as u8,
                    ),
                    font.as_deref(),
                    *bounds,
                ));
            }
        }

        self.widgets = widgets;
        self.text_items = text_items;
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

        let (hue, saturation, lightness) = rgb_to_hsl(r, g, b);

        let expecting_output = !std::io::stdout().is_terminal();

        let initial_w = 380;
        let initial_h = if expecting_output {
            if with_alpha { 464 } else { 428 }
        } else {
            if with_alpha { 360 } else { 324 }
        };

        let root_window = Backplate::new(0.0, 0.0, initial_w as f32, initial_h as f32);

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

        let font_system = cce_ui::create_font_system_with_system_fonts();

        let mut app = Self {
            root_window,
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
            font_system,
            cursor_x: 0.0,
            cursor_y: 0.0,
            dragging: None,
            width: initial_w,
            height: initial_h,
            scale_factor: 1.0,
            needs_rebuild: true,
            ui_context: UiContext::new(),
            widgets: Vec::new(),
            text_items: Vec::new(),
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
                *exit = true;
            }
        }
    }

    fn tick(&mut self, _dt: f32, _needs_rebuild: &mut bool) {}

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
        let root_ptr = self.root_window.as_ptr_mut();
        let mut changed_slider = None;
        if self.ui_context.propagate_event(&event, root_ptr) {
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

    fn view(&mut self, _quads: &mut Vec<(f32, f32, f32, f32, [f32; 4])>, size: LogicalSize, scale: f64) {
        if self.needs_rebuild || self.width != size.width as u32 || self.height != size.height as u32 || self.scale_factor != scale {
            self.width = size.width as u32;
            self.height = size.height as u32;
            self.scale_factor = scale;
            cce_ui::scale::set_scale_factor(scale as f32);
            self.rebuild_layout();
        }
    }

    fn view_rounded_quads(&mut self, quads: &mut Vec<(f32, f32, f32, f32, f32, [f32; 4], (bool, bool, bool, bool))>, size: LogicalSize, scale: f64) {
        if self.needs_rebuild || self.width != size.width as u32 || self.height != size.height as u32 || self.scale_factor != scale {
            self.width = size.width as u32;
            self.height = size.height as u32;
            self.scale_factor = scale;
            cce_ui::scale::set_scale_factor(scale as f32);
            self.rebuild_layout();
        }
        for w in &self.widgets {
            quads.push((w.x, w.y, w.w, w.h, w.radius, w.color, w.corners));
        }
    }

    fn display_list(&mut self, _size: cce_ui::engine::LogicalSize, _scale: f64) -> Option<cce_ui::scene::paint::DisplayList> {
        // Phase 3 single paint path (flat-list bridge). rebuild_layout flattens the UI (incl. color
        // ramps/gradients) into self.widgets, which view_rounded_quads runs above. CCE_LEGACY_PAINT
        // falls back.
        if std::env::var("CCE_LEGACY_PAINT").is_ok() {
            return None;
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
        Some(pc.finish())
    }

    fn text_items(&self) -> &[cce_ui::widget::TextItem] {
        &self.text_items
    }

    fn clear_color(&self) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn handle_pointer_move(&mut self, pos: LogicalPosition, needs_rebuild: &mut bool) {
        self.cursor_x = pos.x;
        self.cursor_y = pos.y;
        cce_ui::widget::hover_animation::set_cursor_pos(pos.x, pos.y);

        if let Some(i) = self.dragging {
            let slider = &mut self.sliders[i];
            if slider.drag_update(pos.x, pos.y) {
                let val = slider.value;
                self.update_color_from_slider(i, val);
                *needs_rebuild = true;
                self.needs_rebuild = true;
            }
        } else {
            if self.expecting_output {
                let _ = self.apply_btn.cursor_moved(pos.x, pos.y, &mut self.ui_context);
                let _ = self.cancel_btn.cursor_moved(pos.x, pos.y, &mut self.ui_context);
            }
            for slider in &mut self.sliders {
                let _ = slider.cursor_moved(pos.x, pos.y, &mut self.ui_context);
            }
            *needs_rebuild = true;
            self.needs_rebuild = true;
        }
    }

    fn handle_mouse_input(&mut self, button: MouseButton, state: ElementState, pos: LogicalPosition, needs_rebuild: &mut bool) -> Option<Self::Message> {
        if button != MouseButton::Left {
            return None;
        }

        let px = pos.x;
        let py = pos.y;

        let mut handled = false;

        if self.expecting_output {
            if self.apply_btn.mouse_input(button, state, px, py, &mut self.ui_context) {
                handled = true;
                *needs_rebuild = true;
                self.needs_rebuild = true;
            }
            if self.cancel_btn.mouse_input(button, state, px, py, &mut self.ui_context) {
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

        if state == ElementState::Released {
            if let Some(i) = self.dragging {
                self.sliders[i].drag_end();
                self.dragging = None;
                *needs_rebuild = true;
                self.needs_rebuild = true;
            }
        }

        if !handled {
            for (i, slider) in self.sliders.iter_mut().enumerate() {
                if slider.mouse_input(button, state, px, py, &mut self.ui_context) {
                    if slider.dragging {
                        self.dragging = Some(i);
                    } else {
                        self.dragging = None;
                    }
                    let val = slider.value;
                    self.update_color_from_slider(i, val);
                    *needs_rebuild = true;
                    self.needs_rebuild = true;
                    break;
                }
            }
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
