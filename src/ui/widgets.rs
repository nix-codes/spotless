//! Small Winamp-style controls drawn with the painter.

use egui::{
    Align2, FontId, Pos2, Rect, Response, Sense, Shape, TextEdit, Ui, Vec2, WidgetInfo, WidgetType,
    pos2, vec2,
};

use super::skin_paint;
use super::theme::{self, Palette};
use crate::skin::format::State;

/// The transport glyphs.
#[derive(Clone, Copy, Debug)]
pub enum Glyph {
    Previous,
    Play,
    Pause,
    Stop,
    Next,
    Eject,
    Up,
    Down,
}

impl Glyph {
    fn name(self) -> &'static str {
        match self {
            Self::Previous => "Previous",
            Self::Play => "Play",
            Self::Pause => "Pause",
            Self::Stop => "Stop",
            Self::Next => "Next",
            Self::Eject => "Browse",
            Self::Up => "Move up",
            Self::Down => "Move down",
        }
    }
}

pub const BUTTON_HEIGHT: f32 = 26.0;
/// For buttons inside list rows.
pub const SMALL_HEIGHT: f32 = 18.0;

/// A bevelled text button. `on` draws it pressed in, as a toggle that is on.
pub fn text_button(
    ui: &mut Ui,
    palette: &Palette,
    label: &str,
    on: bool,
    enabled: bool,
) -> Response {
    sized_button(ui, palette, label, on, enabled, BUTTON_HEIGHT)
}

/// A text button that fits in a list row.
pub fn small_button(ui: &mut Ui, palette: &Palette, label: &str, on: bool) -> Response {
    sized_button(ui, palette, label, on, true, SMALL_HEIGHT)
}

/// A small button that can be disabled, for toolbars.
pub fn small_action(ui: &mut Ui, palette: &Palette, label: &str, enabled: bool) -> Response {
    sized_button(ui, palette, label, false, enabled, SMALL_HEIGHT)
}

/// A small square glyph button, for toolbars.
pub fn small_glyph(ui: &mut Ui, palette: &Palette, glyph: Glyph, enabled: bool) -> Response {
    let (rect, response, skinned) =
        button_frame(ui, palette, vec2(24.0, SMALL_HEIGHT), false, enabled);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, glyph.name()));
    let color = label_color(&response, palette, false, enabled, skinned);
    paint_glyph(ui, rect.center(), glyph, color);
    response
}

/// A text field: in the skin's frame when it has one, else egui's.
pub fn field(ui: &mut Ui, edit: TextEdit) -> Response {
    let Some(paint) = skin_paint::current(ui.ctx()).filter(|paint| paint.has("field")) else {
        return ui.add(edit);
    };
    // The frame goes under the text, which is laid out first.
    let slot = ui.painter().add(Shape::Noop);
    let response =
        ui.add(edit.frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(6, 4))));
    let state = if response.has_focus() {
        State::On
    } else if response.hovered() {
        State::Hover
    } else {
        State::Normal
    };
    let rect = response.rect;
    if let Some(shapes) = paint.shapes("field", state, rect, paint.scale(), ui.pixels_per_point()) {
        ui.painter().set(slot, Shape::Vec(shapes));
    }
    response
}

/// A label that follows the pointer while something is dragged.
pub fn drag_ghost(ui: &Ui, palette: &Palette, text: &str) {
    let Some(pointer) = ui.ctx().pointer_interact_pos() else {
        return;
    };
    egui::Area::new(egui::Id::new("drag-ghost"))
        .order(egui::Order::Tooltip)
        .fixed_pos(pointer + vec2(14.0, 10.0))
        .interactable(false)
        .show(ui.ctx(), |ui| {
            egui::Frame::new()
                .fill(palette.selected_bg)
                .stroke(egui::Stroke::new(1.0, palette.hi))
                .inner_margin(egui::Margin::symmetric(6, 3))
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(text).color(palette.current));
                });
        });
}

/// The width a small button with `label` takes.
pub fn small_button_width(ui: &Ui, label: &str) -> f32 {
    label_width(ui, label, &FontId::monospace(10.0)) + 14.0
}

fn label_width(ui: &Ui, label: &str, font: &FontId) -> f32 {
    ui.painter()
        .layout_no_wrap(label.to_string(), font.clone(), egui::Color32::WHITE)
        .size()
        .x
}

fn sized_button(
    ui: &mut Ui,
    palette: &Palette,
    label: &str,
    on: bool,
    enabled: bool,
    height: f32,
) -> Response {
    let small = height < BUTTON_HEIGHT;
    let font = FontId::monospace(if small { 10.0 } else { 11.0 });
    let padding = if small { 14.0 } else { 20.0 };
    let size = vec2(label_width(ui, label, &font) + padding, height);
    let (rect, response, skinned) = button_frame(ui, palette, size, on, enabled);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    let color = label_color(&response, palette, on, enabled, skinned);
    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, label, font, color);
    response
}

/// A square transport button.
pub fn glyph_button(ui: &mut Ui, palette: &Palette, glyph: Glyph, enabled: bool) -> Response {
    glyph_toggle(ui, palette, glyph, false, enabled)
}

/// A transport button that stays pressed in while `on`.
pub fn glyph_toggle(
    ui: &mut Ui,
    palette: &Palette,
    glyph: Glyph,
    on: bool,
    enabled: bool,
) -> Response {
    let (rect, response, skinned) =
        button_frame(ui, palette, vec2(36.0, BUTTON_HEIGHT), on, enabled);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, glyph.name()));
    let color = label_color(&response, palette, on, enabled, skinned);
    paint_glyph(ui, rect.center(), glyph, color);
    response
}

/// A button's frame: the skin's when it has one (and then true), else
/// Classic's bevel.
fn button_frame(
    ui: &mut Ui,
    palette: &Palette,
    size: Vec2,
    on: bool,
    enabled: bool,
) -> (Rect, Response, bool) {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let painter = ui.painter();
    let piece = if size.y < BUTTON_HEIGHT {
        "small_button"
    } else {
        "button"
    };
    let state = skin_paint::state(&response, on, enabled);
    let skinned = skin_paint::chrome(painter, piece, state, rect);
    if !skinned {
        painter.rect_filled(rect, 0.0, palette.button);
        let pressed = on || response.is_pointer_button_down_on();
        theme::bevel(painter, rect, !pressed, &palette.for_buttons());
    }
    if response.has_focus() {
        theme::focus_ring(painter, rect, palette);
    }
    (rect, response, skinned)
}

/// A label's colour: lit when on or under the pointer on Classic's
/// buttons; on a skin's, whose face may be light, its button text.
fn label_color(
    response: &Response,
    palette: &Palette,
    on: bool,
    enabled: bool,
    skinned: bool,
) -> egui::Color32 {
    if !enabled {
        palette.button_text.gamma_multiply(0.4)
    } else if (on || response.hovered()) && !skinned {
        palette.accent
    } else {
        palette.button_text
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
        Glyph::Eject => {
            painter.add(Shape::convex_polygon(
                vec![
                    pos2(c.x - 5.0, c.y + 1.0),
                    pos2(c.x + 5.0, c.y + 1.0),
                    pos2(c.x, c.y - 5.0),
                ],
                color,
                egui::Stroke::NONE,
            ));
            let tray = Rect::from_min_size(pos2(c.x - 5.0, c.y + 3.0), vec2(10.0, 2.0));
            painter.rect_filled(tray, 0.0, color);
        }
        Glyph::Up | Glyph::Down => {
            let (tip, base) = match glyph {
                Glyph::Up => (c.y - 3.0, c.y + 3.0),
                _ => (c.y + 3.0, c.y - 3.0),
            };
            painter.add(Shape::convex_polygon(
                vec![pos2(c.x - 5.0, base), pos2(c.x + 5.0, base), pos2(c.x, tip)],
                color,
                egui::Stroke::NONE,
            ));
        }
    }
}
