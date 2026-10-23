//! The Spotify tab: the account's queue, as Spotify's own apps show it.
//!
//! What played before, dimmed; the song playing; then the songs queued and
//! the rest of the album or playlist. While another device plays, it only
//! shows, with PLAY HERE to take playback over; while Spotless plays it,
//! Delete or REM removes songs to come. Rows drag onto a playlist's tab to
//! copy them there.

use std::collections::BTreeSet;

use egui::{
    Align, Align2, DragAndDrop, FontId, Id, Key, Layout, Rect, RichText, Sense, Ui, UiBuilder,
    pos2, vec2,
};

use super::playlist_view::RowDrag;
use super::scrollbar;
use super::widgets::{drag_ghost, small_action};
use super::{Inputs, View};
use crate::action::{Action, Actions};
use crate::model::{EntryId, PlaylistId};
use crate::playback::engine::Status;
use crate::spotify::queue::{Row, SpotifyQueue};

const ROW_HEIGHT: f32 = 18.0;
const HEADING_HEIGHT: f32 = 24.0;
const TOOLBAR_HEIGHT: f32 = 18.0;
const GAP: f32 = 12.0;

pub fn draw(
    ui: &mut Ui,
    view: &View,
    queue: &SpotifyQueue,
    inputs: &mut Inputs,
    actions: &mut Actions,
) {
    let palette = view.palette;
    let full = ui.available_rect_before_wrap();
    let toolbar_rect =
        Rect::from_min_max(pos2(full.left(), full.bottom() - TOOLBAR_HEIGHT), full.max);
    let rows = Rect::from_min_max(full.min, pos2(full.right(), toolbar_rect.top() - GAP));
    let selection = &mut inputs.selection.rows;
    selection.retain(|&id| queue.row(id).is_some());
    keyboard(ui, queue, selection, inputs.renaming.is_some(), actions);

    ui.scope_builder(UiBuilder::new().max_rect(rows), |ui| {
        ui.label(RichText::new(where_it_plays(queue)).color(palette.chrome));
        ui.add_space(4.0);
        if queue.now.is_none() && queue.parts.is_empty() {
            ui.label(
                RichText::new("Spotify's queue is empty. Play something on another device, or hand it to Spotless.")
                    .color(palette.dim),
            );
            return;
        }
        let id = Id::new("spotify-scrollbar");
        scrollbar::show(ui, view, id, egui::ScrollArea::vertical(), |area, ui| {
            area.show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let mut index = 0;
                let mut row = |ui: &mut Ui, row: Row, index: &mut usize| {
                    draw_row(ui, view, queue, row, *index, selection, actions);
                    *index += 1;
                };
                for track in &queue.before {
                    row(ui, Row::Before(track), &mut index);
                }
                if let Some(now) = &queue.now {
                    heading(ui, view, "Now playing");
                    row(ui, Row::Now(now), &mut index);
                }
                for part in &queue.parts {
                    heading(ui, view, &part.title);
                    for song in &part.songs {
                        row(ui, Row::Next(song), &mut index);
                    }
                }
            })
        });
    });
    ui.scope_builder(UiBuilder::new().max_rect(toolbar_rect), |ui| {
        toolbar(ui, view, queue, selection, actions);
    });
    if let Some(dragging) = DragAndDrop::payload::<RowDrag>(ui.ctx()) {
        let count = match dragging.0.len() {
            1 => "1 song".to_string(),
            n => format!("{n} songs"),
        };
        drag_ghost(ui, &palette, &count);
    }
}

fn where_it_plays(queue: &SpotifyQueue) -> String {
    match (&queue.device, queue.here, queue.status) {
        (None, _, _) => "Nothing is playing on your account.".into(),
        (Some(_), true, Status::Playing) => "Playing here.".into(),
        (Some(_), true, _) => "Paused here.".into(),
        (Some(device), false, Status::Playing) => format!("Playing on {device}."),
        (Some(device), false, _) => format!("Paused on {device}."),
    }
}

fn heading(ui: &mut Ui, view: &View, title: &str) {
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), HEADING_HEIGHT), Sense::hover());
    ui.painter().text(
        pos2(rect.left() + 4.0, rect.bottom() - 4.0),
        Align2::LEFT_BOTTOM,
        title.to_uppercase(),
        FontId::monospace(10.0),
        view.palette.chrome,
    );
}

fn draw_row(
    ui: &mut Ui,
    view: &View,
    queue: &SpotifyQueue,
    row: Row,
    index: usize,
    selection: &mut BTreeSet<EntryId>,
    actions: &mut Actions,
) {
    let palette = view.palette;
    let id = SpotifyQueue::row_id(index);
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), ROW_HEIGHT),
        Sense::click_and_drag(),
    );
    let selected = selection.contains(&id);
    let painter = ui.painter_at(rect);
    if selected {
        painter.rect_filled(rect, 0.0, palette.selected_bg);
    }
    let color = match row {
        Row::Before(_) => palette.dim,
        Row::Now(_) => palette.current,
        Row::Next(_) => palette.list_fg,
    };
    let track = row.track();
    let mono = FontId::monospace(11.0);
    let seconds = track.duration_ms / 1000;
    let right = painter.text(
        rect.right_center() - vec2(6.0, 0.0),
        Align2::RIGHT_CENTER,
        format!("{}:{:02}", seconds / 60, seconds % 60),
        mono,
        color,
    );
    let artists: Vec<&str> = track.artists.iter().map(|a| a.name.as_str()).collect();
    let title = if artists.is_empty() {
        track.title.clone()
    } else {
        format!("{} - {}", artists.join(", "), track.title)
    };
    painter
        .with_clip_rect(Rect::from_min_max(
            rect.min,
            pos2(right.left() - 10.0, rect.bottom()),
        ))
        .text(
            pos2(rect.left() + 4.0, rect.center().y),
            Align2::LEFT_CENTER,
            title,
            FontId::proportional(12.5),
            color,
        );

    if response.double_clicked() && !queue.here {
        actions.push(Action::PlaySpotifyHere);
    } else if response.clicked() {
        if ui.input(|i| i.modifiers.command) {
            if !selection.remove(&id) {
                selection.insert(id);
            }
        } else {
            *selection = BTreeSet::from([id]);
        }
    }
    if response.drag_started() {
        if !selected {
            *selection = BTreeSet::from([id]);
        }
        // In the tab's order: the IDs count down.
        let rows = selection.iter().rev().copied().collect();
        response.dnd_set_drag_payload(RowDrag(rows));
    }
}

/// The selected songs to come, which can be removed.
fn removable(queue: &SpotifyQueue, selection: &BTreeSet<EntryId>) -> Vec<EntryId> {
    selection
        .iter()
        .copied()
        .filter(|&id| matches!(queue.row(id), Some(Row::Next(_))))
        .collect()
}

fn toolbar(
    ui: &mut Ui,
    view: &View,
    queue: &SpotifyQueue,
    selection: &BTreeSet<EntryId>,
    actions: &mut Actions,
) {
    let palette = view.palette;
    let removable = removable(queue, selection);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if small_action(ui, &palette, "+ ADD", queue.playing_here()).clicked() {
            actions.push(Action::ToggleBrowse);
        }
        let can_remove = queue.playing_here() && !removable.is_empty();
        if small_action(ui, &palette, "REM", can_remove).clicked() {
            actions.push(Action::Remove(PlaylistId::SPOTIFY, removable));
        }
        let elsewhere = !queue.here && (queue.device.is_some() || queue.now.is_some());
        if small_action(ui, &palette, "PLAY HERE", elsewhere).clicked() {
            actions.push(Action::PlaySpotifyHere);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let songs = match queue.len() {
                1 => "1 song to come".to_string(),
                n => format!("{n} songs to come"),
            };
            ui.label(
                RichText::new(songs)
                    .monospace()
                    .size(10.0)
                    .color(palette.dim),
            );
        });
    });
}

fn keyboard(
    ui: &Ui,
    queue: &SpotifyQueue,
    selection: &BTreeSet<EntryId>,
    renaming: bool,
    actions: &mut Actions,
) {
    if renaming || ui.ctx().text_edit_focused() || !queue.playing_here() {
        return;
    }
    if ui.input(|i| i.key_pressed(Key::Delete)) {
        let rows = removable(queue, selection);
        if !rows.is_empty() {
            actions.push(Action::Remove(PlaylistId::SPOTIFY, rows));
        }
    }
}
