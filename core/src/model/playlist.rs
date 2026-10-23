//! A playlist and its cursor.
//!
//! Songs are never consumed: the cursor moves over them. The cursor and the
//! play-next marks hold entry IDs, so they follow their songs through
//! inserts, moves and removals.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

/// Unique for the app's lifetime and persisted, so the same song added twice
/// is two entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntryId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlaylistId(pub u64);

impl PlaylistId {
    /// The playlist that always exists.
    pub const DEFAULT: Self = Self(0);
    /// The Spotify tab's, which shows the account's queue: no playlist of
    /// the library has it.
    pub const SPOTIFY: Self = Self(u64::MAX);
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtistRef {
    pub uri: String,
    pub name: String,
    /// The artist's picture's URL, about 300 px, where the catalog gave one
    /// (never for a song's artists).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlbumRef {
    pub uri: String,
    pub name: String,
}

/// Enough to draw a row without asking Spotify.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackRef {
    pub uri: String,
    pub title: String,
    pub artists: Vec<ArtistRef>,
    pub album: AlbumRef,
    pub duration_ms: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: EntryId,
    pub track: TrackRef,
}

/// Invariant: unless `detached`, the cursor is set exactly when the
/// playlist has entries.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Playlist {
    pub id: PlaylistId,
    pub name: String,
    entries: Vec<Entry>,
    cursor: Option<EntryId>,
    /// The cursor's song was removed. The cursor already stands on the song
    /// that followed it (`None` if it was the last), so the next step plays
    /// the cursor instead of moving past it. Not saved: after a restart the
    /// cursor is simply on that song.
    #[serde(skip)]
    detached: bool,
    /// Where to resume the cursor's song, in ms.
    pub resume_ms: u32,
    /// The Q marks, in the order they play.
    #[serde(default)]
    play_next: Vec<EntryId>,
}

impl Playlist {
    pub fn new(id: PlaylistId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            entries: Vec::new(),
            cursor: None,
            detached: false,
            resume_ms: 0,
            play_next: Vec::new(),
        }
    }

    /// Restores the invariants of a playlist read from disk: unique
    /// entries, a cursor on one of them, marks only on its entries.
    pub(super) fn repair(&mut self) {
        let mut seen = HashSet::new();
        self.entries.retain(|entry| seen.insert(entry.id));
        self.detached = false;
        if self.cursor.is_none_or(|cursor| !seen.contains(&cursor)) {
            self.reset_cursor();
        }
        let mut marked = HashSet::new();
        self.play_next
            .retain(|id| seen.contains(id) && marked.insert(*id));
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn index_of(&self, id: EntryId) -> Option<usize> {
        self.entries.iter().position(|entry| entry.id == id)
    }

    pub fn get(&self, id: EntryId) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    pub fn first(&self) -> Option<EntryId> {
        self.entries.first().map(|entry| entry.id)
    }

    pub fn cursor(&self) -> Option<EntryId> {
        self.cursor
    }

    pub fn detached(&self) -> bool {
        self.detached
    }

    /// Puts the cursor on `id` at the start of the song.
    pub fn set_cursor(&mut self, id: EntryId) -> bool {
        if self.index_of(id).is_none() {
            return false;
        }
        self.cursor = Some(id);
        self.detached = false;
        self.resume_ms = 0;
        true
    }

    /// Back to the first song, as at the end of the list.
    pub fn reset_cursor(&mut self) {
        self.cursor = self.first();
        self.detached = false;
        self.resume_ms = 0;
    }

    /// Moves a detached cursor without attaching it: shuffle's successor
    /// replaces the one that follows in the list.
    pub(super) fn detach_to(&mut self, id: Option<EntryId>) {
        debug_assert!(self.detached);
        self.cursor = id;
    }

    pub fn play_next(&self) -> &[EntryId] {
        &self.play_next
    }

    /// The 1-based number of `id`'s Q mark, if it has one.
    pub fn mark_number(&self, id: EntryId) -> Option<usize> {
        self.play_next.iter().position(|&m| m == id).map(|i| i + 1)
    }

    /// Adds a Q mark at the back, or removes the one `id` has.
    pub fn toggle_mark(&mut self, id: EntryId) {
        if let Some(i) = self.play_next.iter().position(|&m| m == id) {
            self.play_next.remove(i);
        } else if self.index_of(id).is_some() {
            self.play_next.push(id);
        }
    }

    pub fn unmark(&mut self, id: EntryId) {
        self.play_next.retain(|&m| m != id);
    }

    pub fn clear_marks(&mut self) {
        self.play_next.clear();
    }

    pub fn append(&mut self, entries: Vec<Entry>) {
        self.insert(self.entries.len(), entries);
    }

    /// Inserts `entries` before the entry at `index` (clamped to the end).
    pub fn insert(&mut self, index: usize, entries: Vec<Entry>) {
        let Some(first) = entries.first().map(|entry| entry.id) else {
            return;
        };
        let index = index.min(self.entries.len());
        let at_end = index == self.entries.len();
        self.entries.splice(index..index, entries);
        // An empty list gets a cursor. A removed last song is followed by
        // what is added after it.
        if self.cursor.is_none() && (!self.detached || at_end) {
            self.cursor = Some(first);
        }
    }

    /// Removes the entries with these IDs. A removed cursor moves to the
    /// next surviving entry and detaches.
    pub fn remove(&mut self, ids: &[EntryId]) {
        let ids: HashSet<EntryId> = ids.iter().copied().collect();
        if let Some(cursor) = self.cursor.filter(|cursor| ids.contains(cursor)) {
            let after = self.index_of(cursor).map_or(0, |i| i + 1);
            self.cursor = self.entries[after..]
                .iter()
                .map(|entry| entry.id)
                .find(|id| !ids.contains(id));
            self.detached = true;
            self.resume_ms = 0;
        }
        self.entries.retain(|entry| !ids.contains(&entry.id));
        self.play_next.retain(|id| !ids.contains(id));
        if self.entries.is_empty() {
            self.cursor = None;
            self.detached = false;
        }
    }

    /// Moves the entries with these IDs, keeping their order, to just before
    /// the entry now at `index` (clamped to the end).
    pub fn move_to(&mut self, ids: &[EntryId], index: usize) {
        let ids: HashSet<EntryId> = ids.iter().copied().collect();
        let anchor = self
            .entries
            .iter()
            .skip(index)
            .map(|entry| entry.id)
            .find(|id| !ids.contains(id));
        let (moved, kept): (Vec<Entry>, Vec<Entry>) = std::mem::take(&mut self.entries)
            .into_iter()
            .partition(|entry| ids.contains(&entry.id));
        self.entries = kept;
        let at = anchor
            .and_then(|anchor| self.index_of(anchor))
            .unwrap_or(self.entries.len());
        self.entries.splice(at..at, moved);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn entry(id: u64) -> Entry {
        Entry {
            id: EntryId(id),
            track: TrackRef {
                uri: format!("spotify:track:{id}"),
                title: format!("Song {id}"),
                artists: vec![],
                album: AlbumRef {
                    uri: String::new(),
                    name: String::new(),
                },
                duration_ms: 180_000,
            },
        }
    }

    /// A playlist with entries 1..=n.
    pub fn list(n: u64) -> Playlist {
        let mut list = Playlist::new(PlaylistId(1), "Test");
        list.append((1..=n).map(entry).collect());
        list
    }

    pub fn ids(list: &Playlist) -> Vec<u64> {
        list.entries().iter().map(|entry| entry.id.0).collect()
    }

    #[test]
    fn an_empty_list_gets_a_cursor_on_its_first_song() {
        let mut list = Playlist::new(PlaylistId(1), "Test");
        assert_eq!(list.cursor(), None);
        list.append(vec![entry(1), entry(2)]);
        assert_eq!(list.cursor(), Some(EntryId(1)));
        list.insert(0, vec![entry(3)]);
        assert_eq!(list.cursor(), Some(EntryId(1)), "an insert keeps it");
    }

    #[test]
    fn a_removed_cursor_moves_to_the_next_survivor() {
        let mut list = list(5);
        list.set_cursor(EntryId(2));
        list.remove(&[EntryId(2), EntryId(3)]);
        assert_eq!(list.cursor(), Some(EntryId(4)));
        assert!(list.detached());
        assert_eq!(ids(&list), [1, 4, 5]);
    }

    #[test]
    fn a_removed_last_cursor_is_followed_by_what_is_appended() {
        let mut list = list(3);
        list.set_cursor(EntryId(3));
        list.remove(&[EntryId(3)]);
        assert_eq!(list.cursor(), None);
        list.insert(0, vec![entry(7)]);
        assert_eq!(list.cursor(), None, "not added after it");
        list.append(vec![entry(8)]);
        assert_eq!(list.cursor(), Some(EntryId(8)));
    }

    #[test]
    fn removing_everything_clears_the_cursor() {
        let mut list = list(2);
        list.remove(&[EntryId(1), EntryId(2)]);
        assert_eq!(list.cursor(), None);
        assert!(!list.detached());
    }

    #[test]
    fn marks_are_numbered_toggled_and_pruned() {
        let mut list = list(4);
        list.toggle_mark(EntryId(3));
        list.toggle_mark(EntryId(1));
        list.toggle_mark(EntryId(9));
        assert_eq!(list.mark_number(EntryId(1)), Some(2));
        list.toggle_mark(EntryId(3));
        assert_eq!(list.mark_number(EntryId(1)), Some(1));
        list.remove(&[EntryId(1)]);
        assert!(list.play_next().is_empty());
    }

    #[test]
    fn moves_keep_order_and_follow_the_anchor() {
        let mut list = list(6);
        list.set_cursor(EntryId(2));
        list.move_to(&[EntryId(5), EntryId(2)], 1);
        assert_eq!(ids(&list), [1, 2, 5, 3, 4, 6]);
        list.move_to(&[EntryId(1)], 99);
        assert_eq!(ids(&list), [2, 5, 3, 4, 6, 1]);
        // Down by one: before the entry two below.
        list.move_to(&[EntryId(3)], 4);
        assert_eq!(ids(&list), [2, 5, 4, 3, 6, 1]);
        assert_eq!(list.cursor(), Some(EntryId(2)));
    }
}
