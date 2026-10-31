//! The playlist editor: the tabs, the rows, and a toolbar.
//!
//! Tabs: click to show, double-click to rename, drag to reorder, ✕ to
//! close (not Default), + for a new playlist; ▶ marks the playing one.
//!
//! Rows: click selects (Ctrl toggles, Shift extends), double-click or Enter
//! plays, Delete removes, Q marks to play next, Alt+Up/Down or dragging
//! moves the selection, Ctrl+A selects all. Up/Down, PageUp/PageDown and
//! Home/End move the selection; with Shift they extend it. Dragged onto
//! another tab, the selection is copied there, or moved with Shift held
//! (from the Spotify tab, songs to come, unless another device has it);
//! right-click for a menu that does the same, and that loves the songs on
//! Last.fm while it is connected.

use egui::{Align, Id, Layout, Rect, RichText, Ui, UiBuilder, pos2};

use super::scrollbar::Scrollbar;
use super::widgets::{Glyph, small_action, small_glyph, tip};
use super::{Inputs, View, spotify_view};
use crate::action::{Actions, BrowseAction, PlaylistsAction};
use spotless_core::model::{EntryId, Playlist, PlaylistId};

mod menu;
mod rows;
mod selection;
mod tabs;

use rows::draw_rows;
use selection::{keyboard, move_by, remove, selected};
use tabs::{confirm_close, confirm_sync, tab_bar};

pub(super) use menu::{Choice, Menu, all_loved, love, row_menu};
pub(super) use rows::drag_label;
pub use selection::Selection;
pub use tabs::TabScroll;
pub(super) use tabs::{paint_tab, tab_baseline};

const TOOLBAR_HEIGHT: f32 = 18.0;
/// Between the tabs and the rows, and between the rows and the toolbar.
const LIST_GAP: f32 = 16.0;
/// Dragging this close to the list's top or bottom edge scrolls it.
const SCROLL_EDGE: f32 = 24.0;

/// Rows being dragged, in list order. Dropped on another tab, they are
/// copied there, or moved with Shift held.
pub(super) struct RowDrag(pub Vec<EntryId>);

pub fn draw(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
    let onto_tab = tab_bar(ui, view, inputs, actions);
    let selection = &mut inputs.selection;
    let switched = selection.shown != Some(view.playlist.id);
    if switched {
        selection.shown = Some(view.playlist.id);
        selection.glide = None;
        let list = view.playlist;
        let at = list.cursor().and_then(|id| list.index_of(id));
        selection.open_at = Some(at.unwrap_or(0));
    }
    // The item spacing after the tabs makes up the rest of the gap.
    ui.add_space(LIST_GAP - ui.spacing().item_spacing.y);
    if view.playlist.id == PlaylistId::SPOTIFY
        && let Some(queue) = view.spotify
    {
        return spotify_view::draw(ui, view, queue, inputs, onto_tab, actions);
    }
    let list = view.playlist;
    let full = ui.available_rect_before_wrap();
    let toolbar_rect =
        Rect::from_min_max(pos2(full.left(), full.bottom() - TOOLBAR_HEIGHT), full.max);
    let rows = Rect::from_min_max(full.min, pos2(full.right(), toolbar_rect.top() - LIST_GAP));
    let bar = Scrollbar::new(ui, view, Id::new("playlist-scrollbar"));
    let (rows, slot) = bar.split(rows);

    if let Some((jump_list, entry)) = inputs.jump
        && jump_list == list.id
    {
        inputs.jump = None;
        if let Some(index) = list.index_of(entry) {
            let selection = &mut inputs.selection;
            selection.select(entry, index);
            // Placed as on opening the playlist, not just brought into view:
            // at once in a playlist just shown, gliding in the one already.
            selection.reveal = None;
            if switched {
                selection.open_at = Some(index);
            } else {
                selection.glide_to = Some(index);
            }
        }
    }
    inputs
        .selection
        .rows
        .retain(|&id| list.index_of(id).is_some());
    if inputs.renaming.is_none()
        && inputs.closing.is_none()
        && inputs.spotify_pick.is_none()
        && view.sync_name_taken.is_none()
    {
        keyboard(ui, list, &mut inputs.selection, actions);
    }
    ui.scope_builder(UiBuilder::new().max_rect(rows), |ui| {
        if list.is_empty() {
            ui.label(
                RichText::new("This playlist is empty. Press + ADD to find songs.")
                    .color(view.palette.dim),
            );
            bar.paint(ui, slot, 0.0, 0.0, false);
        } else {
            draw_rows(
                ui,
                view,
                &mut inputs.selection,
                (&bar, slot),
                onto_tab,
                actions,
            );
        }
    });
    ui.scope_builder(UiBuilder::new().max_rect(toolbar_rect), |ui| {
        toolbar(ui, view, inputs, actions);
    });
    confirm_close(ui, view, inputs, actions);
    confirm_sync(ui, view, inputs, actions);
}

fn toolbar(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
    let palette = view.palette;
    let list = view.playlist;
    let selected = selected(list, &inputs.selection);
    let any = !selected.is_empty();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let add = small_action(ui, &palette, "+ ADD", true);
        if tip(add, "Add songs from Browse (Ctrl+B)").clicked() {
            super::browse_view::opening(view, inputs);
            actions.push(BrowseAction::ToggleBrowse);
        }
        let rem = small_action(ui, &palette, "REM", any);
        if tip(rem, "Remove the selected songs (Delete)").clicked() {
            remove(list, &mut inputs.selection, selected.clone(), actions);
        }
        let next = small_action(ui, &palette, "Q NEXT", any);
        if tip(next, "Play the selected songs next, or not (Q)").clicked() {
            actions.push(PlaylistsAction::ToggleMarks(selected.clone()));
        }
        let up = small_glyph(ui, &palette, Glyph::Up, any);
        if tip(up, "Move the selected songs up (Alt+↑)").clicked() {
            move_by(list, &mut inputs.selection, selected.clone(), true, actions);
        }
        let down = small_glyph(ui, &palette, Glyph::Down, any);
        if tip(down, "Move the selected songs down (Alt+↓)").clicked() {
            move_by(
                list,
                &mut inputs.selection,
                selected.clone(),
                false,
                actions,
            );
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(summary(list, view.left_ms))
                    .monospace()
                    .size(10.0)
                    .color(palette.dim),
            );
        });
    });
}

/// "12 songs · 54:10", and "· 23:05 left" once some has played.
pub fn summary(list: &Playlist, left_ms: Option<u64>) -> String {
    let total: u64 = list
        .entries()
        .iter()
        .map(|entry| u64::from(entry.track.duration_ms))
        .sum();
    let songs = match list.len() {
        1 => "1 song".to_string(),
        n => format!("{n} songs"),
    };
    match left_ms {
        // Counted up, as the time left of the song is.
        Some(left) => {
            let left = clock(left.div_ceil(1000) * 1000);
            format!("{songs} · {} · {left} left", clock(total))
        }
        None => format!("{songs} · {}", clock(total)),
    }
}

/// "54:10", or "1:02:15" from an hour.
fn clock(ms: u64) -> String {
    let seconds = ms / 1000;
    let (hours, minutes) = (seconds / 3600, seconds / 60 % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{:02}", seconds % 60)
    } else {
        format!("{minutes}:{:02}", seconds % 60)
    }
}
