//! Small Winamp-style controls drawn with the painter.

use egui::{
    Align2, Color32, FontId, Pos2, Rect, Response, RichText, Sense, Shape, TextEdit, Ui, Vec2,
    WidgetInfo, WidgetType, pos2, vec2,
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
    Shuffle,
    Repeat,
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
            Self::Shuffle => "Shuffle",
            Self::Repeat => "Repeat",
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

/// A button's tooltip, shown whether it's enabled or not (egui shows a
/// disabled one's only through `on_disabled_hover_text`).
pub fn tip(response: Response, text: &str) -> Response {
    response.on_hover_text(text).on_disabled_hover_text(text)
}

/// A right-click menu's Share, as the Spotify app's: true when its one
/// item, `copy` ("Copy song link"), was clicked.
pub fn share_menu(ui: &mut Ui, copy: &str) -> bool {
    let mut clicked = false;
    ui.menu_button(RichText::new("Share").monospace().size(11.0), |ui| {
        let label = RichText::new(copy).monospace().size(11.0);
        clicked = ui
            .add(egui::Button::new(label).frame_when_inactive(false))
            .clicked();
    });
    clicked
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
                    ui.add(
                        egui::Label::new(egui::RichText::new(text).color(palette.current)).extend(),
                    );
                });
        });
}

/// The width a small button with `label` takes.
pub fn small_button_width(ui: &Ui, label: &str) -> f32 {
    label_width(ui, label, &FontId::monospace(10.0)) + 14.0
}

/// A text button's width, to leave room for one.
pub fn text_button_width(ui: &Ui, label: &str) -> f32 {
    label_width(ui, label, &FontId::monospace(11.0)) + 20.0
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
        palette.button_lit()
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
        // As the Android app draws them, on a 24-unit square.
        Glyph::Shuffle | Glyph::Repeat => {
            let at = |(x, y): (f32, f32)| pos2(c.x + (x - 12.0) * 0.6, c.y + (y - 12.0) * 0.6);
            let arm = |points: &[(f32, f32)]| {
                Shape::line(
                    points.iter().copied().map(at).collect(),
                    egui::Stroke::new(1.5, color),
                )
            };
            let head = |points: [(f32, f32); 3]| {
                Shape::convex_polygon(points.map(at).to_vec(), color, egui::Stroke::NONE)
            };
            if let Glyph::Shuffle = glyph {
                // Two arrows crossing on their way right.
                painter.add(arm(&[(3.5, 7.0), (8.0, 7.0), (14.5, 17.0), (17.0, 17.0)]));
                painter.add(head([(16.0, 13.0), (21.0, 17.0), (16.0, 21.0)]));
                painter.add(arm(&[(3.5, 17.0), (8.0, 17.0), (14.5, 7.0), (17.0, 7.0)]));
                painter.add(head([(16.0, 3.0), (21.0, 7.0), (16.0, 11.0)]));
            } else {
                // Two arrows chasing each other round a loop.
                painter.add(arm(&[(4.5, 13.0), (4.5, 8.0), (16.0, 8.0)]));
                painter.add(head([(15.5, 4.0), (20.5, 8.0), (15.5, 12.0)]));
                painter.add(arm(&[(19.5, 11.0), (19.5, 16.0), (8.0, 16.0)]));
                painter.add(head([(8.5, 12.0), (3.5, 16.0), (8.5, 20.0)]));
            }
        }
    }
}

/// A heart `width` points wide, centred on `c`: filled, or its outline.
pub fn paint_heart(painter: &egui::Painter, c: Pos2, width: f32, color: Color32, filled: bool) {
    use std::f32::consts::PI;
    // Two lobes, circles whose outer edges are the heart's sides, and the
    // lines from the tip that touch them.
    let r = width * 0.26;
    let d = width / 2.0 - r;
    let height = width * 0.88;
    let lobes = c.y - height / 2.0 + r;
    let tip = pos2(c.x, c.y + height / 2.0);
    let right = pos2(c.x + d, lobes);
    let to_lobe = right - tip;
    let side = to_lobe.angle() + (r / to_lobe.length()).asin();
    let reach = (to_lobe.length_sq() - r * r).sqrt();
    let touch = tip + reach * Vec2::angled(side);
    if filled {
        // Each part antialiased on its own, with no sharp corner but the tip.
        painter.circle_filled(pos2(c.x - d, lobes), r, color);
        painter.circle_filled(right, r, color);
        let left_touch = pos2(2.0 * c.x - touch.x, touch.y);
        painter.add(Shape::convex_polygon(
            vec![tip, left_touch, pos2(c.x - d, lobes), right, touch],
            color,
            egui::Stroke::NONE,
        ));
        return;
    }
    // The right lobe from the cleft, over the top, to where the side
    // leaves it; the left is its mirror.
    let cleft = (-(r * r - d * d).sqrt()).atan2(-d);
    let leave = side + PI / 2.0;
    let steps = 12;
    let arc: Vec<Pos2> = (0..=steps)
        .map(|i| right + r * Vec2::angled(cleft + (leave - cleft) * i as f32 / steps as f32))
        .collect();
    let mut points: Vec<Pos2> = arc
        .iter()
        .rev()
        .map(|p| pos2(2.0 * c.x - p.x, p.y))
        .collect();
    points.extend(arc.iter().skip(1));
    points.push(tip);
    painter.add(Shape::closed_line(points, egui::Stroke::new(1.0, color)));
}

/// A list row's song length, against `rect`'s right end; gives where
/// its column starts, the same for every length up to 99:59 so the
/// hearts and marks before it line up.
pub fn row_length(painter: &egui::Painter, rect: Rect, ms: u32, color: Color32) -> f32 {
    let mono = FontId::monospace(11.0);
    let seconds = ms / 1000;
    let right = rect.right() - 6.0;
    let drawn = painter.text(
        pos2(right, rect.center().y),
        Align2::RIGHT_CENTER,
        format!("{}:{:02}", seconds / 60, seconds % 60),
        mono.clone(),
        color,
    );
    let widest = painter.layout_no_wrap("00:00".into(), mono, color).size().x;
    drawn.left().min(right - widest)
}

/// A list row's heart for a song loved on Last.fm, its right edge at
/// `right`; gives where it starts.
pub fn row_heart(painter: &egui::Painter, right: f32, y: f32, color: Color32) -> f32 {
    const WIDTH: f32 = 9.0;
    paint_heart(painter, pos2(right - WIDTH / 2.0, y), WIDTH, color, true);
    right - WIDTH
}
