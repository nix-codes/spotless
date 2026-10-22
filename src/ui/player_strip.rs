//! The player strip, laid out as Winamp's main window: the LCD with the
//! time, the song's display beside it over the volume row, then seek
//! across the strip and the buttons below. With a skin
//! that has them, it is drawn from the skin's bitmaps (`skinned_strip`).

use std::time::Duration;

use egui::{Align, Align2, FontId, Layout, Rect, Slider, Ui, UiBuilder, pos2, vec2};

use super::widgets::{Glyph, glyph_button, glyph_toggle, paint_glyph, small_button};
use super::{Inputs, View, theme};
use crate::action::{Action, Actions};
use crate::backend::SessionState;
use crate::playback::engine::Status;

pub const HEIGHT: f32 = 122.0;
/// The LCD and the song's display beside it, with the volume row below.
const TOP: f32 = 68.0;
const LCD_WIDTH: f32 = 170.0;
const TITLE_HEIGHT: f32 = 44.0;
const SEEK_HEIGHT: f32 = 18.0;
/// While paused, the time is shown this long, then hidden as long.
const BLINK: Duration = Duration::from_secs(1);

pub fn draw(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
    if let Some(paint) = view.art.filter(|paint| paint.has_player()) {
        return super::skinned_strip::draw(ui, view, paint, inputs, actions);
    }
    let rect = ui.max_rect();
    let lcd = Rect::from_min_size(rect.min, vec2(LCD_WIDTH, TOP));
    draw_lcd(ui, lcd, view);
    let title = Rect::from_min_max(
        pos2(lcd.right() + 10.0, rect.top()),
        pos2(rect.right(), rect.top() + TITLE_HEIGHT),
    );
    draw_title(ui, title, view);
    let info = Rect::from_min_max(
        pos2(title.left(), title.bottom()),
        pos2(rect.right(), lcd.bottom()),
    );
    row(ui, info, |ui| info_row(ui, view, inputs, actions));
    let seek = Rect::from_min_size(
        pos2(rect.left(), lcd.bottom() + 8.0),
        vec2(rect.width(), SEEK_HEIGHT),
    );
    row(ui, seek, |ui| seek_row(ui, view, inputs, actions));
    let buttons = Rect::from_min_max(pos2(rect.left(), seek.bottom() + 8.0), rect.max);
    row(ui, buttons, |ui| buttons_row(ui, view, inputs, actions));
}

/// Lays `add` out left to right in `rect`, centred on its middle.
fn row(ui: &mut Ui, rect: Rect, add: impl FnOnce(&mut Ui)) {
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center)),
        add,
    );
}

/// The LCD: the status glyph and the time, as in Winamp.
fn draw_lcd(ui: &Ui, rect: Rect, view: &View) {
    let palette = view.palette;
    let painter = ui.painter().with_clip_rect(rect);
    theme::inset(&painter, rect, palette.lcd_bg, &palette);
    let player = view.player;
    let left = rect.left() + 14.0;

    let glyph = match player.status {
        Status::Playing | Status::Loading => Glyph::Play,
        Status::Paused => Glyph::Pause,
        Status::Stopped => Glyph::Stop,
    };
    paint_glyph(ui, pos2(left + 5.0, rect.center().y), glyph, palette.lcd);

    // Stopped, the time is blank, as in Winamp.
    if player.status == Status::Stopped {
        return;
    }
    let seconds = player.position_now() / 1000;
    let time = if time_shown(ui, view) {
        format!("{:02}:{:02}", seconds / 60, seconds % 60)
    } else {
        "  :  ".to_string()
    };
    painter.text(
        pos2(left + 22.0, rect.center().y),
        Align2::LEFT_CENTER,
        time,
        FontId::monospace(32.0),
        palette.lcd,
    );
}

/// The song's display: the artist above, the song below.
fn draw_title(ui: &Ui, rect: Rect, view: &View) {
    let palette = view.palette;
    let painter = ui.painter().with_clip_rect(rect);
    theme::inset(&painter, rect, palette.lcd_bg, &palette);
    let (artist, song) = now_lines(view);
    let left = rect.left() + 10.0;
    let font = FontId::monospace(14.0);
    for (y, line) in [(14.0, artist), (31.0, song)] {
        painter.text(
            pos2(left, rect.top() + y),
            Align2::LEFT_CENTER,
            line,
            font.clone(),
            palette.lcd,
        );
    }
}

/// Whether the time's digits show: while paused they blink, as in
/// Winamp; the colon stays.
pub(super) fn time_shown(ui: &Ui, view: &View) -> bool {
    if view.player.status != Status::Paused {
        return true;
    }
    let blink = BLINK.as_secs_f64();
    let now = ui.input(|i| i.time);
    let into = now % (2.0 * blink);
    ui.ctx()
        .request_repaint_after(Duration::from_secs_f64(blink - into % blink));
    into < blink
}

/// The song's display: the artist and the song while one plays, else
/// blank, unless not being signed in is why nothing can.
pub(super) fn now_lines(view: &View) -> (String, String) {
    match (&view.player.track, view.player.status) {
        (_, Status::Loading) => ("Loading…".to_string(), String::new()),
        (Some(track), Status::Playing | Status::Paused) => {
            (track.artists.clone(), track.title.clone())
        }
        _ => match view.session {
            SessionState::Ready { .. } => (String::new(), String::new()),
            _ => ("Not signed in".into(), String::new()),
        },
    }
}

/// What every song streams at, while one does: stopped, the displays are
/// blank, as the time is, and keep their width.
pub(super) fn rates(view: &View) -> [&'static str; 2] {
    if view.player.status == Status::Stopped {
        ["   ", "  "]
    } else {
        ["320", "44"]
    }
}

/// The song's length, or blank, at its width, while stopped.
pub(super) fn length(view: &View) -> String {
    match &view.player.track {
        Some(track) if view.player.status != Status::Stopped => {
            let total = track.duration_ms / 1000;
            format!("{}:{:02}", total / 60, total % 60)
        }
        _ => "    ".into(),
    }
}

/// Eject opens Browse, as it opens files in Winamp, and stays pressed in
/// until it is clicked again for the playlists.
pub fn eject_clicked(
    response: egui::Response,
    view: &View,
    inputs: &mut Inputs,
    actions: &mut Actions,
) {
    let tip = if view.browse.open {
        "Back to the playlists (Ctrl+B)"
    } else {
        "Browse (Ctrl+B)"
    };
    if response.on_hover_text(tip).clicked() {
        actions.push(Action::ToggleBrowse);
        if !view.browse.open {
            inputs.focus_search = true;
        }
    }
}

/// PL shows or hides the playlists below the player; in Browse or the
/// settings it only sets what leaving them comes back to.
pub(super) fn pl_clicked(response: egui::Response, view: &View, actions: &mut Actions) {
    let tip = if view.settings.playlists {
        "Hide the playlists"
    } else {
        "Show the playlists"
    };
    if response.on_hover_text(tip).clicked() {
        actions.push(Action::TogglePlaylists);
    }
}

/// The bitrate, then the volume and PL at the right end.
fn info_row(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
    let palette = view.palette;
    let label = |text: String| {
        egui::RichText::new(text)
            .monospace()
            .size(9.0)
            .color(palette.chrome)
    };
    ui.add_space(2.0);
    let [kbps, khz] = rates(view);
    ui.label(label(format!("{kbps} KBPS · {khz} KHZ")));
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let pl = small_button(ui, &palette, "PL", view.settings.playlists);
        pl_clicked(pl, view, actions);
        ui.add_space(8.0);
        ui.spacing_mut().slider_width = 90.0;
        let mut volume = inputs.volume;
        let response = ui.add(Slider::new(&mut volume, 0..=100).show_value(false));
        if response.changed() {
            inputs.volume = volume;
            actions.push(Action::Volume(volume));
        }
        ui.label(label("VOL".into()));
    });
}

/// Seek, across the strip, with the song's length at its end.
fn seek_row(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
    let palette = view.palette;
    let ready = matches!(view.session, SessionState::Ready { .. });
    let player = view.player;
    let duration = player.track.as_ref().map_or(0, |t| t.duration_ms) as f32 / 1000.0;
    ui.spacing_mut().slider_width = (ui.available_width() - 50.0).max(60.0);
    let mut position = inputs
        .seeking
        .unwrap_or(player.position_now() as f32 / 1000.0);
    let enabled = ready && duration > 0.0 && player.status != Status::Stopped;
    let response = ui.add_enabled(
        enabled,
        Slider::new(&mut position, 0.0..=duration.max(1.0)).show_value(false),
    );
    if response.dragged() || response.changed() {
        inputs.seeking = Some(position);
    }
    if response.drag_stopped() || (response.changed() && !response.dragged()) {
        actions.push(Action::Seek((position * 1000.0) as u32));
        inputs.seeking = None;
    }
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.label(
            egui::RichText::new(length(view))
                .monospace()
                .size(9.0)
                .color(palette.chrome),
        );
    });
}

/// Transport, eject, shuffle and repeat.
fn buttons_row(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
    let palette = view.palette;
    let ready = matches!(view.session, SessionState::Ready { .. });
    ui.spacing_mut().item_spacing.x = 4.0;
    if glyph_button(ui, &palette, Glyph::Previous, ready).clicked() {
        actions.push(Action::Previous);
    }
    if glyph_button(ui, &palette, Glyph::Play, ready).clicked() {
        actions.push(Action::Play);
    }
    if glyph_button(ui, &palette, Glyph::Pause, ready).clicked() {
        actions.push(Action::TogglePause);
    }
    if glyph_button(ui, &palette, Glyph::Stop, ready).clicked() {
        actions.push(Action::Stop);
    }
    if glyph_button(ui, &palette, Glyph::Next, ready).clicked() {
        actions.push(Action::Next);
    }
    ui.add_space(6.0);
    let eject = glyph_toggle(ui, &palette, Glyph::Eject, view.browse.open, ready);
    eject_clicked(eject, view, inputs, actions);
    ui.add_space(6.0);
    if glyph_toggle(ui, &palette, Glyph::Shuffle, view.shuffle, true).clicked() {
        actions.push(Action::SetShuffle(!view.shuffle));
    }
    if glyph_toggle(ui, &palette, Glyph::Repeat, view.repeat, true).clicked() {
        actions.push(Action::SetRepeat(!view.repeat));
    }
}
