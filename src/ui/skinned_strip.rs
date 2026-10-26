//! The player strip drawn from a skin's pieces: its transport buttons,
//! the time in its digits, the song in its font, its seek bar, shuffle,
//! repeat, volume and PL. Laid out as the plain strip is, every size taken
//! from the skin's own pieces. The title bars are the skin's too.

use std::time::Duration;

use egui::emath::GuiRounding;
use egui::{
    Align2, FontId, Id, Painter, Pos2, Rect, Response, Sense, Ui, WidgetInfo, WidgetType, pos2,
    vec2,
};

use super::player_strip::{
    HEIGHT, display_clicked, eject_clicked, eq_clicked, follow_press, held, length, now_lines,
    pl_clicked, rates, time_shown, vis_clicked,
};
use super::skin_paint::SkinPaint;
use super::{Inputs, View, theme};
use crate::action::{Action, Actions};
use crate::backend::SessionState;
use crate::playback::engine::Status;
use crate::skin::format::{Role, State};

/// Gaps, in image pixels: between groups of controls, between the
/// display's two lines, and between the rows.
const GAP: f32 = 6.0;
const LINE_GAP: f32 = 3.0;
const ROW_GAP: f32 = 4.0;
/// A frame's padding when the skin draws no frame there.
const PADDING: [f32; 4] = [3.0, 3.0, 3.0, 3.0];
/// Points between the time's box and what's inside it.
const LCD_MARGIN: f32 = 10.0;
/// The narrowest strip that keeps everything at full size, in image
/// pixels.
const MIN_WIDTH: f32 = 300.0;
/// What separates the end of a scrolling song from its start again.
const MARQUEE_GAP: &str = "  ***  ";
/// How long the marquee waits between one-character steps, in ms.
const MARQUEE_STEP: u64 = 220;
const TRANSPORT: [(&str, &str, Action); 5] = [
    ("previous", "Previous", Action::Previous),
    ("play", "Play", Action::Play),
    ("pause", "Pause", Action::TogglePause),
    ("stop", "Stop", Action::Stop),
    ("next", "Next", Action::Next),
];

/// The strip's rows in image pixels, from the skin's pieces.
struct Rows {
    display: f32,
    info_y: f32,
    info: f32,
    top: f32,
    seek_y: f32,
    seek: f32,
    buttons_y: f32,
    buttons: f32,
    total: f32,
}

impl Rows {
    fn of(paint: &SkinPaint) -> Self {
        let height = |name: &str| paint.size(name, 1.0).map_or(0.0, |size| size.y);
        let line = paint.cell(Role::Display).y;
        let [_, top, _, bottom] = padding(paint, "display");
        let display = top + 2.0 * line + LINE_GAP + bottom;
        let [_, small_top, _, small_bottom] = padding(paint, "small_display");
        let info = (small_top + line + small_bottom)
            .max(height("volume.track"))
            .max(height("volume.thumb"))
            .max(height("pl"));
        let info_y = display + ROW_GAP;
        let top = info_y + info;
        let seek_y = top + ROW_GAP + 1.0;
        let seek = height("seek.track").max(height("seek.thumb"));
        let buttons_y = seek_y + seek + GAP;
        let buttons = [
            "previous", "play", "pause", "stop", "next", "eject", "shuffle", "repeat",
        ]
        .into_iter()
        .map(height)
        .fold(0.0, f32::max);
        Self {
            display,
            info_y,
            info,
            top,
            seek_y,
            seek,
            buttons_y,
            buttons,
            total: buttons_y + buttons,
        }
    }
}

/// A frame's padding in image pixels, or the default where the skin has
/// no frame.
fn padding(paint: &SkinPaint, name: &str) -> [f32; 4] {
    match paint.size(name, 1.0) {
        Some(size) => {
            let content = paint
                .content(name, Rect::from_min_size(Pos2::ZERO, size), 1.0)
                .unwrap_or(Rect::ZERO);
            [
                content.left(),
                content.top(),
                size.x - content.right(),
                size.y - content.bottom(),
            ]
        }
        None => PADDING,
    }
}

/// Points per image pixel for a strip `width` points wide: the skin's
/// scale, or smaller when the window is too narrow or the strip too tall.
pub fn unit(paint: &SkinPaint, width: f32) -> f32 {
    let rows = Rows::of(paint).total.max(1.0);
    paint
        .scale()
        .min(width / MIN_WIDTH)
        .min((HEIGHT - 2.0) / rows)
}

/// A title bar: the skin's stripes across it, and its plate behind the
/// name in the middle.
pub fn title_bar(ui: &Ui, rect: Rect, view: &View, paint: &SkinPaint, unit: f32, label: &str) {
    let lit = ui.input(|i| i.viewport().focused.unwrap_or(true));
    let state = if lit { State::Normal } else { State::Unlit };
    let painter = ui.painter().with_clip_rect(rect);
    let name = if label == super::TITLE {
        "titlebar"
    } else {
        "separator"
    };
    paint.paint(&painter, name, state, rect, unit);
    let text = painter.layout_no_wrap(label.into(), FontId::monospace(10.0), view.palette.title);
    if let Some(plate) = paint.size("title", unit) {
        let gap = paint.title_gap() * unit;
        let ends = paint
            .content("title", Rect::from_min_size(Pos2::ZERO, plate), unit)
            .map_or(0.0, |inner| plate.x - inner.width());
        let width = text.size().x + 2.0 * gap + ends;
        let plate = Rect::from_center_size(rect.center(), vec2(width, plate.y))
            .round_to_pixels(ui.pixels_per_point());
        paint.paint(&painter, "title", state, plate, unit);
    }
    let at = rect.center() - text.size() / 2.0;
    painter.galley(at, text, view.palette.title);
}

/// Where the strip's top left corner goes in `rect`, at `unit`.
fn origin(ui: &Ui, rect: Rect, rows: &Rows, unit: f32) -> Pos2 {
    pos2(
        rect.left(),
        rect.top() + (rect.height() - rows.total * unit) / 2.0,
    )
    .round_to_pixels(ui.pixels_per_point())
}

/// The buttons' row in the strip `rect`, the points per pixel they're
/// drawn at, and how wide they are together.
pub fn keys(ui: &Ui, paint: &SkinPaint, rect: Rect) -> (Rect, f32, f32) {
    let unit = unit(paint, rect.width());
    let rows = Rows::of(paint);
    let origin = origin(ui, rect, &rows, unit);
    let row = Rect::from_min_max(
        pos2(rect.left(), origin.y + rows.buttons_y * unit),
        pos2(
            rect.right(),
            origin.y + (rows.buttons_y + rows.buttons) * unit,
        ),
    );
    let width = |name: &str| paint.size(name, unit).map_or(0.0, |size| size.x);
    let buttons: f32 = [
        "previous", "play", "pause", "stop", "next", "eject", "shuffle", "repeat",
    ]
    .into_iter()
    .map(width)
    .sum();
    (row, unit, buttons + 2.0 * GAP * unit)
}

pub fn draw(
    ui: &mut Ui,
    view: &View,
    paint: &SkinPaint,
    inputs: &mut Inputs,
    actions: &mut Actions,
    with_buttons: bool,
) {
    let rect = ui.max_rect();
    let unit = unit(paint, rect.width());
    let rows = Rows::of(paint);
    let origin = origin(ui, rect, &rows, unit);
    let y = |pixels: f32| origin.y + pixels * unit;

    let lcd_width = time_width(paint) * unit + 2.0 * LCD_MARGIN;
    let lcd = Rect::from_min_size(origin, vec2(lcd_width, rows.top * unit));
    draw_lcd(ui, lcd, view, paint, unit);
    let title = Rect::from_min_max(
        pos2(lcd.right() + GAP * unit, origin.y),
        pos2(rect.right(), y(rows.display)),
    );
    draw_title(ui, title, view, paint, unit);
    display_clicked(ui, title, view, inputs, actions);
    let info = Rect::from_min_max(
        pos2(title.left(), y(rows.info_y)),
        pos2(rect.right(), y(rows.info_y + rows.info)),
    );
    info_row(ui, info, view, paint, unit, inputs, actions);
    let seek = Rect::from_min_max(
        pos2(rect.left(), y(rows.seek_y)),
        pos2(rect.right(), y(rows.seek_y + rows.seek)),
    );
    seek_bar(ui, seek, view, paint, unit, inputs, actions);
    let buttons = Rect::from_min_max(
        pos2(rect.left(), y(rows.buttons_y)),
        pos2(rect.right(), y(rows.buttons_y + rows.buttons)),
    );
    if with_buttons {
        buttons_row(ui, buttons, view, paint, unit, inputs, actions);
    }
}

/// Where the time's parts go from the status icon's left, in image
/// pixels, at Winamp's spacing: the four digits, the colon; and how wide
/// it all is.
fn time_layout(paint: &SkinPaint) -> ([f32; 4], f32, f32) {
    let status = paint.size("status.playing", 1.0).map_or(9.0, |size| size.x);
    let digit = paint.cell(Role::Digits).x;
    let first = status + 11.0;
    let second = first + digit + 3.0;
    let colon = second + digit + 3.0;
    let third = colon + 6.0;
    let fourth = third + digit + 3.0;
    ([first, second, third, fourth], colon, fourth + digit)
}

fn time_width(paint: &SkinPaint) -> f32 {
    time_layout(paint).2
}

/// The LCD: the status icon and the time.
fn draw_lcd(ui: &Ui, rect: Rect, view: &View, paint: &SkinPaint, unit: f32) {
    let palette = view.palette;
    let painter = ui.painter().with_clip_rect(rect);
    if !paint.paint(&painter, "lcd", State::Normal, rect, unit) {
        theme::inset(&painter, rect, palette.lcd_bg, &palette);
    }
    let player = view.player;
    let height = paint.cell(Role::Digits).y;
    let origin = pos2(
        rect.left() + LCD_MARGIN,
        rect.top() + (rect.height() - height * unit) / 2.0,
    )
    .round_to_pixels(ui.pixels_per_point());
    let at = |x: f32, y: f32| origin + vec2(x, y) * unit;

    let status = match player.status {
        Status::Playing | Status::Loading => "status.playing",
        Status::Paused => "status.paused",
        Status::Stopped => "status.stopped",
    };
    let icon = paint.size(status, 1.0).map_or(0.0, |size| size.y);
    paint.paint_at(
        &painter,
        status,
        State::Normal,
        at(0.0, (height - icon) / 2.0),
        unit,
    );

    // Stopped, the time is blank, as in Winamp.
    if player.status == Status::Stopped {
        return;
    }
    let seconds = player.position_now() / 1000;
    let minutes = (seconds / 60).min(99);
    let values = [minutes / 10, minutes % 10, seconds % 60 / 10, seconds % 10];
    let (digits, colon, _) = time_layout(paint);
    if time_shown(ui, view) {
        let width = paint.cell(Role::Digits).x * unit;
        for (x, value) in digits.into_iter().zip(values) {
            let text = char::from_digit(value, 10).unwrap_or('0').to_string();
            paint.text(
                &painter,
                Role::Digits,
                &text,
                at(x, 0.0),
                width,
                unit,
                palette.lcd,
            );
        }
    }
    // The colon is part of Winamp's main window, not of the digits.
    for y in [0.3, 0.6] {
        let dot = Rect::from_min_size(at(colon, (height * y).round()), vec2(3.0, 1.0) * unit);
        painter.rect_filled(dot, 0.0, palette.lcd);
    }
}

/// The song's display: the artist above, the song below, each scrolling
/// on its own when too long.
fn draw_title(ui: &Ui, rect: Rect, view: &View, paint: &SkinPaint, unit: f32) {
    let palette = view.palette;
    let painter = ui.painter().with_clip_rect(rect);
    if !paint.paint(&painter, "display", State::Normal, rect, unit) {
        theme::inset(&painter, rect, palette.lcd_bg, &palette);
    }
    let [left, top, right, _] = padding(paint, "display").map(|n| n * unit);
    let origin = pos2(rect.left() + left, rect.top() + top).round_to_pixels(ui.pixels_per_point());
    let width = rect.right() - right - origin.x;
    let line = (paint.cell(Role::Display).y + LINE_GAP) * unit;
    let (artist, song) = now_lines(view);
    for (index, text) in [artist, song].into_iter().enumerate() {
        let shown = marquee(ui, view, paint, &text, width, unit);
        let at = origin + vec2(0.0, index as f32 * line);
        paint.text(
            &painter,
            Role::Display,
            &shown,
            at,
            width,
            unit,
            palette.lcd,
        );
    }
}

/// A small display with `text` in it, its corner at `at`; gives its rect.
fn small_display(
    painter: &Painter,
    view: &View,
    paint: &SkinPaint,
    at: Pos2,
    text: &str,
    unit: f32,
) -> Rect {
    let palette = view.palette;
    let cell = paint.cell(Role::Display);
    let [left, top, right, bottom] = padding(paint, "small_display");
    let width = text.chars().count() as f32 * cell.x;
    let size = vec2(left + width + right, top + cell.y + bottom) * unit;
    let rect = Rect::from_min_size(at, size).round_to_pixels(painter.pixels_per_point());
    if !paint.paint(painter, "small_display", State::Normal, rect, unit) {
        theme::inset(painter, rect, palette.lcd_bg, &palette);
    }
    let text_at = rect.min + vec2(left, top) * unit;
    paint.text(
        painter,
        Role::Display,
        text,
        text_at,
        width * unit,
        unit,
        palette.lcd,
    );
    rect
}

/// Under the song's display: the bitrate in small displays, then the
/// volume, EQ and PL at the right end.
fn info_row(
    ui: &mut Ui,
    rect: Rect,
    view: &View,
    paint: &SkinPaint,
    unit: f32,
    inputs: &mut Inputs,
    actions: &mut Actions,
) {
    let palette = view.palette;
    let painter = ui.painter().clone();
    let mut x = rect.left();
    let small = paint.cell(Role::Display).y + {
        let [_, top, _, bottom] = padding(paint, "small_display");
        top + bottom
    };
    for (value, label) in rates(view).into_iter().zip(["kbps", "kHz"]) {
        let at = pos2(x, rect.center().y - small * unit / 2.0);
        let display = small_display(&painter, view, paint, at, value, unit);
        let text = painter.text(
            pos2(display.right() + 3.0 * unit, display.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::monospace(9.0),
            palette.chrome,
        );
        x = text.right() + GAP * unit;
    }
    let Some(pl) = paint.size("pl", unit) else {
        return;
    };
    let pl_at = pos2(rect.right() - pl.x, rect.center().y - pl.y / 2.0);
    let state = toggle_state(view.settings.playlists);
    let response = button(
        ui,
        &painter,
        paint,
        pl_at,
        unit,
        "pl",
        state,
        "Playlists",
        true,
    );
    pl_clicked(response, view, actions);
    // EQ beside PL, as on Winamp's main window, where the skin has it.
    let mut left = pl_at.x;
    if let Some(eq) = paint.size("eq", unit) {
        let eq_at = pos2(left - eq.x, rect.center().y - eq.y / 2.0);
        let state = toggle_state(view.settings.equaliser_shown);
        let response = button(
            ui,
            &painter,
            paint,
            eq_at,
            unit,
            "eq",
            state,
            "Equaliser",
            true,
        );
        eq_clicked(response, view, actions);
        left = eq_at.x;
    }
    // VIS beside EQ: Winamp has no art for it, so a converted skin makes
    // it from EQ's; without it, it's in the skin's button colours.
    if let Some(vis) = paint.size("vis", unit) {
        let vis_at = pos2(left - 2.0 * unit - vis.x, rect.center().y - vis.y / 2.0);
        let state = toggle_state(view.settings.deck_shown);
        let response = button(
            ui, &painter, paint, vis_at, unit, "vis", state, "Deck", true,
        );
        vis_clicked(response, view, actions);
        left = vis_at.x;
    } else {
        let vis = super::widgets::small_button_width(ui, "VIS");
        let vis_rect = Rect::from_min_size(
            pos2(
                left - 2.0 * unit - vis,
                rect.center().y - super::widgets::SMALL_HEIGHT / 2.0,
            ),
            vec2(vis, super::widgets::SMALL_HEIGHT),
        );
        let response = ui
            .scope_builder(egui::UiBuilder::new().max_rect(vis_rect), |ui| {
                super::widgets::small_button(ui, &palette, "VIS", view.settings.deck_shown)
            })
            .inner;
        vis_clicked(response, view, actions);
        left = vis_rect.left();
    }
    if let Some(track) = paint.size("volume.track", unit) {
        let at = pos2(left - GAP * unit - track.x, rect.center().y - track.y / 2.0);
        if at.x >= x {
            volume(ui, &painter, paint, at, unit, inputs, actions);
        }
    }
}

/// The song as the marquee shows it: while it plays, a song longer than
/// the display scrolls a character at a time, from its start.
fn marquee(ui: &Ui, view: &View, paint: &SkinPaint, text: &str, width: f32, unit: f32) -> String {
    let fits = (width / (paint.cell(Role::Display).x * unit)).floor() as usize;
    if view.player.status != Status::Playing || text.chars().count() <= fits {
        return text.to_string();
    }
    let strip: Vec<char> = text.chars().chain(MARQUEE_GAP.chars()).collect();
    let ms = u64::from(view.player.position_now());
    let step = (ms / MARQUEE_STEP) as usize % strip.len();
    ui.ctx()
        .request_repaint_after(Duration::from_millis(MARQUEE_STEP - ms % MARQUEE_STEP));
    strip.iter().cycle().skip(step).take(fits + 1).collect()
}

/// Seek, across the strip, with the song's length in a small display at
/// its end.
fn seek_bar(
    ui: &mut Ui,
    rect: Rect,
    view: &View,
    paint: &SkinPaint,
    unit: f32,
    inputs: &mut Inputs,
    actions: &mut Actions,
) {
    let painter = ui.painter().clone();
    let ready = matches!(view.session, SessionState::Ready { .. });
    let player = view.player;
    let duration = player.track.as_ref().map_or(0, |t| t.duration_ms) as f32 / 1000.0;
    let length = length(view);

    // The length's display first, from the right, so the track takes the
    // rest.
    let cell = paint.cell(Role::Display);
    let [left, top, right, bottom] = padding(paint, "small_display");
    let display = vec2(
        left + length.chars().count() as f32 * cell.x + right,
        top + cell.y + bottom,
    ) * unit;
    let display_at = pos2(rect.right() - display.x, rect.center().y - display.y / 2.0);
    small_display(&painter, view, paint, display_at, &length, unit);

    let height = paint
        .size("seek.track", unit)
        .map_or(rect.height(), |size| size.y);
    let track = Rect::from_min_max(
        pos2(rect.left(), rect.center().y - height / 2.0),
        pos2(display_at.x - 4.0 * unit, rect.center().y + height / 2.0),
    )
    .round_to_pixels(ui.pixels_per_point());
    paint.paint(&painter, "seek.track", State::Normal, track, unit);

    let enabled = ready && duration > 0.0 && player.status != Status::Stopped;
    let sense = if enabled {
        Sense::click_and_drag()
    } else {
        Sense::hover()
    };
    let seek = ui.interact(track, Id::new("skin-seek"), sense);
    let thumb = paint.size("seek.thumb", unit).unwrap_or(vec2(0.0, height));
    let travel = (track.width() - thumb.x).max(1.0);
    if enabled
        && seek.is_pointer_button_down_on()
        && let Some(pointer) = seek.interact_pointer_pos()
    {
        let fraction = ((pointer.x - track.left() - thumb.x / 2.0) / travel).clamp(0.0, 1.0);
        inputs.seeking = Some(fraction * duration);
    }
    if (seek.drag_stopped() || seek.clicked())
        && let Some(seconds) = inputs.seeking.take()
    {
        actions.push(Action::Seek((seconds * 1000.0) as u32));
    }
    let position = inputs
        .seeking
        .unwrap_or(player.position_now() as f32 / 1000.0);
    seek.widget_info(|| WidgetInfo::slider(enabled, f64::from(position), "Seek"));
    if enabled {
        let fraction = (position / duration).clamp(0.0, 1.0);
        let x = track.left() + fraction * travel;
        let played = painter.with_clip_rect(track.with_max_x(x + thumb.x / 2.0));
        paint.paint(&played, "seek.fill", State::Normal, track, unit);
        let state = if seek.is_pointer_button_down_on() {
            State::Pressed
        } else {
            State::Normal
        };
        let at = pos2(x, track.center().y - thumb.y / 2.0);
        paint.paint_at(&painter, "seek.thumb", state, at, unit);
    }
}

/// Transport, eject, shuffle and repeat, each centred in the row.
pub fn buttons_row(
    ui: &mut Ui,
    rect: Rect,
    view: &View,
    paint: &SkinPaint,
    unit: f32,
    inputs: &mut Inputs,
    actions: &mut Actions,
) {
    let painter = ui.painter().clone();
    let ready = matches!(view.session, SessionState::Ready { .. });
    let mut x = rect.left();
    let mut place = |name: &str, gap_before: f32| {
        let size = paint.size(name, unit).unwrap_or_default();
        x += gap_before * unit;
        let at = pos2(x, rect.center().y - size.y / 2.0);
        x += size.x;
        at
    };
    let [playing, paused] = held(ui, view, inputs);
    for (name, label, action) in TRANSPORT {
        let at = place(name, 0.0);
        let state = toggle_state(match name {
            "play" => playing,
            "pause" => paused,
            _ => false,
        });
        let response = button(ui, &painter, paint, at, unit, name, state, label, ready);
        if matches!(action, Action::Play | Action::TogglePause) {
            follow_press(ui, &response, action, view, inputs, actions);
        } else if response.clicked() {
            actions.push(action);
        }
    }
    let at = place("eject", GAP);
    let state = toggle_state(view.browse.open);
    let response = button(
        ui, &painter, paint, at, unit, "eject", state, "Browse", ready,
    );
    eject_clicked(response, view, inputs, actions);
    let at = place("shuffle", GAP);
    let state = toggle_state(view.shuffle);
    if button(
        ui, &painter, paint, at, unit, "shuffle", state, "Shuffle", true,
    )
    .clicked()
    {
        actions.push(Action::SetShuffle(!view.shuffle));
    }
    let at = place("repeat", 0.0);
    let state = toggle_state(view.repeat);
    if button(
        ui, &painter, paint, at, unit, "repeat", state, "Repeat", true,
    )
    .clicked()
    {
        actions.push(Action::SetRepeat(!view.repeat));
    }
}

pub(super) fn toggle_state(on: bool) -> State {
    if on { State::On } else { State::Normal }
}

/// A button drawn pressed while the pointer holds it down. `state` is
/// Normal, or On for a toggle that is on.
#[allow(clippy::too_many_arguments)]
pub(super) fn button(
    ui: &Ui,
    painter: &Painter,
    paint: &SkinPaint,
    at: Pos2,
    unit: f32,
    name: &str,
    state: State,
    label: &str,
    enabled: bool,
) -> Response {
    let size = paint.size(name, unit).unwrap_or_default();
    let rect = Rect::from_min_size(at, size);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let response = ui.interact(rect, Id::new(("skin-button", name)), sense);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    let state = super::skin_paint::state(&response, state == State::On, enabled);
    paint.paint_at(painter, name, state, at, unit);
    response
}

/// The volume: the skin's track in the shade for the level, or with its
/// fill up to it, and its thumb.
fn volume(
    ui: &Ui,
    painter: &Painter,
    paint: &SkinPaint,
    at: Pos2,
    unit: f32,
    inputs: &mut Inputs,
    actions: &mut Actions,
) {
    let Some(size) = paint.size("volume.track", unit) else {
        return;
    };
    let track = Rect::from_min_size(at, size).round_to_pixels(painter.pixels_per_point());
    let response = ui.interact(track, Id::new("skin-volume"), Sense::click_and_drag());
    let thumb = paint.size("volume.thumb", unit).unwrap_or_default();
    let travel = track.width() - thumb.x;
    if response.is_pointer_button_down_on()
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let fraction = ((pointer.x - track.left() - thumb.x / 2.0) / travel).clamp(0.0, 1.0);
        let level = (fraction * 100.0).round() as u8;
        if level != inputs.volume {
            inputs.volume = level;
            actions.push(Action::Volume(level));
        }
    }
    response.widget_info(|| WidgetInfo::slider(true, f64::from(inputs.volume), "Volume"));
    let level = f32::from(inputs.volume) / 100.0;
    let x = track.left() + level * travel;
    // A track in shades shows the level by its shade, as Winamp's does;
    // one without is filled up to the thumb.
    paint.paint_level(painter, "volume.track", level, track, unit);
    if !paint.has_levels("volume.track") {
        let filled = painter.with_clip_rect(track.with_max_x(x + thumb.x / 2.0));
        paint.paint(&filled, "volume.fill", State::Normal, track, unit);
    }
    let state = if response.is_pointer_button_down_on() {
        State::Pressed
    } else {
        State::Normal
    };
    let at = pos2(x, track.center().y - thumb.y / 2.0);
    paint.paint_at(painter, "volume.thumb", state, at, unit);
}
