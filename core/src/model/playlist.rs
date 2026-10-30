//! A playlist and its cursor.
//!
//! Songs are never consumed: the cursor moves over them. The cursor and the
//! play-next marks hold entry IDs, so they follow their songs through
//! inserts, moves and removals.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::sync::{ItemId, Link, Position};

/// Unique for the app's lifetime and persisted, so the same song added twice
/// is two entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntryId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlaylistId(pub u64);

impl PlaylistId {
    /// The playlist a new library starts with.
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

/// Where songs were added from: an album, a Spotify playlist, Liked
/// Songs or an artist, by URI, with its name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub uri: String,
    pub name: String,
}

impl Source {
    pub fn album(album: &AlbumRef) -> Self {
        Self {
            uri: album.uri.clone(),
            name: album.name.clone(),
        }
    }
}

/// Enough to draw a row without asking Spotify.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackRef {
    pub uri: String,
    pub title: String,
    pub artists: Vec<ArtistRef>,
    pub album: AlbumRef,
    pub duration_ms: u32,
    /// Where it was added from, which it keeps when copied or moved to
    /// another playlist. None for a song added on its own, as from a
    /// search, or added before Spotless kept it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
}

/// These songs, as added from `source`.
pub fn from_source(tracks: Vec<TrackRef>, source: &Source) -> Vec<TrackRef> {
    tracks
        .into_iter()
        .map(|track| TrackRef {
            source: Some(source.clone()),
            ..track
        })
        .collect()
}

/// What the list puts its songs under headers by. A setting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Grouping {
    /// Where they were added from; those added on their own, by none.
    #[default]
    Source,
    /// Their album.
    Album,
    /// Nothing: no headers.
    None,
}

impl Grouping {
    pub const ALL: [Self; 3] = [Self::Source, Self::Album, Self::None];

    pub fn name(self) -> &'static str {
        match self {
            Self::Source => "Where they came from",
            Self::Album => "Album",
            Self::None => "None",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Album => "album",
            Self::None => "none",
        }
    }

    pub fn from_key(key: Option<&str>) -> Self {
        match key {
            Some("album") => Self::Album,
            Some("none") => Self::None,
            _ => Self::Source,
        }
    }

    /// What songs share to sit under one header: None, nothing.
    fn key_of(self, track: &TrackRef) -> Option<&str> {
        let key = match self {
            Self::Source => track.source.as_ref().map(|source| source.uri.as_str()),
            Self::Album => Some(track.album.uri.as_str()),
            Self::None => None,
        };
        key.filter(|key| !key.is_empty())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: EntryId,
    pub track: TrackRef,
    /// In a synced playlist, the song's ID in its Spotify playlist: None
    /// until Spotify has it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<ItemId>,
}

/// A run of songs the list shows together. Two or more in a row from one
/// album, or one source (see [`Grouping`]), are a `header` run, under a
/// header with its name. The songs between those, each alone there, make
/// one plain run, set off from the header run before it by a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    pub start: usize,
    pub len: usize,
    pub header: bool,
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
    /// Synced: kept on Spotify too, in this playlist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync: Option<Link>,
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
            sync: None,
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

    /// The list cut into runs, in order, by `by`; see [`Run`].
    pub fn runs(&self, by: Grouping) -> Vec<Run> {
        let mut runs: Vec<Run> = Vec::new();
        let mut start = 0;
        while start < self.entries.len() {
            let len = match by.key_of(&self.entries[start].track) {
                None => 1,
                Some(key) => self.entries[start..]
                    .iter()
                    .take_while(|entry| by.key_of(&entry.track) == Some(key))
                    .count(),
            };
            match runs.last_mut() {
                Some(last) if len == 1 && !last.header => last.len += 1,
                _ => runs.push(Run {
                    start,
                    len,
                    header: len > 1,
                }),
            }
            start += len;
        }
        runs
    }

    /// A header run's name, and what it is, or whose, when that says
    /// more: an album's artist, "Spotify playlist", "Artist".
    pub fn heading(&self, run: Run, by: Grouping) -> (String, Option<String>) {
        let Some(track) = self.entries.get(run.start).map(|entry| &entry.track) else {
            return (String::new(), None);
        };
        let artist = || track.artists.first().map(|artist| artist.name.clone());
        let source = match (by, &track.source) {
            (Grouping::Source, Some(source)) => source,
            _ => return (track.album.name.clone(), artist()),
        };
        let uri = source.uri.as_str();
        let detail = if uri.starts_with("spotify:album:") {
            artist()
        } else if uri.starts_with("spotify:playlist:") {
            Some("Spotify playlist".into())
        } else if uri.starts_with("spotify:artist:") {
            Some("Artist".into())
        } else if uri.contains(":collection") {
            Some("Spotify".into())
        } else {
            None
        };
        (source.name.clone(), detail)
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

/// Synced playlists' part.
impl Playlist {
    /// Each song by entry, with its Spotify ID if it has one.
    pub fn sync_rows(&self) -> Vec<(EntryId, Option<ItemId>)> {
        self.entries
            .iter()
            .map(|entry| (entry.id, entry.item.clone()))
            .collect()
    }

    /// Gives songs their Spotify IDs.
    pub fn set_items(&mut self, items: &[(EntryId, ItemId)]) {
        for (id, item) in items {
            if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == *id) {
                entry.item = Some(item.clone());
            }
        }
    }

    /// Takes the Spotify IDs off, as the playlist stops being synced.
    pub fn clear_items(&mut self) {
        for entry in &mut self.entries {
            entry.item = None;
        }
    }

    /// Where it's up to, saved at `saved_at`; None while its song or a Q
    /// marked one isn't on Spotify yet.
    pub fn position(&self, saved_at: u64) -> Option<Position> {
        let item = |id: EntryId| self.get(id)?.item.clone();
        let song = match self.cursor {
            Some(cursor) => Some(item(cursor)?),
            None => None,
        };
        let marks = self
            .play_next
            .iter()
            .map(|&id| item(id))
            .collect::<Option<_>>()?;
        Some(Position {
            resume_s: if song.is_some() {
                self.resume_ms / 1000
            } else {
                0
            },
            song,
            saved_at,
            marks,
        })
    }

    /// Goes where `position` says, as far as it can: a song not here
    /// leaves the cursor where it is, and its Q mark out.
    pub fn set_position(&mut self, position: &Position) {
        let find = |item: &ItemId| {
            self.entries
                .iter()
                .find(|entry| entry.item.as_ref() == Some(item))
                .map(|entry| entry.id)
        };
        if let Some(cursor) = position.song.as_ref().and_then(find) {
            self.cursor = Some(cursor);
            self.detached = false;
            self.resume_ms = position.resume_s.saturating_mul(1000);
        }
        self.play_next = position.marks.iter().filter_map(find).collect();
    }

    /// Takes these songs, in this order, in place of the ones it has, as
    /// another device changed them. A song gone moves the cursor as
    /// [`Playlist::remove`] does.
    pub fn replace_entries(&mut self, entries: Vec<Entry>) {
        let kept: HashSet<EntryId> = entries.iter().map(|entry| entry.id).collect();
        let gone: Vec<EntryId> = self
            .entries
            .iter()
            .map(|entry| entry.id)
            .filter(|id| !kept.contains(id))
            .collect();
        self.remove(&gone);
        self.entries = entries;
        if self.cursor.is_none() && !self.detached {
            self.cursor = self.first();
        }
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
                source: None,
            },
            item: None,
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
    fn albums_of_two_or_more_get_a_run_and_lone_songs_share_one() {
        let mut list = Playlist::new(PlaylistId(1), "Test");
        let on = |id, album: &str| {
            let mut entry = entry(id);
            entry.track.album.uri = album.to_string();
            entry
        };
        list.append(vec![
            on(1, "a"),
            on(2, "b"),
            on(3, "b"),
            on(4, "b"),
            on(5, "c"),
            on(6, "d"),
            on(7, ""),
            on(8, ""),
            on(9, "e"),
            on(10, "e"),
            on(11, "f"),
        ]);
        let run = |start, len, header| Run { start, len, header };
        assert_eq!(
            list.runs(Grouping::Album),
            [
                run(0, 1, false),
                run(1, 3, true),
                run(4, 4, false),
                run(8, 2, true),
                run(10, 1, false),
            ]
        );
        assert_eq!(list.runs(Grouping::None), [run(0, 11, false)]);
        let empty = Playlist::new(PlaylistId(1), "Test");
        assert_eq!(empty.runs(Grouping::Album), []);
        assert_eq!(empty.runs(Grouping::None), []);
    }

    #[test]
    fn songs_from_one_source_get_a_run_whatever_their_albums() {
        let mut list = Playlist::new(PlaylistId(1), "Test");
        let late = Source {
            uri: "spotify:playlist:late".into(),
            name: "Late Trains".into(),
        };
        let on = |id, album: &str, source: Option<&Source>| {
            let mut entry = entry(id);
            entry.track.album.uri = album.to_string();
            entry.track.album.name = album.to_uppercase();
            entry.track.source = source.cloned();
            entry
        };
        let a = Source {
            uri: "a".into(),
            name: "A".into(),
        };
        list.append(vec![
            on(1, "x", Some(&late)),
            on(2, "y", Some(&late)),
            on(3, "z", Some(&late)),
            on(4, "a", Some(&a)),
            on(5, "a", Some(&a)),
            on(6, "a", None),
            on(7, "a", None),
        ]);
        let run = |start, len, header| Run { start, len, header };
        let runs = list.runs(Grouping::Source);
        assert_eq!(runs, [run(0, 3, true), run(3, 2, true), run(5, 2, false)]);
        assert_eq!(
            list.heading(runs[0], Grouping::Source),
            ("Late Trains".into(), Some("Spotify playlist".into()))
        );
        // By album, the songs added on their own join their album's.
        assert_eq!(
            list.runs(Grouping::Album),
            [run(0, 3, false), run(3, 4, true)]
        );
        assert_eq!(list.heading(run(3, 4, true), Grouping::Album).0, "A");
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
