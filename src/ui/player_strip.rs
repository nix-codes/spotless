//! The player strip, laid out as Winamp's main window: the LCD with the
//! time, the song's display beside it over the volume row, then seek
//! across the strip and the buttons below. With a skin
//! that has them, it is drawn from the skin's bitmaps (`skinned_strip`).

use std::time::Duration;

use egui::{
    Align, Align2, CursorIcon, FontId, Id, Layout, Rect, Response, Sense, Slider, Ui, UiBuilder,
    pos2, vec2,
};

use super::widgets::{Glyph, glyph_button, glyph_toggle, paint_glyph, small_button};
use super::{Inputs, View, theme};
use crate::action::{Action, Actions};
use crate::backend::SessionState;
use crate::model::{EntryId, PlaylistId};
use crate::playback::engine::Status;

pub const HEIGHT: f32 = 122.0;
/// The LCD and the song's display beside it, with the volume row below.
const TOP: f32 = 68.0;
const LCD_WIDTH: f32 = 170.0;
const TITLE_HEIGHT: f32 = 44.0;
const SEEK_HEIGHT: f32 = 18.0;
/// While paused, the time is shown this long, then hidden as long.
const BLINK: Duration = Duration::from_secs(1);
/// How long Play or Pause stays down for a click the player hasn't
/// answered, in seconds.
const PRESS_HOLD: f64 = 1.0;

/// Where the strip's buttons go, and how big they are together.
pub struct Keys {
    /// The row they sit in, across the strip, centred on its middle.
    pub row: Rect,
    pub width: f32,
    pub height: f32,
    /// Points per image pixel, when the skin draws them.
    unit: Option<f32>,
}

/// Transport, eject, shuffle and repeat of the plain strip, side by side:
/// eight buttons, 4 points apart and 10 between the groups.
const KEYS_WIDTH: f32 = 328.0;

/// Where the buttons go in the strip `rect`.
pub fn keys(ui: &Ui, view: &View, rect: Rect) -> Keys {
    if let Some(paint) = view.art.filter(|paint| paint.has_player()) {
        let (row, unit, width) = super::skinned_strip::keys(ui, paint, rect);
        return Keys {
            row,
            width,
            height: row.height(),
            unit: Some(unit),
        };
    }
    Keys {
        row: Rect::from_min_max(
            pos2(rect.left(), rect.top() + TOP + 8.0 + SEEK_HEIGHT + 8.0),
            rect.max,
        ),
        width: KEYS_WIDTH,
        height: super::widgets::BUTTON_HEIGHT,
        unit: None,
    }
}

/// The buttons, from the left of `rect` and centred on its middle.
pub fn draw_keys(
    ui: &mut Ui,
    rect: Rect,
    keys: &Keys,
    view: &View,
    inputs: &mut Inputs,
    actions: &mut Actions,
) {
    match (keys.unit, view.art) {
        (Some(unit), Some(paint)) => {
            super::skinned_strip::buttons_row(ui, rect, view, paint, unit, inputs, actions);
        }
        _ => row(ui, rect, |ui| buttons_row(ui, view, inputs, actions)),
    }
}

/// The strip; without its buttons when the deck has them.
pub fn draw(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Actions, buttons: bool) {
    if let Some(paint) = view.art.filter(|paint| paint.has_player()) {
        return super::skinned_strip::draw(ui, view, paint, inputs, actions, buttons);
    }
    let rect = ui.max_rect();
    let lcd = Rect::from_min_size(rect.min, vec2(LCD_WIDTH, TOP));
    draw_lcd(ui, lcd, view);
    let title = Rect::from_min_max(
        pos2(lcd.right() + 10.0, rect.top()),
        pos2(rect.right(), rect.top() + TITLE_HEIGHT),
    );
    draw_title(ui, title, view);
    display_clicked(ui, title, view, inputs, actions);
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
    if buttons {
        let keys = keys(ui, view, rect);
        row(ui, keys.row, |ui| buttons_row(ui, view, inputs, actions));
    }
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

/// Which of Play and Pause stay pressed in: Play while a song plays,
/// Pause while it is paused. Stop never does.
///
/// Pressed, Play or Pause shows what it asks for from the moment it goes
/// down until the player gets there, so it doesn't pop up while the
/// player catches up.
pub(super) fn held(ui: &Ui, view: &View, inputs: &mut Inputs) -> [bool; 2] {
    let mut status = view.player.status;
    if let Some(press) = &inputs.pressed {
        let there =
            press.asked == status || (press.asked == Status::Playing && status == Status::Loading);
        let waited = ui.input(|i| i.time) - press.since;
        if press.clicked && (there || waited > PRESS_HOLD) {
            inputs.pressed = None;
        } else {
            status = press.asked;
            if press.clicked {
                ui.ctx()
                    .request_repaint_after(Duration::from_secs_f64(PRESS_HOLD - waited));
            }
        }
    }
    [
        matches!(status, Status::Playing | Status::Loading),
        status == Status::Paused,
    ]
}

/// Stop, clicked: stops, or with Ctrl stops once the playing song ends
/// (Winamp's Stop after current).
pub(super) fn stop_clicked(ui: &Ui, response: Response, actions: &mut Actions) {
    let stop = response
        .on_hover_text("Stop (V). Ctrl-click: stop after this song (Shift+V)")
        .clicked();
    if stop {
        actions.push(if ui.input(|i| i.modifiers.command) {
            Action::ToggleStopAfter
        } else {
            Action::Stop
        });
    }
}

/// A press of Play or Pause not yet answered: what it asks the player
/// for, when, and whether it was let go on the button (a click).
pub struct Pressed {
    asked: Status,
    pause: bool,
    since: f64,
    clicked: bool,
}

/// Follows Play or Pause (`action`) through a press: down, it holds the
/// state it asks for; clicked, it asks for it; let go off the button, it
/// asks for nothing.
pub(super) fn follow_press(
    ui: &Ui,
    response: &Response,
    action: Action,
    view: &View,
    inputs: &mut Inputs,
    actions: &mut Actions,
) {
    let pause = action == Action::TogglePause;
    let asked = match (pause, view.player.status) {
        (false, _) | (true, Status::Paused) => Some(Status::Playing),
        (true, Status::Playing | Status::Loading) => Some(Status::Paused),
        (true, Status::Stopped) => None,
    };
    let since = ui.input(|i| i.time);
    let press = |clicked| {
        asked.map(|asked| Pressed {
            asked,
            pause,
            since,
            clicked,
        })
    };
    let mine = inputs.pressed.as_ref().is_some_and(|p| p.pause == pause);
    if response.is_pointer_button_down_on() {
        inputs.pressed = press(false);
    } else if response.clicked() {
        actions.push(action);
        match &mut inputs.pressed {
            Some(pressed) if mine && !pressed.clicked => {
                pressed.clicked = true;
                pressed.since = since;
            }
            _ => inputs.pressed = press(true),
        }
    } else if mine && inputs.pressed.as_ref().is_some_and(|p| !p.clicked) {
        inputs.pressed = None;
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

/// The song's display: the artist and the song while one plays, or the
/// one Play would start while stopped, else blank, unless not being
/// signed in is why nothing can.
pub(super) fn now_lines(view: &View) -> (String, String) {
    match (&view.player.track, view.player.status) {
        (_, Status::Loading) => ("Loading…".to_string(), String::new()),
        (Some(track), _) => (track.artists.clone(), track.title.clone()),
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
        super::browse_view::opening(view, inputs);
        actions.push(Action::ToggleBrowse);
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

/// Clicking the song's display jumps to the song in its playlist (J).
pub(super) fn display_clicked(
    ui: &Ui,
    rect: Rect,
    view: &View,
    inputs: &mut Inputs,
    actions: &mut Actions,
) {
    let response = ui.interact(rect, Id::new("song-display"), Sense::click());
    if playing_entry(view).is_none() {
        return;
    }
    if response
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text("Show the song in its playlist (J)")
        .clicked()
    {
        jump_to_playing(view, inputs, actions);
    }
}

/// The playing song's playlist and row, or while stopped the one Play
/// would start.
pub(super) fn playing_entry(view: &View) -> Option<(PlaylistId, EntryId)> {
    let list = view.playing_list.and_then(|id| view.list(id))?;
    Some((list.id, list.cursor().or(list.first())?))
}

/// Shows the playlist the song plays from, the playlists too if hidden,
/// and selects the song, placed as on opening the playlist.
pub(super) fn jump_to_playing(view: &View, inputs: &mut Inputs, actions: &mut Actions) {
    let Some((list, entry)) = playing_entry(view) else {
        return;
    };
    actions.push(Action::CloseBrowse);
    if !view.settings.playlists {
        actions.push(Action::TogglePlaylists);
    }
    actions.push(Action::SelectPlaylist(list));
    inputs.jump = Some((list, entry));
}

/// EQ shows or hides the equaliser below the player.
pub(super) fn eq_clicked(response: egui::Response, view: &View, actions: &mut Actions) {
    let tip = if view.settings.equaliser_shown {
        "Hide the equaliser (Alt+G)"
    } else {
        "Show the equaliser (Alt+G)"
    };
    if response.on_hover_text(tip).clicked() {
        actions.push(Action::ToggleEqualiser);
    }
}

/// VIS shows or hides the deck below the player.
pub(super) fn vis_clicked(response: egui::Response, view: &View, actions: &mut Actions) {
    let tip = if view.settings.deck_shown {
        "Hide the deck (Alt+V)"
    } else {
        "Show the deck: the music on a cassette, reels, a CD or a record, its spectrum and levels (Alt+V)"
    };
    if response.on_hover_text(tip).clicked() {
        actions.push(Action::ToggleDeck);
    }
}

/// The bitrate, then the volume, VIS, EQ and PL at the right end.
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
        ui.add_space(4.0);
        let eq = small_button(ui, &palette, "EQ", view.settings.equaliser_shown);
        eq_clicked(eq, view, actions);
        ui.add_space(4.0);
        let vis = small_button(ui, &palette, "VIS", view.settings.deck_shown);
        vis_clicked(vis, view, actions);
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
/// The tooltips of the transport, shuffle and repeat buttons, with their
/// keys; Stop's is [`stop_clicked`]'s.
pub(super) const PREVIOUS_TIP: &str = "Previous song (Z)";
pub(super) const PLAY_TIP: &str = "Play (Space)";
pub(super) const PAUSE_TIP: &str = "Pause (Space)";
pub(super) const NEXT_TIP: &str = "Next song (B)";
pub(super) const SHUFFLE_TIP: &str = "Shuffle on / off (S)";
pub(super) const REPEAT_TIP: &str = "Repeat on / off (R)";

fn buttons_row(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
    let palette = view.palette;
    let ready = matches!(view.session, SessionState::Ready { .. });
    ui.spacing_mut().item_spacing.x = 4.0;
    if glyph_button(ui, &palette, Glyph::Previous, ready)
        .on_hover_text(PREVIOUS_TIP)
        .clicked()
    {
        actions.push(Action::Previous);
    }
    let [playing, paused] = held(ui, view, inputs);
    let play = glyph_toggle(ui, &palette, Glyph::Play, playing, ready).on_hover_text(PLAY_TIP);
    follow_press(ui, &play, Action::Play, view, inputs, actions);
    let pause = glyph_toggle(ui, &palette, Glyph::Pause, paused, ready).on_hover_text(PAUSE_TIP);
    follow_press(ui, &pause, Action::TogglePause, view, inputs, actions);
    let stop = glyph_toggle(ui, &palette, Glyph::Stop, view.stop_after, ready);
    stop_clicked(ui, stop, actions);
    if glyph_button(ui, &palette, Glyph::Next, ready)
        .on_hover_text(NEXT_TIP)
        .clicked()
    {
        actions.push(Action::Next);
    }
    ui.add_space(6.0);
    let eject = glyph_toggle(ui, &palette, Glyph::Eject, view.browse.open, ready);
    eject_clicked(eject, view, inputs, actions);
    ui.add_space(6.0);
    if glyph_toggle(ui, &palette, Glyph::Shuffle, view.shuffle, true)
        .on_hover_text(SHUFFLE_TIP)
        .clicked()
    {
        actions.push(Action::SetShuffle(!view.shuffle));
    }
    if glyph_toggle(ui, &palette, Glyph::Repeat, view.repeat, true)
        .on_hover_text(REPEAT_TIP)
        .clicked()
    {
        actions.push(Action::SetRepeat(!view.repeat));
    }
}
