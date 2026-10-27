//! The Spotify tab: the account's queue, as Spotify's own apps show it.
//!
//! What played before, dimmed; the song playing; then the songs queued and
//! the rest of the album or playlist. Enter or a double-click on a song to
//! come plays the queue here from it, taking it over from a local playlist
//! or another device; on the song playing, plays it here. While another
//! device holds the queue, the tab only shows it, and TAKE OVER brings it
//! here, paused. While Spotless has it, or no device has, + ADD adds songs
//! to it and Delete or REM removes songs to come (taking it over), and
//! CLEAR stops and empties it. Rows drag onto a playlist's tab to copy them
//! there, or, then, to move songs to come with Shift held; right-click for
//! a menu that does the same, to a new playlist too, or removes them.
//!
//! Keys as in a playlist: Up/Down, PageUp/PageDown and Home/End move the
//! selection, with Shift they extend it; Ctrl+A selects all.

use std::collections::BTreeSet;

use egui::{
    Align, Align2, DragAndDrop, FontId, Id, Key, Layout, Rect, RichText, Sense, Ui, UiBuilder,
    pos2, vec2,
};

use super::playlist_view::{self, Choice, Menu, RowDrag};
use super::scrollbar;
use super::widgets::{drag_ghost, small_action};
use super::{Inputs, View};
use crate::action::{Action, Actions};
use crate::model::{EntryId, PlaylistId};
use crate::spotify::queue::{Row, SpotifyQueue};

const ROW_HEIGHT: f32 = 18.0;
const HEADING_HEIGHT: f32 = 24.0;
const TOOLBAR_HEIGHT: f32 = 18.0;
const GAP: f32 = 12.0;

/// Where the keys move from, kept between frames.
#[derive(Clone, Copy, Default)]
struct Keys {
    /// Where Shift extends from.
    anchor: Option<EntryId>,
    /// The row the arrows move from.
    cursor: Option<EntryId>,
    /// The row to bring into view.
    reveal: Option<EntryId>,
}

fn keys_id() -> Id {
    Id::new("spotify-keys")
}

pub fn draw(
    ui: &mut Ui,
    view: &View,
    queue: &SpotifyQueue,
    inputs: &mut Inputs,
    onto_tab: bool,
    actions: &mut Actions,
) {
    let palette = view.palette;
    let mut chosen = None;
    let full = ui.available_rect_before_wrap();
    let toolbar_rect =
        Rect::from_min_max(pos2(full.left(), full.bottom() - TOOLBAR_HEIGHT), full.max);
    let rows = Rect::from_min_max(full.min, pos2(full.right(), toolbar_rect.top() - GAP));
    let selection = &mut inputs.selection.rows;
    selection.retain(|&id| queue.row(id).is_some());
    let mut keys: Keys = ui.data(|d| d.get_temp(keys_id())).unwrap_or_default();
    let page = ((rows.height() / ROW_HEIGHT) as usize)
        .saturating_sub(3)
        .max(1);
    if inputs.renaming.is_none() && inputs.closing.is_none() {
        keyboard(ui, queue, selection, &mut keys, page, actions);
    }

    ui.scope_builder(UiBuilder::new().max_rect(rows), |ui| {
        ui.label(RichText::new(queue.where_it_plays()).color(palette.chrome));
        ui.add_space(4.0);
        if queue.now.is_none() && queue.parts.is_empty() {
            ui.label(
                RichText::new(if queue.takes_songs() {
                    "Nothing to play. Add songs with + ADD, or play something on another device."
                } else {
                    "Nothing to play. Play something on another device, or hand it to Spotless."
                })
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
                    chosen = chosen.or(draw_row(
                        ui,
                        view,
                        queue,
                        row,
                        *index,
                        (selection, &mut keys),
                        actions,
                    ));
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
    keys.reveal = None;
    ui.data_mut(|d| d.insert_temp(keys_id(), keys));
    if let Some(choice) = chosen {
        // In the tab's order: the IDs count down.
        let tracks = || {
            selection
                .iter()
                .rev()
                .filter_map(|&id| queue.row(id))
                .map(|row| row.track().clone())
                .collect()
        };
        match choice {
            Choice::Copy(Some(to)) => actions.push(Action::CopyTo(to, tracks())),
            Choice::Copy(None) => actions.push(Action::NewPlaylistWith(tracks())),
            Choice::Remove => actions.push(Action::Remove(
                PlaylistId::SPOTIFY,
                removable(queue, selection),
            )),
            Choice::Move(Some(to)) => {
                let rows = selection.iter().rev().copied().collect();
                actions.push(Action::MoveTo(PlaylistId::SPOTIFY, to, rows));
            }
            Choice::Move(None) => {
                actions.push(Action::NewPlaylistWith(tracks()));
                actions.push(Action::Remove(
                    PlaylistId::SPOTIFY,
                    removable(queue, selection),
                ));
            }
            Choice::ToggleMarks => {}
        }
    }
    ui.scope_builder(UiBuilder::new().max_rect(toolbar_rect), |ui| {
        toolbar(ui, view, queue, selection, actions);
    });
    if let Some(dragging) = DragAndDrop::payload::<RowDrag>(ui.ctx()) {
        let movable = queue.movable(&dragging.0);
        let label = playlist_view::drag_label(ui, dragging.0.len(), onto_tab, movable);
        drag_ghost(ui, &palette, &label);
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
    (selection, keys): (&mut BTreeSet<EntryId>, &mut Keys),
    actions: &mut Actions,
) -> Option<Choice> {
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

    if keys.reveal == Some(id) {
        response.scroll_to_me(None);
    }
    if response.double_clicked() {
        actions.push(Action::PlayEntry(PlaylistId::SPOTIFY, id));
    } else if response.clicked() {
        let modifiers = ui.input(|i| i.modifiers);
        if modifiers.shift
            && let Some(anchor) = keys.anchor.and_then(|a| index_of(queue, a))
        {
            *selection = (anchor.min(index)..=anchor.max(index))
                .map(SpotifyQueue::row_id)
                .collect();
            keys.cursor = Some(id);
            return None;
        }
        if modifiers.command {
            if !selection.remove(&id) {
                selection.insert(id);
            }
        } else {
            *selection = BTreeSet::from([id]);
        }
        keys.anchor = Some(id);
        keys.cursor = Some(id);
    } else if response.secondary_clicked() && !selected {
        *selection = BTreeSet::from([id]);
        keys.anchor = Some(id);
        keys.cursor = Some(id);
    }
    let open = response.context_menu_opened();
    let rows: Vec<EntryId> = if open {
        selection.iter().copied().collect()
    } else {
        Vec::new()
    };
    let menu = Menu {
        unmovable: unmovable(queue, &rows),
        marked: None,
        can_remove: open && queue.takes_songs() && !removable(queue, selection).is_empty(),
    };
    let chosen = playlist_view::row_menu(&response, view, menu);
    if response.drag_started() {
        if !selected {
            *selection = BTreeSet::from([id]);
        }
        // In the tab's order: the IDs count down.
        let rows = selection.iter().rev().copied().collect();
        response.dnd_set_drag_payload(RowDrag(rows));
    }
    chosen
}

/// Why these rows can't move to a playlist, when they can't.
fn unmovable(queue: &SpotifyQueue, rows: &[EntryId]) -> Option<&'static str> {
    if queue.movable(rows) {
        None
    } else if !queue.takes_songs() {
        Some("Another device has the queue. TAKE OVER the queue first.")
    } else {
        Some("Only songs to come can leave the queue.")
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
        // Held elsewhere, the tab only shows the queue until taken over.
        if let Some(device) = queue.device.as_ref().filter(|_| queue.held_elsewhere())
            && small_action(ui, &palette, "TAKE OVER", true)
                .on_hover_text(format!(
                    "Takes the queue from {device}, paused, to change it here"
                ))
                .clicked()
        {
            actions.push(Action::TakeOverSpotify);
        }
        if small_action(ui, &palette, "+ ADD", queue.takes_songs()).clicked() {
            actions.push(Action::ToggleBrowse);
        }
        let can_remove = queue.takes_songs() && !removable.is_empty();
        if small_action(ui, &palette, "REM", can_remove).clicked() {
            actions.push(Action::Remove(PlaylistId::SPOTIFY, removable));
        }
        let something = queue.now.is_some() || !queue.parts.is_empty();
        let can_clear = something && !queue.held_elsewhere();
        if small_action(ui, &palette, "CLEAR", can_clear).clicked() {
            actions.push(Action::ClearSpotify);
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

/// The place of the row with this ID.
fn index_of(queue: &SpotifyQueue, id: EntryId) -> Option<usize> {
    (0..queue.rows().count()).find(|&i| SpotifyQueue::row_id(i) == id)
}

/// The tab's keys, unless a text field has the keyboard.
fn keyboard(
    ui: &Ui,
    queue: &SpotifyQueue,
    selection: &mut BTreeSet<EntryId>,
    keys: &mut Keys,
    page: usize,
    actions: &mut Actions,
) {
    let count = queue.rows().count();
    if ui.ctx().text_edit_focused() || count == 0 {
        return;
    }
    let (pressed, modifiers) = ui.input(|i| {
        let keys = [
            Key::Enter,
            Key::Delete,
            Key::A,
            Key::ArrowUp,
            Key::ArrowDown,
            Key::Home,
            Key::End,
            Key::PageUp,
            Key::PageDown,
        ];
        (
            keys.into_iter().find(|&key| i.key_pressed(key)),
            i.modifiers,
        )
    });
    let Some(key) = pressed else { return };
    match key {
        Key::Enter => {
            let row = keys
                .cursor
                .filter(|&id| queue.row(id).is_some())
                .unwrap_or(SpotifyQueue::row_id(queue.before.len()));
            actions.push(Action::PlayEntry(PlaylistId::SPOTIFY, row));
        }
        Key::Delete if queue.takes_songs() => {
            let rows = removable(queue, selection);
            if !rows.is_empty() {
                actions.push(Action::Remove(PlaylistId::SPOTIFY, rows));
            }
        }
        Key::A if modifiers.command => {
            *selection = (0..count).map(SpotifyQueue::row_id).collect();
        }
        Key::ArrowUp | Key::ArrowDown | Key::Home | Key::End | Key::PageUp | Key::PageDown
            if !modifiers.alt && !modifiers.command =>
        {
            // From the row last moved to, or the song playing.
            let from = keys
                .cursor
                .or(keys.anchor)
                .and_then(|id| index_of(queue, id));
            let last = count - 1;
            let to = match (key, from) {
                (Key::Home, _) => 0,
                (Key::End, _) => last,
                (_, None) => queue.before.len().min(last),
                (Key::ArrowUp, Some(i)) => i.saturating_sub(1),
                (Key::ArrowDown, Some(i)) => (i + 1).min(last),
                (Key::PageUp, Some(i)) => i.saturating_sub(page),
                (Key::PageDown, Some(i)) => (i + page).min(last),
                _ => 0,
            };
            let id = SpotifyQueue::row_id(to);
            if modifiers.shift {
                let anchor = keys
                    .anchor
                    .and_then(|id| index_of(queue, id))
                    .unwrap_or(from.unwrap_or(to));
                keys.anchor = Some(SpotifyQueue::row_id(anchor));
                *selection = (anchor.min(to)..=anchor.max(to))
                    .map(SpotifyQueue::row_id)
                    .collect();
            } else {
                *selection = BTreeSet::from([id]);
                keys.anchor = Some(id);
            }
            keys.cursor = Some(id);
            keys.reveal = Some(id);
        }
        _ => {}
    }
}
