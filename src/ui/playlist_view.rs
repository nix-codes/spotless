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
//!
//! Find (Ctrl+F): a field in place of the toolbar narrows the rows to the
//! songs with every word typed in their name, artists' or album's, as it's
//! typed. ↑/↓ and Enter work from it; Esc closes it. Narrowed, songs aren't
//! moved or sorted: where one would land among those hidden can't be seen.

use egui::{
    Align, Id, Key, Layout, Modifiers, Rect, RichText, TextEdit, Ui, UiBuilder, pos2, vec2,
};

use super::scrollbar::Scrollbar;
use super::widgets::{
    Glyph, KeyTag, SMALL_HEIGHT, field, sized_toggle, small_action, small_glyph, tip,
};
use super::{Inputs, View, spotify_view};
use crate::action::{Actions, BrowseAction, PlaybackAction, PlaylistsAction};
use spotless_core::browse::Find;
use spotless_core::model::{EntryId, Order, Playlist, PlaylistId};

mod menu;
mod rows;
mod selection;
mod tabs;

use rows::draw_rows;
use selection::{keyboard, move_by, remove, selected, step};
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
const FIND_ID: &str = "playlist-find";

/// Rows being dragged, in list order. Dropped on another tab, they are
/// copied there, or moved with Shift held.
pub(super) struct RowDrag(pub Vec<EntryId>);

pub fn draw(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Actions) {
    let onto_tab = tab_bar(ui, view, inputs, actions);
    let selection = &mut inputs.selection;
    let switched = selection.shown != Some(view.playlist.id);
    if switched {
        // Another playlist's songs aren't what was being found.
        if selection.shown.is_some() {
            selection.find = None;
        }
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

    find(list, &mut inputs.selection);
    if let Some((jump_list, entry)) = inputs.jump
        && jump_list == list.id
    {
        inputs.jump = None;
        if let Some(index) = list.index_of(entry) {
            // A song the find hides is shown by closing it.
            if !inputs.selection.shows(index) {
                inputs.selection.close_find(list);
            }
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
        let nothing_found = inputs.selection.found.as_ref().is_some_and(Vec::is_empty);
        if list.is_empty() || nothing_found {
            let note = if list.is_empty() {
                "This playlist is empty. Press + ADD to find songs."
            } else {
                "No song here has all of that in its name, its artists' or its album's."
            };
            ui.label(RichText::new(note).color(view.palette.dim));
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
        if inputs.selection.find.is_some() {
            find_bar(ui, view, &mut inputs.selection, actions);
        } else {
            toolbar(ui, view, inputs, actions);
        }
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
        if tip(add, "Add songs from Browse (Ctrl+B)")
            .key_tag("Ctrl+B")
            .clicked()
        {
            super::browse_view::opening(view, inputs);
            actions.push(BrowseAction::ToggleBrowse);
        }
        let rem = small_action(ui, &palette, "REM", any);
        if tip(rem, "Remove the selected songs (Delete)")
            .key_tag("Del")
            .clicked()
        {
            remove(list, &mut inputs.selection, selected.clone(), actions);
        }
        let next = small_action(ui, &palette, "Q NEXT", any);
        if tip(next, "Play the selected songs next, or not (Q)")
            .key_tag("Q")
            .clicked()
        {
            actions.push(PlaylistsAction::ToggleMarks(selected.clone()));
        }
        let up = small_glyph(ui, &palette, Glyph::Up, any);
        if tip(up, "Move the selected songs up (Alt+↑)")
            .key_tag("Alt+↑")
            .clicked()
        {
            move_by(list, &mut inputs.selection, selected.clone(), true, actions);
        }
        let down = small_glyph(ui, &palette, Glyph::Down, any);
        if tip(down, "Move the selected songs down (Alt+↓)")
            .key_tag("Alt+↓")
            .clicked()
        {
            move_by(
                list,
                &mut inputs.selection,
                selected.clone(),
                false,
                actions,
            );
        }
        sort_menu(ui, view, actions);
        let headers = sized_toggle(ui, &palette, "HEADERS", list.headers_on());
        if tip(headers, "Headers in this playlist, on or off").clicked() {
            actions.push(PlaylistsAction::SetHeaders(list.id, !list.headers_on()));
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

/// Works out the songs the find has, keeps the selection to them, and
/// when what's typed changed, moves it to the first of them unless it's
/// on one already.
fn find(list: &Playlist, selection: &mut selection::Selection) {
    let Some(text) = &mut selection.find else {
        selection.found = None;
        return;
    };
    let changed = std::mem::take(&mut text.changed);
    let find = Find::new(&text.text);
    selection.found = (!find.is_empty()).then(|| {
        list.entries()
            .iter()
            .enumerate()
            .filter(|(_, entry)| find.track(&entry.track))
            .map(|(index, _)| index)
            .collect()
    });
    let Some(found) = &selection.found else {
        return;
    };
    let shown: Vec<EntryId> = found.iter().map(|&i| list.entries()[i].id).collect();
    selection.rows.retain(|id| shown.contains(id));
    let on_one = selection.cursor.is_some_and(|id| shown.contains(&id));
    if changed && !on_one {
        match found.first() {
            Some(&first) => selection.select(list.entries()[first].id, first),
            None => {
                selection.rows.clear();
                selection.cursor = None;
                selection.anchor = None;
            }
        }
    }
}

/// The find, in place of the toolbar: the field, CLOSE, and how many of
/// the songs it has. ↑/↓, PgUp/PgDn and Enter in the field work on those
/// as in the list; Esc closes it.
fn find_bar(ui: &mut Ui, view: &View, selection: &mut selection::Selection, actions: &mut Actions) {
    let palette = view.palette;
    let list = view.playlist;
    let id = Id::new(FIND_ID);
    // The field's keys, taken before it sees them.
    if ui.memory(|m| m.has_focus(id)) {
        let (escape, enter, moved) = ui.input_mut(|i| {
            let moves = [Key::ArrowUp, Key::ArrowDown, Key::PageUp, Key::PageDown];
            let mut moved = None;
            for key in moves {
                if i.consume_key(Modifiers::NONE, key) {
                    moved = Some((key, false));
                }
                if i.consume_key(Modifiers::SHIFT, key) {
                    moved = Some((key, true));
                }
            }
            (
                i.consume_key(Modifiers::NONE, Key::Escape),
                i.consume_key(Modifiers::NONE, Key::Enter),
                moved,
            )
        });
        if escape {
            selection.close_find(list);
            ui.memory_mut(|m| m.surrender_focus(id));
            return;
        }
        if let Some((key, extend)) = moved {
            step(list, selection, key, extend);
        }
        if enter && let Some(&first) = selected(list, selection).first() {
            actions.push(PlaybackAction::PlayEntry(list.id, first));
        }
    }
    let Some(text) = &mut selection.find else {
        return;
    };
    let summary = match &selection.found {
        Some(found) => format!("{} of {}", found.len(), songs(list.len())),
        None => songs(list.len()),
    };
    let closed = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let label = RichText::new("FIND")
            .monospace()
            .size(10.0)
            .color(palette.dim);
        ui.label(label);
        let close_width = 52.0;
        let summary_width = summary.len() as f32 * 6.5 + 8.0;
        let width = (ui.available_width() - close_width - summary_width).clamp(80.0, 320.0);
        let edit = TextEdit::singleline(&mut text.text)
            .id(id)
            .hint_text("A song, artist or album")
            .font(egui::FontId::proportional(12.0))
            .margin(vec2(4.0, 1.0))
            .desired_width(width)
            .min_size(vec2(0.0, SMALL_HEIGHT));
        let response = field(ui, edit);
        if std::mem::take(&mut text.focus) {
            response.request_focus();
        }
        if response.changed() {
            text.changed = true;
            ui.ctx().request_repaint();
        }
        let close = small_action(ui, &palette, "CLOSE", true);
        let close = tip(close, "Close the find, and show all the songs (Esc)").key_tag("Esc");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(&summary)
                    .monospace()
                    .size(10.0)
                    .color(palette.dim),
            );
        });
        close.clicked()
    });
    if closed.inner {
        selection.close_find(list);
    }
}

/// SORT: what reorders the playlist, once.
fn sort_menu(ui: &mut Ui, view: &View, actions: &mut Actions) {
    let list = view.playlist;
    let button = small_action(ui, &view.palette, "SORT ▾", list.len() > 1);
    let button = tip(button, "Sort or shuffle this playlist");
    egui::Popup::menu(&button).show(|ui| {
        ui.set_min_width(320.0);
        let mut item = |ui: &mut Ui, order: Order, name: &str, key: &str| {
            let name = RichText::new(name).monospace().size(11.0);
            let key = RichText::new(key)
                .monospace()
                .size(10.0)
                .color(view.palette.dim);
            let item = egui::Button::new(name)
                .right_text(key)
                .frame_when_inactive(false);
            if ui.add(item).clicked() {
                actions.push(PlaylistsAction::Reorder(list.id, order));
            }
        };
        item(
            ui,
            Order::ArtistAlbums,
            "Sort by artist, albums by date",
            "Ctrl+S",
        );
        item(
            ui,
            Order::ArtistShuffled,
            "Sort by artist, songs shuffled",
            "",
        );
        ui.separator();
        item(ui, Order::Shuffled, "Shuffle all songs", "Ctrl+Shift+R");
        item(ui, Order::Reversed, "Reverse", "Ctrl+R");
    });
}

/// "12 songs · 54:10", and "· 23:05 left" once some has played.
pub fn summary(list: &Playlist, left_ms: Option<u64>) -> String {
    let total: u64 = list
        .entries()
        .iter()
        .map(|entry| u64::from(entry.track.duration_ms))
        .sum();
    let songs = songs(list.len());
    match left_ms {
        // Counted up, as the time left of the song is.
        Some(left) => {
            let left = clock(left.div_ceil(1000) * 1000);
            format!("{songs} · {} · {left} left", clock(total))
        }
        None => format!("{songs} · {}", clock(total)),
    }
}

/// "1 song", "12 songs".
fn songs(count: usize) -> String {
    match count {
        1 => "1 song".to_string(),
        n => format!("{n} songs"),
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

#[cfg(test)]
mod tests {
    use egui::Key;

    use super::selection::{FindText, Selection, step};
    use super::*;
    use spotless_core::{Controller, Paths};

    /// The demo's Night Drive: 32 songs, Kestrel & the Tide's 17th to 20th.
    fn night_drive() -> Playlist {
        let root = std::env::temp_dir().join(format!("spotless-find-{}", std::process::id()));
        let paths = Paths::from_dirs(root.join("config"), root.join("cache")).unwrap();
        let controller = Controller::demo(paths);
        let state = controller.state();
        state.list(state.viewed).unwrap().clone()
    }

    fn finding(list: &Playlist, text: &str, selection: &mut Selection) {
        selection.find = Some(FindText {
            text: text.into(),
            changed: true,
            ..FindText::default()
        });
        find(list, selection);
    }

    #[test]
    fn a_find_narrows_to_every_word_and_selects_the_first_found() {
        let list = night_drive();
        let mut selection = Selection::default();
        selection.rows.insert(list.entries()[0].id);
        finding(&list, "KESTREL tide", &mut selection);
        assert_eq!(selection.found, Some(vec![16, 17, 18, 19]));
        assert_eq!(
            selection.rows.iter().copied().collect::<Vec<_>>(),
            vec![list.entries()[16].id]
        );
        assert!(selection.narrowed());
        finding(&list, "  ", &mut selection);
        assert!(!selection.narrowed(), "no words: all shown");
    }

    #[test]
    fn the_keys_go_over_the_songs_found_only() {
        let list = night_drive();
        let mut selection = Selection::default();
        finding(&list, "kestrel", &mut selection);
        let at = |selection: &Selection| list.index_of(selection.cursor.unwrap());
        step(&list, &mut selection, Key::ArrowUp, false);
        assert_eq!(at(&selection), Some(16), "the first stays the first");
        step(&list, &mut selection, Key::End, false);
        assert_eq!(at(&selection), Some(19));
        step(&list, &mut selection, Key::Home, true);
        assert_eq!(selection.rows.len(), 4, "extended over the four found");
    }

    #[test]
    fn closing_the_find_shows_all_and_keeps_the_song() {
        let list = night_drive();
        let mut selection = Selection::default();
        finding(&list, "lighthouse", &mut selection);
        selection.close_find(&list);
        find(&list, &mut selection);
        assert!(!selection.narrowed());
        assert_eq!(selection.open_at, Some(17));
        assert!(selection.rows.contains(&list.entries()[17].id));
    }
}
