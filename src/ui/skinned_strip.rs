//! The player strip drawn from a skin's bitmaps: its transport buttons,
//! the time in its digits, the song in its font, its seek bar, shuffle,
//! repeat, volume and PL. Laid out as the plain strip is.
//! The title bar above it is the skin's too.

use std::time::Duration;

use egui::emath::GuiRounding;
use egui::{
    Align2, FontId, Id, Painter, Pos2, Rect, Response, Sense, Ui, WidgetInfo, WidgetType, pos2,
    vec2,
};

use super::player_strip::{HEIGHT, KBPS, KHZ, eject_clicked, now_lines, pl_clicked, time_shown};
use super::skin_art::{self, SkinArt};
use super::{Inputs, View, theme};
use crate::action::Action;
use crate::backend::SessionState;
use crate::playback::engine::Status;
use crate::skin::sprites::{self, Sprite};

/// The rows, in skin pixels from the strip's top: the LCD and the song's
/// display beside it, the volume row under the display, the seek bar,
/// then the buttons; and how tall they are together.
const TOP: f32 = 40.0;
const TITLE_HEIGHT: f32 = 22.0;
const INFO_Y: f32 = 26.0;
const SEEK_Y: f32 = 45.0;
const BUTTONS_Y: f32 = 61.0;
const ROWS: f32 = 79.0;
/// Widths in skin pixels.
const LCD_WIDTH: f32 = 71.0;
const EJECT: f32 = 22.0;
const SHUFFLE: f32 = 47.0;
const VOLUME: f32 = 68.0;
const PL: f32 = 23.0;
const GAP: f32 = 6.0;
/// The narrowest strip that keeps everything at full size, in skin pixels.
const MIN_WIDTH: f32 = 300.0;
/// What separates the end of a scrolling song from its start again.
const MARQUEE_GAP: &str = "  ***  ";
/// How long the marquee waits between one-character steps, in ms.
const MARQUEE_STEP: u64 = 220;

/// Points per skin pixel for a strip `width` points wide: smaller when
/// the window is too narrow for the strip at full size.
pub fn unit(width: f32) -> f32 {
    skin_art::unit((width / MIN_WIDTH).min((HEIGHT - 2.0) / ROWS))
}

/// The title bar: the playlist editor's, stretched, with the stripes
/// running up to the name from both sides.
pub fn title_bar(ui: &Ui, rect: Rect, view: &View, art: &SkinArt, unit: f32, label: &str) {
    let lit = ui.input(|i| i.viewport().focused.unwrap_or(true));
    let (left, middle, tile) = if lit {
        (
            sprites::TITLE_LEFT,
            sprites::TITLE_MIDDLE,
            sprites::TITLE_TILE,
        )
    } else {
        (
            sprites::TITLE_LEFT_UNLIT,
            sprites::TITLE_MIDDLE_UNLIT,
            sprites::TITLE_TILE_UNLIT,
        )
    };
    let painter = ui.painter().with_clip_rect(rect);
    let px = |n: u32| n as f32 * unit;
    let piece = |x: u32, width: u32| {
        Sprite::new(middle.sheet, middle.x + x, middle.y, width, middle.height)
    };
    // The title piece's ends finish the stripes; between them, its plain
    // background takes the name.
    let (cap, gap) = (piece(0, 5), piece(5, 1));
    let end_cap = piece(95, 5);
    let font = FontId::monospace(10.0);
    let text = ui
        .painter()
        .layout_no_wrap(label.into(), font.clone(), view.palette.chrome);
    let name = Rect::from_center_size(rect.center(), vec2(text.size().x + px(16), rect.height()))
        .round_to_pixels(ui.pixels_per_point());
    let fill_from = rect.left() + px(left.width);
    let fill_to = rect.right() - px(left.width);
    for (from, to) in [
        (fill_from, name.left() - px(cap.width)),
        (name.right() + px(end_cap.width), fill_to),
    ] {
        let tiles = painter.with_clip_rect(rect.with_min_x(from).with_max_x(to));
        let mut x = from;
        while x < to {
            art.paint(&tiles, tile, pos2(x, rect.top()), unit);
            x += px(tile.width);
        }
    }
    art.paint(
        &painter,
        cap,
        pos2(name.left() - px(cap.width), rect.top()),
        unit,
    );
    art.paint_into(&painter, gap, name, false);
    art.paint(&painter, end_cap, pos2(name.right(), rect.top()), unit);
    art.paint(&painter, left, rect.min, unit);
    // The right corner is the left one mirrored: the skin's own has the
    // playlist's close and shade buttons, which Spotless has no use for.
    art.paint_mirrored(&painter, left, pos2(fill_to, rect.top()), unit);
    painter.galley(name.center() - text.size() / 2.0, text, view.palette.chrome);
}

pub fn draw(
    ui: &mut Ui,
    view: &View,
    art: &SkinArt,
    inputs: &mut Inputs,
    actions: &mut Vec<Action>,
) {
    let rect = ui.max_rect();
    let unit = unit(rect.width());
    let origin = pos2(
        rect.left(),
        rect.top() + (rect.height() - ROWS * unit) / 2.0,
    )
    .round_to_pixels(ui.pixels_per_point());
    let at = |x: f32, y: f32| origin + vec2(x, y) * unit;
    let lcd = Rect::from_min_max(origin, at(0.0, TOP) + vec2(20.0 + LCD_WIDTH * unit, 0.0));
    draw_lcd(ui, lcd, view, art, unit);
    let title = Rect::from_min_max(
        pos2(lcd.right() + GAP * unit, origin.y),
        pos2(rect.right(), at(0.0, TITLE_HEIGHT).y),
    );
    draw_title(ui, title, view, art, unit);
    let info = pos2(title.left(), at(0.0, INFO_Y).y);
    info_row(ui, info, rect.right(), view, art, unit, inputs, actions);
    let seek = at(0.0, SEEK_Y);
    seek_bar(ui, seek, rect.right(), view, art, unit, inputs, actions);
    buttons(ui, at(0.0, BUTTONS_Y), view, art, unit, inputs, actions);
}

/// The LCD: the status glyph and the time, at Winamp's spacing.
fn draw_lcd(ui: &Ui, rect: Rect, view: &View, art: &SkinArt, unit: f32) {
    let palette = view.palette;
    let painter = ui.painter().with_clip_rect(rect);
    theme::inset(&painter, rect, palette.lcd_bg, &palette);
    let player = view.player;
    let origin = pos2(
        rect.left() + 10.0,
        rect.top() + (rect.height() - 13.0 * unit) / 2.0,
    )
    .round_to_pixels(ui.pixels_per_point());
    let at = |x: f32, y: f32| origin + vec2(x, y) * unit;

    let status = match player.status {
        Status::Playing | Status::Loading => sprites::STATUS_PLAYING,
        Status::Paused => sprites::STATUS_PAUSED,
        Status::Stopped => sprites::STATUS_STOPPED,
    };
    art.paint(&painter, status, at(0.0, 2.0), unit);

    // Two digits, a colon, two digits.
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
    if time_shown(ui, view) {
        for (x, value) in digits {
            let digit = sprites::digit(art.digits, value);
            art.paint(&painter, digit, at(x, 0.0), unit);
        }
    }
    // The colon is part of Winamp's main window, not of the digits.
    for y in [4.0, 8.0] {
        let dot = Rect::from_min_size(at(44.0, y), vec2(3.0, 1.0) * unit);
        painter.rect_filled(dot, 0.0, palette.lcd);
    }
}

/// The song's display, in the skin's font: the artist above, the song
/// below, each scrolling on its own when too long.
fn draw_title(ui: &Ui, rect: Rect, view: &View, art: &SkinArt, unit: f32) {
    let palette = view.palette;
    let painter = ui.painter().with_clip_rect(rect);
    theme::inset(&painter, rect, palette.lcd_bg, &palette);
    let origin = pos2(rect.left() + 4.0 * unit, rect.top()).round_to_pixels(ui.pixels_per_point());
    let width = rect.right() - 4.0 * unit - origin.x;
    let (artist, song) = now_lines(view);
    for (y, line) in [(4.0, artist), (13.0, song)] {
        let shown = marquee(ui, view, &line, width, unit);
        art.text(&painter, &shown, origin + vec2(0.0, y * unit), width, unit);
    }
}

/// Under the song's display: the bitrate in small displays, then the
/// volume and PL at the right end.
#[allow(clippy::too_many_arguments)]
fn info_row(
    ui: &mut Ui,
    at: Pos2,
    right: f32,
    view: &View,
    art: &SkinArt,
    unit: f32,
    inputs: &mut Inputs,
    actions: &mut Vec<Action>,
) {
    let palette = view.palette;
    let painter = ui.painter().clone();
    let mut x = at.x;
    for (value, label) in [(KBPS, "kbps"), (KHZ, "kHz")] {
        let width = (value.chars().count() * 5) as f32 * unit;
        let display =
            Rect::from_min_size(pos2(x, at.y + unit), vec2(width + 4.0 * unit, 10.0 * unit));
        theme::inset(&painter, display, palette.lcd_bg, &palette);
        art.text(
            &painter,
            value,
            display.min + vec2(2.0, 2.0) * unit,
            width,
            unit,
        );
        let text = painter.text(
            pos2(display.right() + 3.0 * unit, display.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::monospace(9.0),
            palette.chrome,
        );
        x = text.right() + GAP * unit;
    }
    let pl = if view.settings.playlists {
        (sprites::PL_LIT, sprites::PL_LIT_PRESSED)
    } else {
        (sprites::PL, sprites::PL_PRESSED)
    };
    let pl_at = pos2(right - PL * unit, at.y + unit);
    let response = button(ui, &painter, art, pl_at, unit, pl, "Playlists", true);
    pl_clicked(response, view, actions);
    let volume_at = pos2(pl_at.x - (GAP + VOLUME) * unit, at.y);
    if volume_at.x >= x {
        volume(ui, &painter, art, volume_at, unit, inputs, actions);
    }
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

/// Seek, across the strip, with the song's length in a small display at
/// its end.
#[allow(clippy::too_many_arguments)]
fn seek_bar(
    ui: &mut Ui,
    origin: Pos2,
    right: f32,
    view: &View,
    art: &SkinArt,
    unit: f32,
    inputs: &mut Inputs,
    actions: &mut Vec<Action>,
) {
    let palette = view.palette;
    let painter = ui.painter().clone();
    let ready = matches!(view.session, SessionState::Ready { .. });
    let player = view.player;
    let at = |x: f32, y: f32| origin + vec2(x, y) * unit;
    let rect = Rect::from_min_max(origin, pos2(right, origin.y));
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
}

/// Transport, eject, shuffle and repeat.
fn buttons(
    ui: &mut Ui,
    origin: Pos2,
    view: &View,
    art: &SkinArt,
    unit: f32,
    inputs: &mut Inputs,
    actions: &mut Vec<Action>,
) {
    let painter = ui.painter().clone();
    let ready = matches!(view.session, SessionState::Ready { .. });
    let at = |x: f32, y: f32| origin + vec2(x, y) * unit;
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
        if button(ui, &painter, art, at(x, 0.0), unit, sprites, name, ready).clicked() {
            actions.push(action);
        }
        x += normal.width as f32;
    }
    x += GAP;
    let row = 1.0;
    let eject = if view.browse.open {
        (sprites::EJECT_PRESSED, sprites::EJECT_PRESSED)
    } else {
        (sprites::EJECT, sprites::EJECT_PRESSED)
    };
    let response = button(ui, &painter, art, at(x, row), unit, eject, "Browse", ready);
    eject_clicked(response, view, inputs, actions);
    x += EJECT + GAP;
    let shuffle = if view.shuffle {
        (sprites::SHUFFLE_ON, sprites::SHUFFLE_ON_PRESSED)
    } else {
        (sprites::SHUFFLE_OFF, sprites::SHUFFLE_OFF_PRESSED)
    };
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
