//! The selected rows: clicks, the keys, and removing or moving them.

use std::collections::BTreeSet;

use egui::{Key, Modifiers, Ui};

use super::rows::Glide;
use crate::action::{Actions, PlaybackAction, PlaylistsAction};
use spotless_core::model::{EntryId, Playlist, PlaylistId};

/// The editor's own state, kept between frames.
#[derive(Default)]
pub struct Selection {
    pub rows: BTreeSet<EntryId>,
    /// Where Shift+click and Shift+arrows extend from.
    pub(super) anchor: Option<EntryId>,
    /// The row the arrow keys move from, framed when several are selected.
    pub(super) cursor: Option<EntryId>,
    /// Rows that fit in the list, for PageUp and PageDown.
    pub(super) page: usize,
    /// The scroll offset last frame, and the row to bring into view.
    pub(super) offset: f32,
    pub(super) reveal: Option<usize>,
    /// The playlist shown last frame, and the song to open another on,
    /// with the line above it at the top.
    pub(super) shown: Option<PlaylistId>,
    pub(super) open_at: Option<usize>,
    /// The song to glide to, placed as on opening (J), and the glide under way.
    pub(super) glide_to: Option<usize>,
    pub(super) glide: Option<Glide>,
    /// The find in the toolbar, while it's open.
    pub(super) find: Option<FindText>,
    /// The songs the find has, by index in order, worked out each frame
    /// from it: None while it has no words, all shown.
    pub(super) found: Option<Vec<usize>>,
}

/// What's typed in the find; whether the keyboard goes to it next frame;
/// and whether it changed, for the selection to go to what it found.
#[derive(Default)]
pub(super) struct FindText {
    pub text: String,
    pub focus: bool,
    pub changed: bool,
}

impl Selection {
    /// Selects one row and scrolls to it.
    pub(super) fn select(&mut self, id: EntryId, index: usize) {
        self.rows = BTreeSet::from([id]);
        self.anchor = Some(id);
        self.cursor = Some(id);
        self.reveal = Some(index);
    }

    /// Ctrl+F: the find, with the keyboard in it.
    pub fn open_find(&mut self) {
        self.find.get_or_insert_with(FindText::default).focus = true;
    }

    /// The list is narrowed to what the find has: songs can't be moved.
    pub fn narrowed(&self) -> bool {
        self.found.is_some()
    }

    /// Closes the find, the song the keyboard is on placed as on opening
    /// the playlist.
    pub(super) fn close_find(&mut self, list: &Playlist) {
        self.find = None;
        self.found = None;
        self.open_at = self.cursor.and_then(|id| list.index_of(id));
    }

    /// Whether the song at `index` is shown.
    pub(super) fn shows(&self, index: usize) -> bool {
        self.found
            .as_ref()
            .is_none_or(|found| found.binary_search(&index).is_ok())
    }

    /// The songs shown, by index.
    pub(super) fn visible(&self, list: &Playlist) -> Vec<usize> {
        match &self.found {
            Some(found) => found.clone(),
            None => (0..list.len()).collect(),
        }
    }
}

pub(super) fn click(list: &Playlist, selection: &mut Selection, id: EntryId, modifiers: Modifiers) {
    if modifiers.shift
        && let Some(anchor) = selection.anchor
        && let (Some(a), Some(b)) = (list.index_of(anchor), list.index_of(id))
    {
        let range = a.min(b)..=a.max(b);
        if !modifiers.command {
            selection.rows.clear();
        }
        let shown = range.filter(|&index| selection.shows(index));
        let ids: Vec<EntryId> = shown.map(|index| list.entries()[index].id).collect();
        selection.rows.extend(ids);
        selection.cursor = Some(id);
        return;
    }
    if modifiers.command {
        if !selection.rows.remove(&id) {
            selection.rows.insert(id);
        }
    } else {
        selection.rows = BTreeSet::from([id]);
    }
    selection.anchor = Some(id);
    selection.cursor = Some(id);
}

/// The selected rows, in list order.
pub(super) fn selected(list: &Playlist, selection: &Selection) -> Vec<EntryId> {
    list.entries()
        .iter()
        .map(|entry| entry.id)
        .filter(|id| selection.rows.contains(id))
        .collect()
}

/// Removes the selection; the row after the last removed one is selected.
pub(super) fn remove(
    list: &Playlist,
    selection: &mut Selection,
    rows: Vec<EntryId>,
    actions: &mut Actions,
) {
    if select_after(list, selection, &rows) {
        actions.push(PlaylistsAction::Remove(list.id, rows));
    }
}

/// Selects the row after the last of `rows`, which are going; says whether
/// there were any.
pub(super) fn select_after(list: &Playlist, selection: &mut Selection, rows: &[EntryId]) -> bool {
    let Some(last) = rows.iter().filter_map(|&id| list.index_of(id)).max() else {
        return false;
    };
    let next = (last + 1..list.len())
        .chain((0..last).rev())
        .filter(|&index| selection.shows(index))
        .map(|index| list.entries()[index].id)
        .find(|id| !selection.rows.contains(id));
    selection.rows = next.into_iter().collect();
    selection.anchor = next;
    selection.cursor = next;
    true
}

/// Moves the selection one row up or down.
pub(super) fn move_by(
    list: &Playlist,
    selection: &mut Selection,
    rows: Vec<EntryId>,
    up: bool,
    actions: &mut Actions,
) {
    let indices: Vec<usize> = rows.iter().filter_map(|&id| list.index_of(id)).collect();
    let (Some(&first), Some(&last)) = (indices.first(), indices.last()) else {
        return;
    };
    if up && first > 0 {
        actions.push(PlaylistsAction::Move(rows, first - 1));
        selection.reveal = Some(first - 1);
    } else if !up && last + 1 < list.len() {
        actions.push(PlaylistsAction::Move(rows, last + 2));
        selection.reveal = Some(last + 1);
    }
}

/// The editor's keys, unless a text field has the keyboard. Narrowed by
/// the find, they go over the songs shown, and Esc closes it.
pub(super) fn keyboard(ui: &Ui, list: &Playlist, selection: &mut Selection, actions: &mut Actions) {
    if ui.ctx().text_edit_focused() || list.is_empty() {
        return;
    }
    if selection.find.is_some() && ui.input(|i| i.key_pressed(Key::Escape)) {
        selection.close_find(list);
        return;
    }
    let rows = selected(list, selection);
    let (pressed, modifiers) = ui.input(|i| {
        let keys = [
            Key::Enter,
            Key::Delete,
            Key::Q,
            Key::ArrowUp,
            Key::ArrowDown,
            Key::A,
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
            if let Some(&first) = rows.first() {
                actions.push(PlaybackAction::PlayEntry(list.id, first));
            }
        }
        Key::Delete if !rows.is_empty() => remove(list, selection, rows, actions),
        Key::Q if !rows.is_empty() => actions.push(PlaylistsAction::ToggleMarks(rows)),
        Key::A if modifiers.command => {
            let shown = selection.visible(list);
            selection.rows = shown.iter().map(|&i| list.entries()[i].id).collect();
        }
        Key::ArrowUp | Key::ArrowDown if modifiers.alt && !rows.is_empty() => {
            if !selection.narrowed() {
                move_by(list, selection, rows, key == Key::ArrowUp, actions);
            }
        }
        Key::ArrowUp | Key::ArrowDown | Key::Home | Key::End | Key::PageUp | Key::PageDown
            if !modifiers.alt && !modifiers.command =>
        {
            step(list, selection, key, modifiers.shift);
        }
        _ => {}
    }
}

/// Moves the selection by an arrow, page or Home/End key, over the songs
/// shown; with `extend`, from the anchor to there.
pub(super) fn step(list: &Playlist, selection: &mut Selection, key: Key, extend: bool) {
    let shown = selection.visible(list);
    let Some(last) = shown.len().checked_sub(1) else {
        return;
    };
    // Where a song is among those shown; one hidden counts as the next.
    let at = |index: usize| match shown.binary_search(&index) {
        Ok(at) | Err(at) => at.min(last),
    };
    let from = selection
        .cursor
        .or(selection.anchor)
        .and_then(|id| list.index_of(id))
        .or(list.cursor().and_then(|id| list.index_of(id)))
        .map(at);
    let page = selection.page.max(1);
    let to = match (key, from) {
        (Key::Home, _) => 0,
        (Key::End, _) => last,
        (Key::ArrowUp, Some(i)) => i.saturating_sub(1),
        (Key::ArrowDown, Some(i)) => (i + 1).min(last),
        (Key::PageUp, Some(i)) => i.saturating_sub(page),
        (Key::PageDown, Some(i)) => (i + page).min(last),
        _ => 0,
    };
    let index = shown[to];
    let id = list.entries()[index].id;
    if extend {
        // Extend from the anchor to the new row.
        let anchor = selection
            .anchor
            .and_then(|id| list.index_of(id))
            .map(at)
            .unwrap_or(from.unwrap_or(to));
        selection.anchor = Some(list.entries()[shown[anchor]].id);
        selection.rows = shown[anchor.min(to)..=anchor.max(to)]
            .iter()
            .map(|&i| list.entries()[i].id)
            .collect();
        selection.cursor = Some(id);
        selection.reveal = Some(index);
    } else {
        selection.select(id, index);
    }
}
