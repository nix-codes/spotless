//! The player strip drawn from a skin's bitmaps: its transport buttons,
//! the time in its digits, the song in its font, its seek bar, shuffle,
//! repeat and volume. Laid out as the plain strip is, the LCD on the left.

use std::time::Duration;

use egui::emath::GuiRounding;
use egui::{
    Align, Id, Layout, Painter, Pos2, Rect, Response, Sense, Ui, UiBuilder, WidgetInfo, WidgetType,
    pos2, vec2,
};

use super::player_strip::{HEIGHT, meta_line, now_line};
use super::skin_art::{self, SkinArt};
use super::widgets::text_button;
use super::{Inputs, View, theme};
use crate::action::Action;
use crate::backend::SessionState;
use crate::playback::engine::Status;
use crate::skin::sprites::{self, Sprite};

/// The LCD's rows, in skin pixels from its top: the time, the song, the
/// album; and how tall they are together.
const SONG_Y: f32 = 18.0;
const META_Y: f32 = 27.0;
const LCD_ROWS: f32 = 33.0;
/// The controls' rows: the seek bar, then the buttons below it.
const BUTTONS_Y: f32 = 16.0;
const CONTROL_ROWS: f32 = 34.0;
/// Widths in skin pixels.
const TRANSPORT: f32 = 114.0;
const SHUFFLE: f32 = 47.0;
const REPEAT: f32 = 28.0;
const VOLUME: f32 = 68.0;
const GAP: f32 = 6.0;
/// The controls' row, BROWSE aside: as wide as Winamp's main window.
const CONTROLS: f32 = TRANSPORT + GAP + SHUFFLE + REPEAT + GAP + VOLUME + GAP;
/// Points kept for BROWSE.
const BROWSE_WIDTH: f32 = 100.0;
const LCD_WIDTH: f32 = 340.0;
const MIN_LCD_WIDTH: f32 = 160.0;
/// What separates the end of a scrolling song from its start again.
const MARQUEE_GAP: &str = "  ***  ";
/// How long the marquee waits between one-character steps, in ms.
const MARQUEE_STEP: u64 = 220;

pub fn draw(
    ui: &mut Ui,
    view: &View,
    art: &SkinArt,
    inputs: &mut Inputs,
    actions: &mut Vec<Action>,
) {
    let rect = ui.max_rect();
    let room = (rect.width() - MIN_LCD_WIDTH - 10.0 - BROWSE_WIDTH) / CONTROLS;
    let unit = skin_art::unit(ui.ctx(), room.min((HEIGHT - 8.0) / CONTROL_ROWS));
    let controls_width = CONTROLS * unit + BROWSE_WIDTH;
    let lcd_width = (rect.width() - 10.0 - controls_width).clamp(MIN_LCD_WIDTH, LCD_WIDTH);
    let lcd = Rect::from_min_size(rect.min, vec2(lcd_width, HEIGHT));
    draw_lcd(ui, lcd, view, art, unit);
    let controls = Rect::from_min_max(pos2(lcd.right() + 10.0, rect.top()), rect.max);
    draw_controls(ui, controls, view, art, unit, inputs, actions);
}

fn draw_lcd(ui: &Ui, rect: Rect, view: &View, art: &SkinArt, unit: f32) {
    let palette = view.palette;
    let painter = ui.painter().with_clip_rect(rect);
    theme::inset(&painter, rect, palette.lcd_bg, &palette);
    let player = view.player;
    let origin = pos2(
        rect.left() + 10.0,
        rect.top() + (rect.height() - LCD_ROWS * unit) / 2.0,
    )
    .round_to_pixels(ui.pixels_per_point());
    let at = |x: f32, y: f32| origin + vec2(x, y) * unit;

    let status = match player.status {
        Status::Playing | Status::Loading => sprites::STATUS_PLAYING,
        Status::Paused => sprites::STATUS_PAUSED,
        Status::Stopped => sprites::STATUS_STOPPED,
    };
    art.paint(&painter, status, at(0.0, 2.0), unit);

    // The time, at Winamp's spacing: two digits, a colon, two digits.
    let seconds = if player.status == Status::Stopped {
        0
    } else {
        player.position_now() / 1000
    };
    let minutes = (seconds / 60).min(99);
    let digits = [
        (20.0, minutes / 10),
        (32.0, minutes % 10),
        (50.0, seconds % 60 / 10),
        (62.0, seconds % 10),
    ];
    for (x, value) in digits {
        art.paint(
            &painter,
            sprites::digit(art.digits, value),
            at(x, 0.0),
            unit,
        );
    }
    // The colon is part of Winamp's main window, not of the digits.
    for y in [4.0, 8.0] {
        let dot = Rect::from_min_size(at(44.0, y), vec2(3.0, 1.0) * unit);
        painter.rect_filled(dot, 0.0, palette.lcd);
    }

    let width = rect.right() - 10.0 - origin.x;
    let song = marquee(ui, view, &now_line(view), width, unit);
    art.text(&painter, &song, at(0.0, SONG_Y), width, unit);
    let meta = meta_line(view, '-');
    art.text(&painter, &meta, at(0.0, META_Y), width, unit);
}

/// The song as the marquee shows it: while it plays, a song longer than
/// the display scrolls a character at a time, from its start.
fn marquee(ui: &Ui, view: &View, text: &str, width: f32, unit: f32) -> String {
    let fits = (width / (5.0 * unit)).floor() as usize;
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

fn draw_controls(
    ui: &mut Ui,
    rect: Rect,
    view: &View,
    art: &SkinArt,
    unit: f32,
    inputs: &mut Inputs,
    actions: &mut Vec<Action>,
) {
    let palette = view.palette;
    let painter = ui.painter().with_clip_rect(rect);
    let ready = matches!(view.session, SessionState::Ready { .. });
    let player = view.player;
    let origin = pos2(
        rect.left(),
        rect.top() + (rect.height() - CONTROL_ROWS * unit) / 2.0,
    )
    .round_to_pixels(ui.pixels_per_point());
    let at = |x: f32, y: f32| origin + vec2(x, y) * unit;

    // Seek, with the song's length in a small display at its end.
    let duration = player.track.as_ref().map_or(0, |t| t.duration_ms) as f32 / 1000.0;
    let total = duration as u32;
    let length = format!("{}:{:02}", total / 60, total % 60);
    let length_width = (length.chars().count() * 5) as f32 * unit;
    let track = Rect::from_min_max(
        at(0.0, 0.0),
        pos2(rect.right() - length_width - 8.0 * unit, at(0.0, 10.0).y),
    );
    art.paint_bar(
        &painter,
        sprites::POSITION_TRACK,
        track.min,
        track.width(),
        unit,
    );
    let display = Rect::from_min_size(
        pos2(track.right() + 4.0 * unit, track.top()),
        vec2(length_width + 4.0 * unit, 10.0 * unit),
    );
    theme::inset(&painter, display, palette.lcd_bg, &palette);
    let length_at = display.min + vec2(2.0, 2.0) * unit;
    art.text(&painter, &length, length_at, length_width, unit);

    let enabled = ready && duration > 0.0 && player.status != Status::Stopped;
    let sense = if enabled {
        Sense::click_and_drag()
    } else {
        Sense::hover()
    };
    let seek = ui.interact(track, Id::new("skin-seek"), sense);
    let thumb_width = sprites::POSITION_THUMB.width as f32 * unit;
    let travel = (track.width() - thumb_width).max(1.0);
    if enabled
        && seek.is_pointer_button_down_on()
        && let Some(pointer) = seek.interact_pointer_pos()
    {
        let fraction = ((pointer.x - track.left() - thumb_width / 2.0) / travel).clamp(0.0, 1.0);
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
        let thumb = if seek.is_pointer_button_down_on() {
            sprites::POSITION_THUMB_PRESSED
        } else {
            sprites::POSITION_THUMB
        };
        let x = track.left() + fraction * travel;
        art.paint(&painter, thumb, pos2(x, track.top()), unit);
    }

    // Transport, shuffle and repeat.
    let transport = [
        (
            sprites::PREVIOUS,
            sprites::PREVIOUS_PRESSED,
            "Previous",
            Action::Previous,
        ),
        (sprites::PLAY, sprites::PLAY_PRESSED, "Play", Action::Play),
        (
            sprites::PAUSE,
            sprites::PAUSE_PRESSED,
            "Pause",
            Action::TogglePause,
        ),
        (sprites::STOP, sprites::STOP_PRESSED, "Stop", Action::Stop),
        (sprites::NEXT, sprites::NEXT_PRESSED, "Next", Action::Next),
    ];
    let mut x = 0.0;
    for (normal, pressed, name, action) in transport {
        let sprites = (normal, pressed);
        if button(
            ui,
            &painter,
            art,
            at(x, BUTTONS_Y),
            unit,
            sprites,
            name,
            ready,
        )
        .clicked()
        {
            actions.push(action);
        }
        x += normal.width as f32;
    }
    x += GAP;
    let shuffle = if view.shuffle {
        (sprites::SHUFFLE_ON, sprites::SHUFFLE_ON_PRESSED)
    } else {
        (sprites::SHUFFLE_OFF, sprites::SHUFFLE_OFF_PRESSED)
    };
    let row = BUTTONS_Y + 1.0;
    if button(
        ui,
        &painter,
        art,
        at(x, row),
        unit,
        shuffle,
        "Shuffle",
        true,
    )
    .clicked()
    {
        actions.push(Action::SetShuffle(!view.shuffle));
    }
    x += SHUFFLE;
    let repeat = if view.repeat {
        (sprites::REPEAT_ON, sprites::REPEAT_ON_PRESSED)
    } else {
        (sprites::REPEAT_OFF, sprites::REPEAT_OFF_PRESSED)
    };
    if button(ui, &painter, art, at(x, row), unit, repeat, "Repeat", true).clicked() {
        actions.push(Action::SetRepeat(!view.repeat));
    }
    let repeat_end = at(x + REPEAT + GAP, 0.0).x;

    // BROWSE at the right end, the volume before it.
    let buttons = Rect::from_min_max(
        pos2(repeat_end, at(0.0, BUTTONS_Y).y),
        pos2(rect.right(), at(0.0, BUTTONS_Y + 18.0).y),
    );
    let mut browse_left = rect.right();
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(buttons)
            .layout(Layout::right_to_left(Align::Center)),
        |ui| {
            let browsing = view.browse.open;
            let label = if browsing { "PLAYLISTS" } else { "BROWSE" };
            let response = text_button(ui, &palette, label, browsing, ready);
            if response.clicked() {
                actions.push(Action::ToggleBrowse);
                if !browsing {
                    inputs.focus_search = true;
                }
            }
            browse_left = response.rect.left();
        },
    );
    let volume_at = pos2(
        browse_left - 8.0 - VOLUME * unit,
        at(0.0, BUTTONS_Y + 3.0).y,
    );
    if volume_at.x >= repeat_end {
        volume(ui, &painter, art, volume_at, unit, inputs, actions);
    }
}

/// A button drawn pressed while the pointer holds it down.
#[allow(clippy::too_many_arguments)]
fn button(
    ui: &Ui,
    painter: &Painter,
    art: &SkinArt,
    at: Pos2,
    unit: f32,
    (normal, pressed): (Sprite, Sprite),
    name: &str,
    enabled: bool,
) -> Response {
    let rect = Rect::from_min_size(at, vec2(normal.width as f32, normal.height as f32) * unit);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let response = ui.interact(rect, Id::new(("skin-button", name)), sense);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, name));
    let sprite = if response.is_pointer_button_down_on() {
        pressed
    } else {
        normal
    };
    art.paint(painter, sprite, at, unit);
    response
}

/// The volume: the skin's track for the level, and its thumb.
fn volume(
    ui: &Ui,
    painter: &Painter,
    art: &SkinArt,
    at: Pos2,
    unit: f32,
    inputs: &mut Inputs,
    actions: &mut Vec<Action>,
) {
    let frame = (u32::from(inputs.volume) * (sprites::SLIDER_FRAMES - 1) + 50) / 100;
    let track = art.paint(painter, sprites::volume_frame(frame), at, unit);
    let response = ui.interact(track, Id::new("skin-volume"), Sense::click_and_drag());
    let thumb_width = sprites::VOLUME_THUMB.width as f32 * unit;
    let travel = track.width() - thumb_width;
    if response.is_pointer_button_down_on()
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let fraction = ((pointer.x - track.left() - thumb_width / 2.0) / travel).clamp(0.0, 1.0);
        let level = (fraction * 100.0).round() as u8;
        if level != inputs.volume {
            inputs.volume = level;
            actions.push(Action::Volume(level));
        }
    }
    response.widget_info(|| WidgetInfo::slider(true, f64::from(inputs.volume), "Volume"));
    let thumb = if response.is_pointer_button_down_on() {
        sprites::VOLUME_THUMB_PRESSED
    } else {
        sprites::VOLUME_THUMB
    };
    let x = track.left() + f32::from(inputs.volume) / 100.0 * travel;
    art.paint(painter, thumb, pos2(x, track.top() + unit), unit);
}
