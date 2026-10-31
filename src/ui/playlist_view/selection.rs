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
}

impl Selection {
    /// Selects one row and scrolls to it.
    pub(super) fn select(&mut self, id: EntryId, index: usize) {
        self.rows = BTreeSet::from([id]);
        self.anchor = Some(id);
        self.cursor = Some(id);
        self.reveal = Some(index);
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
        selection
            .rows
            .extend(list.entries()[range].iter().map(|entry| entry.id));
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
    let next = list.entries()[last + 1..]
        .iter()
        .chain(list.entries()[..last].iter().rev())
        .map(|entry| entry.id)
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

/// The editor's keys, unless a text field has the keyboard.
pub(super) fn keyboard(ui: &Ui, list: &Playlist, selection: &mut Selection, actions: &mut Actions) {
    if ui.ctx().text_edit_focused() || list.is_empty() {
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
            selection.rows = list.entries().iter().map(|entry| entry.id).collect();
        }
        Key::ArrowUp | Key::ArrowDown if modifiers.alt && !rows.is_empty() => {
            move_by(list, selection, rows, key == Key::ArrowUp, actions);
        }
        Key::ArrowUp | Key::ArrowDown | Key::Home | Key::End | Key::PageUp | Key::PageDown
            if !modifiers.alt && !modifiers.command =>
        {
            let from = selection
                .cursor
                .or(selection.anchor)
                .and_then(|id| list.index_of(id))
                .or(list.cursor().and_then(|id| list.index_of(id)));
            let last = list.len() - 1;
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
            let id = list.entries()[to].id;
            if modifiers.shift {
                // Extend from the anchor to the new row.
                let anchor = selection
                    .anchor
                    .and_then(|id| list.index_of(id))
                    .unwrap_or(from.unwrap_or(to));
                selection.anchor = Some(list.entries()[anchor].id);
                selection.rows = list.entries()[anchor.min(to)..=anchor.max(to)]
                    .iter()
                    .map(|entry| entry.id)
                    .collect();
                selection.cursor = Some(id);
                selection.reveal = Some(to);
            } else {
                selection.select(id, to);
            }
        }
        _ => {}
    }
}
