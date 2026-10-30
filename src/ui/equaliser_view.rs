//! The equaliser below the player (EQ), as Winamp's: ON, the presets and
//! RESET, the preamp and the ten bands, and their curve on a display.
//!
//! A skin with Winamp's equaliser art draws ON, PRESETS, the sliders and
//! their thumbs from it; anything it lacks is drawn as Classic's.

use egui::emath::GuiRounding;
use egui::{
    Align2, FontId, Id, Key, Painter, Pos2, Rect, Response, RichText, Sense, Shape, Stroke, Ui,
    UiBuilder, Vec2, WidgetInfo, pos2, vec2,
};

use super::skin_paint::SkinPaint;
use super::widgets::{field, small_button, text_button};
use super::{Inputs, View, skinned_strip, theme};
use crate::action::{Action, Actions};
use crate::playback::equaliser::{Equaliser, PRESETS, RANGE};
use crate::skin::format::{Role, State};

/// What the bands are called under their sliders, by frequency.
const LABELS: [&str; 10] = [
    "60", "170", "310", "600", "1K", "3K", "6K", "12K", "14K", "16K",
];
/// What a band is called when the pointer is on it.
const NAMES: [&str; 10] = [
    "60 Hz", "170 Hz", "310 Hz", "600 Hz", "1 kHz", "3 kHz", "6 kHz", "12 kHz", "14 kHz", "16 kHz",
];

/// Space above the sliders and below their labels.
const PADDING: f32 = 8.0;
/// Room under the sliders for their labels.
const LABEL_ROOM: f32 = 18.0;
/// The switches' column, at the left.
const SWITCHES: f32 = 90.0;
/// A slider's column at its widest and narrowest.
const COLUMN: f32 = 44.0;
const MIN_COLUMN: f32 = 26.0;
/// The dB scale between the preamp and the bands.
const SCALE: f32 = 42.0;
/// The curve's display, at its narrowest, and the gap before it.
const MIN_DISPLAY: f32 = 150.0;
const DISPLAY_GAP: f32 = 18.0;
/// Classic's sliders: their height, rail and thumb.
const SLIDER_HEIGHT: f32 = 118.0;
const RAIL_WIDTH: f32 = 8.0;
const THUMB: Vec2 = vec2(16.0, 10.0);

/// The skin's equaliser art, when it has the sliders', at the strip's
/// scale for a section `width` points wide.
fn skinned<'a>(view: &View<'a>, width: f32) -> Option<(&'a SkinPaint, f32)> {
    let paint = view
        .art
        .filter(|paint| paint.has("equaliser.slider") && paint.has("equaliser.thumb"))?;
    Some((paint, skinned_strip::unit(paint, width)))
}

fn slider_height(view: &View, width: f32) -> f32 {
    skinned(view, width)
        .and_then(|(paint, unit)| paint.size("equaliser.slider", unit))
        .map_or(SLIDER_HEIGHT, |size| size.y)
}

/// The section's height below its title bar, `width` points wide.
pub fn height(view: &View, width: f32) -> f32 {
    PADDING + slider_height(view, width) + LABEL_ROOM + PADDING
}

pub fn draw(ui: &mut Ui, rect: Rect, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
    let palette = view.palette;
    let equaliser = view.settings.equaliser;
    let skin = skinned(view, rect.width());
    let painter = ui.painter().with_clip_rect(rect);
    let top = rect.top() + PADDING;
    let height = slider_height(view, rect.width());

    let unit = view
        .art
        .map(|paint| skinned_strip::unit(paint, rect.width()));
    switches(
        ui,
        &painter,
        pos2(rect.left(), top),
        unit,
        view,
        inputs,
        actions,
    );

    // The display takes what the sliders leave, if that's enough for it.
    let sliders = rect.width() - SWITCHES - SCALE;
    let wide = 11.0 * COLUMN + DISPLAY_GAP + MIN_DISPLAY;
    let column = if sliders >= wide {
        COLUMN
    } else {
        (sliders / 11.0).clamp(MIN_COLUMN, COLUMN)
    };
    let preamp_x = rect.left() + SWITCHES;
    let scale_x = preamp_x + column;
    let bands_x = scale_x + SCALE;
    let bands_end = bands_x + 10.0 * column;
    let track = Rect::from_min_max(pos2(preamp_x, top), pos2(bands_end, top + height));
    let thumb = skin
        .and_then(|(paint, unit)| paint.size("equaliser.thumb", unit))
        .unwrap_or(THUMB);
    let reach = (height - thumb.y) / 2.0;
    let zero = track.center().y;

    // The dB scale, level with where a thumb's middle is at each.
    for (text, y) in [("+12", zero - reach), ("0", zero), ("-12", zero + reach)] {
        painter.text(
            pos2(bands_x - 8.0, y),
            Align2::RIGHT_CENTER,
            text,
            FontId::monospace(9.0),
            palette.dim,
        );
    }
    if skin.is_none() {
        painter.line_segment(
            [pos2(bands_x - 4.0, zero), pos2(bands_end, zero)],
            Stroke::new(1.0, palette.hi.gamma_multiply(0.6)),
        );
    }

    let mut changed = equaliser;
    let column_rect = |x: f32| Rect::from_min_size(pos2(x, top), vec2(column, height));
    let labels_y = top + height + LABEL_ROOM / 2.0 + 2.0;
    let label = |x: f32, text: &str| {
        painter.text(
            pos2(x + column / 2.0, labels_y),
            Align2::CENTER_CENTER,
            text,
            FontId::monospace(9.0),
            palette.chrome,
        );
    };
    if let Some(db) = slider(
        ui,
        &painter,
        view,
        skin,
        column_rect(preamp_x),
        "Preamp",
        equaliser.preamp,
    ) {
        changed.preamp = db;
    }
    label(preamp_x, "PREAMP");
    for (n, (text, name)) in LABELS.iter().zip(NAMES).enumerate() {
        let x = bands_x + n as f32 * column;
        if let Some(db) = slider(
            ui,
            &painter,
            view,
            skin,
            column_rect(x),
            name,
            equaliser.bands[n],
        ) {
            changed.bands[n] = db;
        }
        label(x, text);
    }
    if changed != equaliser {
        // A slider moved while off turns the equaliser on, to be heard.
        actions.push(Action::SetEqualiser(Equaliser {
            on: true,
            ..changed
        }));
    }

    let display = Rect::from_min_max(
        pos2(bands_end + DISPLAY_GAP, top),
        pos2(rect.right(), top + height),
    );
    if display.width() >= MIN_DISPLAY {
        curve(&painter, display, view, skin);
    }
    save_preset(ui, view, inputs, actions);
}

/// ON, PRESETS and RESET, down the left.
fn switches(
    ui: &mut Ui,
    painter: &Painter,
    at: Pos2,
    unit: Option<f32>,
    view: &View,
    inputs: &mut Inputs,
    actions: &mut Actions,
) {
    let palette = view.palette;
    let equaliser = view.settings.equaliser;
    let mut y = at.y;
    // A skin's sprite where it has one, else a small button.
    let mut button = |ui: &mut Ui, name: &str, label: &str, tip: &str, on: bool| -> Response {
        let response = match (view.art.filter(|paint| paint.has(name)), unit) {
            (Some(paint), Some(unit)) => {
                let state = if on { State::On } else { State::Normal };
                let response = skinned_strip::button(
                    ui,
                    painter,
                    paint,
                    pos2(at.x, y),
                    unit,
                    name,
                    state,
                    tip,
                    true,
                );
                y = response.rect.bottom() + 6.0;
                response
            }
            _ => {
                let area = Rect::from_min_size(pos2(at.x, y), vec2(SWITCHES, 24.0));
                let response = ui
                    .scope_builder(UiBuilder::new().max_rect(area), |ui| {
                        small_button(ui, &palette, label, on)
                    })
                    .inner;
                y = response.rect.bottom() + 6.0;
                response
            }
        };
        response.on_hover_text(tip)
    };
    let on = button(ui, "equaliser.on", "ON", "Equaliser on / off", equaliser.on);
    if on.clicked() {
        actions.push(Action::SetEqualiser(Equaliser {
            on: !equaliser.on,
            ..equaliser
        }));
    }
    let presets = button(ui, "equaliser.presets", "PRESETS ▾", "Presets", false);
    presets_menu(&presets, view, inputs, actions);
    let reset = button(
        ui,
        "equaliser.reset",
        "RESET",
        "Every band and the preamp back to 0",
        false,
    );
    if reset.clicked() {
        actions.push(Action::SetEqualiser(Equaliser {
            preamp: 0.0,
            bands: [0.0; 10],
            ..equaliser
        }));
    }
}

/// Winamp's presets, the user's, and saving the bands as one.
fn presets_menu(button: &Response, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
    let equaliser = view.settings.equaliser;
    let choose = |actions: &mut Actions, bands: [f32; 10]| {
        actions.push(Action::SetEqualiser(Equaliser {
            on: true,
            bands,
            ..equaliser
        }));
    };
    let item = |ui: &mut Ui, name: &str| {
        ui.add(
            egui::Button::new(RichText::new(name).monospace().size(11.0))
                .frame_when_inactive(false),
        )
    };
    egui::Popup::menu(button).show(|ui| {
        egui::Grid::new("eq-presets")
            .num_columns(2)
            .spacing(vec2(16.0, 2.0))
            .show(ui, |ui| {
                for (n, (name, bands)) in PRESETS.iter().enumerate() {
                    if item(ui, name).clicked() {
                        choose(actions, *bands);
                    }
                    if n % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
        let saved = &view.settings.equaliser_presets;
        if !saved.is_empty() {
            ui.separator();
            for preset in saved {
                ui.horizontal(|ui| {
                    if item(ui, &preset.name).clicked() {
                        choose(actions, preset.bands);
                    }
                    let delete = item(ui, "×").on_hover_text("Delete this preset");
                    if delete.clicked() {
                        actions.push(Action::DeleteEqualiserPreset(preset.name.clone()));
                    }
                });
            }
        }
        ui.separator();
        if item(ui, "Save the bands as…").clicked() {
            let current = saved.iter().find(|preset| preset.bands == equaliser.bands);
            inputs.preset_name = Some(
                current
                    .map(|preset| preset.name.clone())
                    .unwrap_or_default(),
            );
        }
    });
}

/// Asks for the name to save the bands under.
fn save_preset(ui: &Ui, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
    let Some(name) = &mut inputs.preset_name else {
        return;
    };
    let palette = view.palette;
    let mut done = None;
    egui::Modal::new(Id::new("save-eq-preset"))
        .frame(
            egui::Frame::new()
                .fill(palette.frame)
                .stroke(Stroke::new(1.0, palette.hi))
                .inner_margin(16),
        )
        .show(ui.ctx(), |ui| {
            ui.set_max_width(320.0);
            ui.label(
                RichText::new("Save the bands as a preset")
                    .size(15.0)
                    .color(palette.current),
            );
            ui.add_space(6.0);
            let response = field(
                ui,
                egui::TextEdit::singleline(name)
                    .id(Id::new("eq-preset-name"))
                    .hint_text("Name")
                    .desired_width(280.0),
            );
            if !response.has_focus() && !response.lost_focus() {
                response.request_focus();
            }
            // Its Enter, not the playlist's too: that would play the song.
            let entered = response.lost_focus()
                && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let can = !name.trim().is_empty();
                let save =
                    super::widgets::tip(text_button(ui, &palette, "SAVE", false, can), "Enter");
                if save.clicked() || (entered && can) {
                    done = Some(true);
                }
                if text_button(ui, &palette, "CANCEL", false, true)
                    .on_hover_text("Esc")
                    .clicked()
                    || ui.input(|i| i.key_pressed(Key::Escape))
                {
                    done = Some(false);
                }
            });
        });
    match done {
        Some(true) => {
            actions.push(Action::SaveEqualiserPreset(name.trim().to_string()));
            inputs.preset_name = None;
        }
        Some(false) => inputs.preset_name = None,
        None => {}
    }
}

/// A slider from -12 to +12 dB in `rect`: dragged or clicked to a value,
/// double-clicked back to 0. Gives the value it was moved to.
fn slider(
    ui: &Ui,
    painter: &Painter,
    view: &View,
    skin: Option<(&SkinPaint, f32)>,
    rect: Rect,
    name: &str,
    db: f32,
) -> Option<f32> {
    let response = ui.interact(rect, Id::new(("eq-slider", name)), Sense::click_and_drag());
    let thumb = skin
        .and_then(|(paint, unit)| paint.size("equaliser.thumb", unit))
        .unwrap_or(THUMB);
    let top = rect.top() + thumb.y / 2.0;
    let travel = rect.height() - thumb.y;
    let y_of = |db: f32| top + (RANGE - db) / (2.0 * RANGE) * travel;
    let mut moved = None;
    if response.double_clicked() {
        moved = Some(0.0);
    } else if response.is_pointer_button_down_on()
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let fraction = ((pointer.y - top) / travel).clamp(0.0, 1.0);
        let value = ((RANGE - fraction * 2.0 * RANGE) * 10.0).round() / 10.0;
        moved = (value != db).then_some(value);
    }
    let shown = moved.unwrap_or(db);
    response.widget_info(|| WidgetInfo::slider(true, f64::from(shown), name));
    let response = response.on_hover_text(format!("{name}: {shown:+.1} dB"));
    let centre = rect.center().x;
    let thumb_rect = Rect::from_center_size(pos2(centre, y_of(shown)), thumb)
        .round_to_pixels(ui.pixels_per_point());
    match skin {
        Some((paint, unit)) => {
            // Winamp's slider shows the level by its colour as well.
            let size = paint.size("equaliser.slider", unit).unwrap_or(rect.size());
            let frame =
                Rect::from_center_size(rect.center(), size).round_to_pixels(ui.pixels_per_point());
            let level = (shown + RANGE) / (2.0 * RANGE);
            paint.paint_level(painter, "equaliser.slider", level, frame, unit);
            let state = if response.is_pointer_button_down_on() {
                State::Pressed
            } else {
                State::Normal
            };
            paint.paint(painter, "equaliser.thumb", state, thumb_rect, unit);
        }
        None => {
            let palette = view.palette;
            let rail = Rect::from_center_size(rect.center(), vec2(RAIL_WIDTH, rect.height()));
            painter.rect_filled(rail, 0.0, palette.button);
            // Filled from 0 dB, up for a boost and down for a cut.
            let (zero, at) = (y_of(0.0), y_of(shown));
            let fill = Rect::from_min_max(
                pos2(rail.left(), zero.min(at)),
                pos2(rail.right(), zero.max(at)),
            );
            painter.rect_filled(fill, 0.0, palette.selected_bg);
            let visuals = ui.style().interact(&response);
            painter.rect_filled(thumb_rect, 0.0, visuals.bg_fill);
            painter.rect_stroke(thumb_rect, 0.0, visuals.fg_stroke, egui::StrokeKind::Inside);
        }
    }
    moved
}

/// The preset's name and the preamp over the bands' curve, on a display.
fn curve(painter: &Painter, rect: Rect, view: &View, skin: Option<(&SkinPaint, f32)>) {
    let palette = view.palette;
    let equaliser = view.settings.equaliser;
    let framed = skin
        .is_some_and(|(paint, unit)| paint.paint(painter, "display", State::Normal, rect, unit));
    if !framed {
        theme::inset(painter, rect, palette.lcd_bg, &palette);
    }
    let lit = if equaliser.on {
        palette.lcd
    } else {
        palette.dim
    };
    let name = preset_name(view).to_uppercase();
    let name = if equaliser.on {
        name
    } else {
        format!("{name} (OFF)")
    };
    let preamp = format!("PREAMP {:+.1} DB", equaliser.preamp);
    let text_at = rect.left_top() + vec2(8.0, 7.0);
    match view.art.filter(|paint| paint.has_player()) {
        Some(paint) => {
            let unit = skin.map_or(1.5, |(_, unit)| unit);
            let width = rect.width() - 16.0;
            paint.text(painter, Role::Display, &name, text_at, width, unit, lit);
            let preamp_width = preamp.chars().count() as f32 * paint.cell(Role::Display).x * unit;
            let at = pos2(rect.right() - 8.0 - preamp_width, text_at.y);
            if at.x > text_at.x + name.chars().count() as f32 * paint.cell(Role::Display).x * unit {
                paint.text(
                    painter,
                    Role::Display,
                    &preamp,
                    at,
                    preamp_width,
                    unit,
                    palette.dim,
                );
            }
        }
        None => {
            painter.text(
                text_at,
                Align2::LEFT_TOP,
                &name,
                FontId::monospace(12.0),
                lit,
            );
            painter.text(
                pos2(rect.right() - 8.0, text_at.y + 1.0),
                Align2::RIGHT_TOP,
                &preamp,
                FontId::monospace(9.0),
                palette.dim,
            );
        }
    }

    let graph = Rect::from_min_max(
        pos2(rect.left() + 10.0, rect.top() + 26.0),
        pos2(rect.right() - 10.0, rect.bottom() - 10.0),
    );
    if graph.height() < 10.0 {
        return;
    }
    let grid = Stroke::new(1.0, palette.dim.gamma_multiply(0.35));
    for i in 0..=4 {
        let y = graph.top() + graph.height() * i as f32 / 4.0;
        painter.line_segment([pos2(graph.left(), y), pos2(graph.right(), y)], grid);
    }
    for i in 0..10 {
        let x = graph.left() + graph.width() * i as f32 / 9.0;
        painter.line_segment([pos2(x, graph.top()), pos2(x, graph.bottom())], grid);
    }
    let points: Vec<Pos2> = equaliser
        .bands
        .iter()
        .enumerate()
        .map(|(i, db)| {
            pos2(
                graph.left() + graph.width() * i as f32 / 9.0,
                graph.center().y - db / RANGE * graph.height() / 2.0,
            )
        })
        .collect();
    painter.add(Shape::line(smooth(&points), Stroke::new(1.5, lit)));
    for point in &points {
        painter.circle_filled(*point, 2.0, lit);
    }
}

/// A smooth line through `points` (Catmull-Rom).
fn smooth(points: &[Pos2]) -> Vec<Pos2> {
    const STEPS: usize = 12;
    let mut line = Vec::with_capacity(points.len() * STEPS);
    for i in 0..points.len().saturating_sub(1) {
        let p0 = points[i.saturating_sub(1)];
        let (p1, p2) = (points[i], points[i + 1]);
        let p3 = points[(i + 2).min(points.len() - 1)];
        for step in 0..STEPS {
            let t = step as f32 / STEPS as f32;
            let (t2, t3) = (t * t, t * t * t);
            let at = |a: f32, b: f32, c: f32, d: f32| {
                0.5 * (2.0 * b
                    + (c - a) * t
                    + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2
                    + (3.0 * b - a - 3.0 * c + d) * t3)
            };
            line.push(pos2(at(p0.x, p1.x, p2.x, p3.x), at(p0.y, p1.y, p2.y, p3.y)));
        }
    }
    line.extend(points.last());
    line
}

/// The preset the bands are set to: the user's first, then Winamp's;
/// Custom when they match none.
fn preset_name(view: &View) -> String {
    let bands = view.settings.equaliser.bands;
    let saved = view.settings.equaliser_presets.iter();
    saved
        .map(|preset| (preset.name.as_str(), preset.bands))
        .chain(PRESETS)
        .find(|(_, preset)| *preset == bands)
        .map_or_else(|| "Custom".into(), |(name, _)| name.to_string())
}
