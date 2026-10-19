//! The player strip: the LCD, seek, transport and volume. With a skin
//! that has them, it is drawn from the skin's bitmaps (`skinned_strip`).

use egui::{Align2, Color32, FontId, Rect, Slider, Ui, UiBuilder, pos2, vec2};

use super::widgets::{Glyph, glyph_button, paint_glyph, text_button};
use super::{Inputs, View, theme};
use crate::action::Action;
use crate::backend::SessionState;
use crate::playback::engine::Status;

pub const HEIGHT: f32 = 84.0;
const LCD_WIDTH: f32 = 340.0;

pub fn draw(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Vec<Action>) {
    if let Some(art) = view.art {
        return super::skinned_strip::draw(ui, view, art, inputs, actions);
    }
    let rect = ui.max_rect();
    let lcd_width = LCD_WIDTH.min(rect.width() * 0.5);
    let lcd = Rect::from_min_size(rect.min, vec2(lcd_width, HEIGHT));
    draw_lcd(ui, lcd, view);

    let controls = Rect::from_min_max(pos2(lcd.right() + 10.0, rect.top()), rect.max);
    ui.scope_builder(UiBuilder::new().max_rect(controls), |ui| {
        controls_column(ui, view, inputs, actions);
    });
}

fn draw_lcd(ui: &Ui, rect: Rect, view: &View) {
    let palette = view.palette;
    let painter = ui.painter().with_clip_rect(rect);
    theme::inset(&painter, rect, palette.lcd_bg, &palette);
    let player = view.player;
    let left = rect.left() + 12.0;

    let glyph = match player.status {
        Status::Playing | Status::Loading => Glyph::Play,
        Status::Paused => Glyph::Pause,
        Status::Stopped => Glyph::Stop,
    };
    paint_glyph(ui, pos2(left + 5.0, rect.top() + 24.0), glyph, palette.lcd);

    let seconds = if player.status == Status::Stopped {
        0
    } else {
        player.position_now() / 1000
    };
    painter.text(
        pos2(left + 20.0, rect.top() + 24.0),
        Align2::LEFT_CENTER,
        format!("{:02}:{:02}", seconds / 60, seconds % 60),
        FontId::monospace(32.0),
        palette.lcd,
    );

    painter.text(
        pos2(left, rect.top() + 54.0),
        Align2::LEFT_CENTER,
        now_line(view),
        FontId::monospace(14.0),
        palette.lcd,
    );
    painter.text(
        pos2(left, rect.top() + 72.0),
        Align2::LEFT_CENTER,
        meta_line(view, '·'),
        FontId::monospace(9.0),
        dimmed(palette.lcd),
    );
}

/// The LCD's main line: what plays, or why nothing does.
pub(super) fn now_line(view: &View) -> String {
    match (&view.player.track, view.player.status) {
        (_, Status::Loading) => "Loading…".to_string(),
        (Some(track), _) => format!("{} - {}", track.artists, track.title),
        (None, _) => match view.session {
            SessionState::Ready { .. } => "Nothing playing".into(),
            _ => "Not signed in".into(),
        },
    }
}

/// The LCD's small line: the bitrate and the album.
pub(super) fn meta_line(view: &View, separator: char) -> String {
    match &view.player.track {
        Some(track) if !track.album.is_empty() => {
            format!("320 KBPS {separator} {}", track.album.to_uppercase())
        }
        _ => format!("320 KBPS {separator} 44 KHZ"),
    }
}

pub(super) fn dimmed(color: Color32) -> Color32 {
    color.gamma_multiply(0.7)
}

fn controls_column(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Vec<Action>) {
    let palette = view.palette;
    let ready = matches!(view.session, SessionState::Ready { .. });
    let player = view.player;
    ui.spacing_mut().item_spacing = vec2(4.0, 10.0);

    // Seek
    let duration = player.track.as_ref().map_or(0, |t| t.duration_ms) as f32 / 1000.0;
    ui.horizontal(|ui| {
        ui.spacing_mut().slider_width = (ui.available_width() - 90.0).max(60.0);
        ui.label(
            egui::RichText::new("SEEK")
                .monospace()
                .size(9.0)
                .color(palette.chrome),
        );
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
        let total = duration as u32;
        ui.label(
            egui::RichText::new(format!("{}:{:02}", total / 60, total % 60))
                .monospace()
                .size(9.0)
                .color(palette.chrome),
        );
    });

    // Transport and volume
    ui.horizontal(|ui| {
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
        if text_button(ui, &palette, "SHUF", view.shuffle, true).clicked() {
            actions.push(Action::SetShuffle(!view.shuffle));
        }
        if text_button(ui, &palette, "REP", view.repeat, true).clicked() {
            actions.push(Action::SetRepeat(!view.repeat));
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let browsing = view.browse.open;
            let label = if browsing { "PLAYLISTS" } else { "BROWSE" };
            if text_button(ui, &palette, label, browsing, ready).clicked() {
                actions.push(Action::ToggleBrowse);
                if !browsing {
                    inputs.focus_search = true;
                }
            }
            ui.add_space(8.0);
            ui.spacing_mut().slider_width = 90.0;
            let mut volume = inputs.volume;
            let response = ui.add(Slider::new(&mut volume, 0..=100).show_value(false));
            if response.changed() {
                inputs.volume = volume;
                actions.push(Action::Volume(volume));
            }
            ui.label(
                egui::RichText::new("VOL")
                    .monospace()
                    .size(9.0)
                    .color(palette.chrome),
            );
        });
    });
}
