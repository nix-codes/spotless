//! Small Winamp-style controls drawn with the painter.

use egui::{
    Align2, FontId, Pos2, Rect, Response, Sense, Shape, Ui, Vec2, WidgetInfo, WidgetType, pos2,
    vec2,
};

use super::theme::{self, Palette};

/// The transport glyphs.
#[derive(Clone, Copy, Debug)]
pub enum Glyph {
    Previous,
    Play,
    Pause,
    Stop,
    Next,
}

impl Glyph {
    fn name(self) -> &'static str {
        match self {
            Self::Previous => "Previous",
            Self::Play => "Play",
            Self::Pause => "Pause",
            Self::Stop => "Stop",
            Self::Next => "Next",
        }
    }
}

pub const BUTTON_HEIGHT: f32 = 26.0;

/// A bevelled text button. `on` draws it pressed in, as a toggle that is on.
pub fn text_button(
    ui: &mut Ui,
    palette: &Palette,
    label: &str,
    on: bool,
    enabled: bool,
) -> Response {
    let font = FontId::monospace(11.0);
    let galley_width = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), palette.chrome)
        .size()
        .x;
    let size = vec2(galley_width + 20.0, BUTTON_HEIGHT);
    let (rect, response) = button_frame(ui, palette, size, on, enabled);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    let color = label_color(&response, palette, on, enabled);
    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, label, font, color);
    response
}

/// A square transport button.
pub fn glyph_button(ui: &mut Ui, palette: &Palette, glyph: Glyph, enabled: bool) -> Response {
    let (rect, response) = button_frame(ui, palette, vec2(36.0, BUTTON_HEIGHT), false, enabled);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, glyph.name()));
    let color = label_color(&response, palette, false, enabled);
    paint_glyph(ui, rect.center(), glyph, color);
    response
}

fn button_frame(
    ui: &mut Ui,
    palette: &Palette,
    size: Vec2,
    on: bool,
    enabled: bool,
) -> (Rect, Response) {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, palette.button);
    let pressed = on || response.is_pointer_button_down_on();
    theme::bevel(painter, rect, !pressed, palette);
    if response.has_focus() {
        theme::focus_ring(painter, rect, palette);
    }
    (rect, response)
}

fn label_color(response: &Response, palette: &Palette, on: bool, enabled: bool) -> egui::Color32 {
    if !enabled {
        palette.chrome.gamma_multiply(0.4)
    } else if on || response.hovered() {
        palette.accent
    } else {
        palette.chrome
    }
}

/// Draws a glyph about 10 px high, centred on `c`.
pub fn paint_glyph(ui: &Ui, c: Pos2, glyph: Glyph, color: egui::Color32) {
    let painter = ui.painter();
    let triangle = |left: f32, right: f32, pointing_right: bool| {
        let (tip, base) = if pointing_right {
            (right, left)
        } else {
            (left, right)
        };
        Shape::convex_polygon(
            vec![pos2(base, c.y - 5.0), pos2(tip, c.y), pos2(base, c.y + 5.0)],
            color,
            egui::Stroke::NONE,
        )
    };
    let bar = |left: f32, width: f32| Rect::from_min_size(pos2(left, c.y - 5.0), vec2(width, 10.0));
    match glyph {
        Glyph::Play => {
            painter.add(triangle(c.x - 4.0, c.x + 5.0, true));
        }
        Glyph::Pause => {
            painter.rect_filled(bar(c.x - 4.5, 3.0), 0.0, color);
            painter.rect_filled(bar(c.x + 1.5, 3.0), 0.0, color);
        }
        Glyph::Stop => {
            painter.rect_filled(Rect::from_center_size(c, vec2(9.0, 9.0)), 0.0, color);
        }
        Glyph::Previous => {
            painter.rect_filled(bar(c.x - 6.0, 2.0), 0.0, color);
            painter.add(triangle(c.x - 3.0, c.x + 6.0, false));
        }
        Glyph::Next => {
            painter.add(triangle(c.x - 6.0, c.x + 3.0, true));
            painter.rect_filled(bar(c.x + 4.0, 2.0), 0.0, color);
        }
    }
}
