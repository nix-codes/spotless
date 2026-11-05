//! Sorting and shuffling a playlist, and the headers its songs sit under.
//!
//! A sort or a shuffle rewrites the order once, like moving songs by hand.
//! It takes the songs' sources away, so only songs added since have one, and
//! says what the songs without one are grouped by ([`SortHeaders`]). Headers
//! are worked out from that and the list as it is now, so nothing has to
//! follow a song moved by hand.

use std::collections::HashMap;

use fastrand::Rng;
use serde::{Deserialize, Serialize};

use super::{Entry, EntryId, Playlist, Run, Source, TrackRef};

/// How to reorder a playlist.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    /// By artist, each artist's albums by date, each album's songs in order.
    ArtistAlbums,
    /// By artist, each artist's songs shuffled.
    ArtistShuffled,
    Shuffled,
    Reversed,
}

impl Order {
    /// What it did, for the status line.
    pub fn done(self) -> &'static str {
        match self {
            Self::ArtistAlbums => "Sorted by artist, albums by date",
            Self::ArtistShuffled => "Sorted by artist, songs shuffled",
            Self::Shuffled => "Shuffled all songs",
            Self::Reversed => "Reversed",
        }
    }
}

/// What the last sort left the songs without a source grouped by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortHeaders {
    Album,
    Artist,
}

/// The songs of a run of this many albums in a row get album headers.
const ALBUM_RUN: usize = 3;

/// What songs share to sit under one header.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Group<'a> {
    Source(&'a str),
    Album(&'a str),
    Artist(&'a str),
}

impl Group<'_> {
    /// The fewest songs in a row that get a header.
    fn fewest(self) -> usize {
        match self {
            Self::Album(_) => ALBUM_RUN,
            Self::Source(_) | Self::Artist(_) => 2,
        }
    }
}

/// Neighbouring songs of one group, or one song of none.
struct GroupRun<'a> {
    start: usize,
    len: usize,
    group: Option<Group<'a>>,
    split: bool,
}

impl GroupRun<'_> {
    fn long_enough(&self) -> bool {
        self.group.is_some_and(|group| self.len >= group.fewest())
    }
}

/// Whether the run at `index` splits the group on both sides of it: it's too
/// short for a header of its own, or both sides have one, which would repeat.
fn splits(runs: &[GroupRun], index: usize) -> bool {
    let (Some(before), Some(run), Some(after)) = (
        index.checked_sub(1).and_then(|i| runs.get(i)),
        runs.get(index),
        runs.get(index + 1),
    ) else {
        return false;
    };
    before.group.is_some()
        && before.group == after.group
        && (!run.long_enough() || (before.long_enough() && after.long_enough()))
}

/// Marks the runs of a group split by a song landing inside it. A song
/// stepping out of a group into the next makes two splits in a row
/// (A A A B a B B): only the later counts, the group it landed in.
fn mark_splits(runs: &mut [GroupRun]) {
    for index in 1..runs.len().saturating_sub(1) {
        if splits(runs, index) && !splits(runs, index + 1) {
            runs[index - 1].split = true;
            runs[index + 1].split = true;
        }
    }
}

/// A playlist's order and sources from before a sort or shuffle.
#[derive(Clone, Debug)]
pub(crate) struct Snapshot {
    rows: Vec<(EntryId, Option<Source>)>,
    sorted: Option<SortHeaders>,
}

impl Playlist {
    fn group_of<'a>(&self, track: &'a TrackRef) -> Option<Group<'a>> {
        let filled = |key: &'a str| (!key.is_empty()).then_some(key);
        if let Some(source) = track.source.as_ref() {
            return filled(&source.uri).map(Group::Source);
        }
        match self.sorted? {
            SortHeaders::Album => filled(&track.album.uri).map(Group::Album),
            SortHeaders::Artist => filled(&track.artists.first()?.uri).map(Group::Artist),
        }
    }

    /// The list cut into runs, in order, under the headers it has now; see
    /// [`Run`].
    pub fn song_runs(&self) -> Vec<Run> {
        if self.headers_off {
            return match self.entries.len() {
                0 => Vec::new(),
                len => vec![Run {
                    start: 0,
                    len,
                    header: false,
                }],
            };
        }
        let mut groups: Vec<GroupRun> = Vec::new();
        let mut start = 0;
        while start < self.entries.len() {
            let group = self.group_of(&self.entries[start].track);
            let len = match group {
                None => 1,
                Some(_) => self.entries[start..]
                    .iter()
                    .take_while(|entry| self.group_of(&entry.track) == group)
                    .count(),
            };
            groups.push(GroupRun {
                start,
                len,
                group,
                split: false,
            });
            start += len;
        }
        mark_splits(&mut groups);
        let mut runs: Vec<Run> = Vec::new();
        for group in &groups {
            let header = group.long_enough() && !group.split;
            match runs.last_mut() {
                Some(last) if !header && !last.header => last.len += group.len,
                _ => runs.push(Run {
                    start: group.start,
                    len: group.len,
                    header,
                }),
            }
        }
        runs
    }

    /// A header's name, and what it is or whose, when that says more: a
    /// source's "Spotify playlist" or artist, an album's artist and year.
    pub fn song_heading(&self, run: Run) -> (String, Option<String>) {
        let Some(track) = self.entries.get(run.start).map(|entry| &entry.track) else {
            return (String::new(), None);
        };
        let artist = || track.artists.first().map(|artist| artist.name.clone());
        match self.group_of(track) {
            Some(Group::Source(_)) => track
                .source
                .as_ref()
                .map_or((String::new(), None), |source| {
                    (source.name.clone(), source_detail(track, source))
                }),
            Some(Group::Artist(_)) => (artist().unwrap_or_default(), None),
            _ => {
                let detail = match (artist(), year(track)) {
                    (Some(artist), Some(year)) => Some(format!("{artist} · {year}")),
                    (artist, year) => artist.or(year.map(String::from)),
                };
                (track.album.name.clone(), detail)
            }
        }
    }

    /// The album to show beside a song, when no header says it. `under`: the
    /// song is under a header.
    pub fn album_beside(&self, index: usize, under: bool) -> Option<String> {
        let track = &self.entries.get(index)?.track;
        let shown = !self.headers_off
            && track.source.is_none()
            && match self.sorted? {
                SortHeaders::Artist => true,
                SortHeaders::Album => !under,
            };
        shown.then(|| match year(track) {
            Some(year) => format!("{} · {year}", track.album.name),
            None => track.album.name.clone(),
        })
    }

    pub fn headers_on(&self) -> bool {
        !self.headers_off
    }

    pub(crate) fn set_headers_on(&mut self, on: bool) {
        self.headers_off = !on;
    }

    /// Rewrites the order, once. The song playing and the Q marks go with
    /// their songs.
    pub(crate) fn reorder(&mut self, order: Order, rng: &mut Rng) {
        match order {
            Order::ArtistAlbums => {
                self.entries.sort_by_cached_key(album_key);
                self.sorted = Some(self.headers_for_albums());
                self.forget_sources();
            }
            Order::ArtistShuffled => {
                rng.shuffle(&mut self.entries);
                self.entries.sort_by_cached_key(artist_key);
                self.sorted = Some(SortHeaders::Artist);
                self.forget_sources();
            }
            Order::Shuffled => {
                rng.shuffle(&mut self.entries);
                self.sorted = None;
                self.forget_sources();
            }
            Order::Reversed => self.entries.reverse(),
        }
    }

    /// Album headers if at least half the songs are in runs of an album
    /// long enough for one, so a playlist of whole albums has those and a
    /// best-of has artists'.
    fn headers_for_albums(&self) -> SortHeaders {
        let mut in_runs = 0;
        let mut start = 0;
        while start < self.entries.len() {
            let album = &self.entries[start].track.album.uri;
            let len = self.entries[start..]
                .iter()
                .take_while(|entry| &entry.track.album.uri == album)
                .count();
            if len >= ALBUM_RUN && !album.is_empty() {
                in_runs += len;
            }
            start += len;
        }
        if in_runs * 2 >= self.entries.len() {
            SortHeaders::Album
        } else {
            SortHeaders::Artist
        }
    }

    fn forget_sources(&mut self) {
        for entry in &mut self.entries {
            entry.track.source = None;
        }
    }

    pub(crate) fn snapshot(&self) -> Snapshot {
        Snapshot {
            rows: (self.entries.iter())
                .map(|entry| (entry.id, entry.track.source.clone()))
                .collect(),
            sorted: self.sorted,
        }
    }

    /// Puts back a snapshot of these same songs. Each keeps the rest of
    /// what it has now, such as the Spotify ID it got since.
    pub(crate) fn restore(&mut self, snapshot: Snapshot) {
        let mut entries: HashMap<EntryId, Entry> = std::mem::take(&mut self.entries)
            .into_iter()
            .map(|entry| (entry.id, entry))
            .collect();
        for (id, source) in snapshot.rows {
            if let Some(mut entry) = entries.remove(&id) {
                entry.track.source = source;
                self.entries.push(entry);
            }
        }
        self.entries.extend(entries.into_values());
        self.sorted = snapshot.sorted;
    }

    /// Its songs, in order.
    pub(crate) fn order(&self) -> Vec<EntryId> {
        self.entries.iter().map(|entry| entry.id).collect()
    }
}

/// A song's year, from its album's date.
fn year(track: &TrackRef) -> Option<&str> {
    track.in_album.as_ref()?.date.as_deref()?.get(..4)
}

/// What a source's header says beside its name.
pub(super) fn source_detail(track: &TrackRef, source: &Source) -> Option<String> {
    let uri = source.uri.as_str();
    if uri.starts_with("spotify:album:") {
        track.artists.first().map(|artist| artist.name.clone())
    } else if uri.starts_with("spotify:playlist:") {
        Some("Spotify playlist".into())
    } else if uri.starts_with("spotify:artist:") {
        Some("Artist".into())
    } else if uri.contains(":collection") {
        Some("Spotify".into())
    } else {
        None
    }
}

type ArtistKey = (String, String);

/// The first artist's name, ignoring case; its URI keeps two artists of one
/// name apart.
fn artist_key(entry: &Entry) -> ArtistKey {
    entry
        .track
        .artists
        .first()
        .map(|artist| (artist.name.to_lowercase(), artist.uri.clone()))
        .unwrap_or_default()
}

/// Artist, album date (without one, after those with), album name, album,
/// disc, song. Songs without numbers keep their order: the sort is stable.
#[allow(clippy::type_complexity)]
fn album_key(entry: &Entry) -> (ArtistKey, bool, String, String, String, u32, u32) {
    let track = &entry.track;
    let place = track.in_album.as_ref();
    let date = place.and_then(|place| place.date.clone());
    (
        artist_key(entry),
        date.is_none(),
        date.unwrap_or_default(),
        track.album.name.to_lowercase(),
        track.album.uri.clone(),
        place.map_or(0, |place| place.disc),
        place.map_or(0, |place| place.number),
    )
}

#[cfg(test)]
mod tests {
    use super::super::{AlbumPlace, AlbumRef, ArtistRef};
    use super::*;
    use crate::model::PlaylistId;

    /// A song, as (artist, album, date, disc, number).
    fn song(id: u64, artist: &str, album: &str, date: Option<&str>, disc: u32, n: u32) -> Entry {
        Entry {
            id: EntryId(id),
            track: TrackRef {
                uri: format!("spotify:track:{id}"),
                title: format!("song {id}"),
                artists: vec![ArtistRef {
                    uri: format!("spotify:artist:{}", artist.to_lowercase()),
                    name: artist.into(),
                    image: None,
                }],
                album: AlbumRef {
                    uri: format!("spotify:album:{album}"),
                    name: album.into(),
                },
                duration_ms: 1000,
                source: None,
                in_album: date.map(|date| AlbumPlace {
                    date: Some(date.into()),
                    disc,
                    number: n,
                }),
            },
            item: None,
        }
    }

    fn album(first: u64, name: &str, artist: &str, date: &str, songs: u32) -> Vec<Entry> {
        (0..songs)
            .map(|n| song(first + u64::from(n), artist, name, Some(date), 1, n + 1))
            .collect()
    }

    fn from(mut entries: Vec<Entry>, source: &str) -> Vec<Entry> {
        for entry in &mut entries {
            entry.track.source = Some(Source {
                uri: format!("spotify:album:{source}"),
                name: source.into(),
            });
        }
        entries
    }

    fn list(entries: Vec<Entry>) -> Playlist {
        let mut list = Playlist::new(PlaylistId(1), "Test");
        list.append(entries);
        list
    }

    fn ids(list: &Playlist) -> Vec<u64> {
        list.entries().iter().map(|entry| entry.id.0).collect()
    }

    /// The names of the headers shown, in order.
    fn headers(list: &Playlist) -> Vec<String> {
        (list.song_runs().into_iter())
            .filter(|run| run.header)
            .map(|run| list.song_heading(run).0)
            .collect()
    }

    fn step_down(list: &mut Playlist, id: u64) {
        let at = list.index_of(EntryId(id)).unwrap();
        list.move_to(&[EntryId(id)], at + 2);
    }

    #[test]
    fn a_sort_goes_by_artist_then_album_date_then_album_name_then_disc_and_song() {
        let mut list = list(vec![
            song(1, "Zed", "Late", Some("2020-01-01"), 1, 1),
            song(2, "abba", "Later", Some("2021-01-01"), 1, 1),
            song(3, "Abba", "Early", Some("2019-01-01"), 2, 1),
            song(4, "Abba", "Early", Some("2019-01-01"), 1, 2),
            song(5, "Abba", "Same Day B", Some("2020-05-05"), 1, 1),
            song(6, "Abba", "Same Day A", Some("2020-05-05"), 1, 2),
            song(7, "Abba", "Same Day A", Some("2020-05-05"), 1, 1),
        ]);
        list.reorder(Order::ArtistAlbums, &mut Rng::with_seed(1));
        assert_eq!(ids(&list), [4, 3, 7, 6, 5, 2, 1]);
    }

    #[test]
    fn songs_without_dates_or_numbers_pile_up_after_the_dated_keeping_their_order() {
        let mut list = list(vec![
            song(1, "Abba", "Old B", None, 0, 0),
            song(2, "Abba", "Old A", None, 0, 0),
            song(3, "Abba", "Old B", None, 0, 0),
            song(4, "Abba", "New", Some("2020-01-01"), 1, 1),
            song(5, "Abba", "Old A", None, 0, 0),
        ]);
        list.reorder(Order::ArtistAlbums, &mut Rng::with_seed(1));
        assert_eq!(ids(&list), [4, 2, 5, 1, 3]);
    }

    #[test]
    fn whole_albums_get_album_headers_with_their_year_and_a_best_of_artist_headers() {
        let mut albums = list(album(1, "One", "Abba", "2001-02-03", 3));
        albums.append(album(10, "Two", "Abba", "2002-02-03", 3));
        albums.reorder(Order::ArtistAlbums, &mut Rng::with_seed(1));
        assert_eq!(headers(&albums), ["One", "Two"]);
        let run = albums.song_runs()[0];
        assert_eq!(
            albums.song_heading(run).1.as_deref(),
            Some("Abba · 2001"),
            "an album's header says its artist and year"
        );
        assert_eq!(albums.album_beside(0, true), None);

        let mut best = list(vec![
            song(1, "Abba", "One", Some("2001-01-01"), 1, 1),
            song(2, "Abba", "Two", Some("2002-01-01"), 1, 1),
            song(3, "Queen", "Three", Some("2003-01-01"), 1, 1),
        ]);
        best.reorder(Order::ArtistAlbums, &mut Rng::with_seed(1));
        assert_eq!(headers(&best), ["Abba"]);
        assert_eq!(best.album_beside(0, true).as_deref(), Some("One · 2001"));
    }

    #[test]
    fn a_short_run_in_album_headers_shows_its_album_beside_the_song() {
        let mut entries = album(1, "One", "Abba", "2001-01-01", 4);
        entries.push(song(9, "Queen", "Two", Some("2002-01-01"), 1, 1));
        let mut list = list(entries);
        list.reorder(Order::ArtistAlbums, &mut Rng::with_seed(1));
        assert_eq!(headers(&list), ["One"]);
        assert_eq!(list.album_beside(4, false).as_deref(), Some("Two · 2002"));
    }

    #[test]
    fn a_saved_before_album_has_no_year_in_its_header() {
        let mut list = list(
            (1..=3)
                .map(|id| song(id, "Abba", "Old", None, 0, 0))
                .collect(),
        );
        list.reorder(Order::ArtistAlbums, &mut Rng::with_seed(1));
        let run = list.song_runs()[0];
        assert_eq!(list.song_heading(run).1.as_deref(), Some("Abba"));
    }

    #[test]
    fn artist_shuffled_gathers_artists_under_their_headers() {
        let mut list = list(vec![
            song(1, "Queen", "A", Some("2001-01-01"), 1, 1),
            song(2, "Abba", "B", Some("2001-01-01"), 1, 1),
            song(3, "Queen", "C", Some("2001-01-01"), 1, 1),
            song(4, "Abba", "D", Some("2001-01-01"), 1, 1),
        ]);
        list.reorder(Order::ArtistShuffled, &mut Rng::with_seed(7));
        assert_eq!(headers(&list), ["Abba", "Queen"]);
    }

    #[test]
    fn a_shuffle_leaves_no_headers_until_an_album_is_added_with_its_source() {
        let mut list = list(from(album(1, "One", "Abba", "2001-01-01", 4), "One"));
        assert_eq!(headers(&list), ["One"]);
        list.reorder(Order::Shuffled, &mut Rng::with_seed(3));
        assert!(headers(&list).is_empty());
        list.append(from(album(10, "Two", "Queen", "2002-01-01", 2), "Two"));
        assert_eq!(headers(&list), ["Two"], "only what was added since");
    }

    #[test]
    fn songs_added_after_a_sort_show_their_source_beside_the_sorts_headers() {
        let mut list = list(album(1, "One", "Abba", "2001-01-01", 3));
        list.reorder(Order::ArtistAlbums, &mut Rng::with_seed(1));
        list.append(from(album(10, "Two", "Queen", "2002-01-01", 2), "Two"));
        assert_eq!(headers(&list), ["One", "Two"]);
        list.reorder(Order::ArtistAlbums, &mut Rng::with_seed(1));
        assert_eq!(headers(&list), ["One"], "the next sort takes them in");
    }

    #[test]
    fn reversing_keeps_the_headers_and_the_sources() {
        let mut list = list(from(album(1, "One", "Abba", "2001-01-01", 2), "One"));
        list.reorder(Order::Reversed, &mut Rng::with_seed(1));
        assert_eq!(ids(&list), [2, 1]);
        assert_eq!(headers(&list), ["One"]);
    }

    #[test]
    fn a_song_stepping_through_an_album_hides_only_that_albums_header_meanwhile() {
        let mut entries = album(1, "A", "Abba", "2001-01-01", 4);
        entries.extend(album(11, "B", "Beck", "2002-01-01", 4));
        entries.extend(album(21, "C", "Cher", "2003-01-01", 4));
        let mut list = list(entries);
        list.reorder(Order::ArtistAlbums, &mut Rng::with_seed(1));
        assert_eq!(headers(&list), ["A", "B", "C"]);
        for _ in 0..3 {
            step_down(&mut list, 4);
            assert_eq!(headers(&list), ["A", "C"], "{:?}", ids(&list));
        }
        step_down(&mut list, 4);
        assert_eq!(headers(&list), ["A", "B", "C"], "out of it, all are back");
        step_down(&mut list, 4);
        assert_eq!(headers(&list), ["A", "B"], "into the next");
    }

    #[test]
    fn moving_a_song_to_the_gap_between_groups_breaks_nothing() {
        let mut entries = from(album(1, "A", "Abba", "2001-01-01", 3), "A");
        entries.extend(from(album(11, "B", "Beck", "2002-01-01", 3), "B"));
        entries.extend(from(album(21, "C", "Cher", "2003-01-01", 3), "C"));
        let mut list = list(entries);
        list.move_to(&[EntryId(1)], 6);
        assert_eq!(headers(&list), ["A", "B", "C"]);
    }

    #[test]
    fn a_group_dropped_inside_another_splits_it_rather_than_repeat_its_header() {
        let mut entries = from(album(1, "A", "Abba", "2001-01-01", 2), "A");
        entries.extend(from(album(11, "B", "Beck", "2002-01-01", 4), "B"));
        let mut list = list(entries);
        list.move_to(&[EntryId(1), EntryId(2)], 4);
        assert_eq!(ids(&list), [11, 12, 1, 2, 13, 14]);
        assert_eq!(headers(&list), ["A"]);
    }

    #[test]
    fn a_song_inside_a_group_doesnt_touch_the_other_groups() {
        let mut entries = from(album(1, "A", "Abba", "2001-01-01", 3), "A");
        entries.extend(from(album(11, "B", "Beck", "2002-01-01", 4), "B"));
        entries.extend(from(album(21, "C", "Cher", "2003-01-01", 3), "C"));
        let mut list = list(entries);
        list.move_to(&[EntryId(21)], 5);
        assert_eq!(headers(&list), ["A", "C"]);
    }

    #[test]
    fn headers_off_are_for_this_playlist_and_show_nothing() {
        let mut list = list(from(album(1, "One", "Abba", "2001-01-01", 3), "One"));
        list.set_headers_on(false);
        assert_eq!(
            list.song_runs(),
            [Run {
                start: 0,
                len: 3,
                header: false
            }]
        );
        assert_eq!(list.album_beside(0, false), None);
        list.set_headers_on(true);
        assert_eq!(headers(&list), ["One"]);
        assert!(Playlist::new(PlaylistId(2), "Empty").song_runs().is_empty());
    }

    #[test]
    fn a_snapshot_puts_back_the_order_and_sources_keeping_what_songs_got_since() {
        let mut list = list(from(album(1, "One", "Abba", "2001-01-01", 3), "One"));
        let before = list.snapshot();
        list.reorder(Order::Reversed, &mut Rng::with_seed(1));
        list.reorder(Order::Shuffled, &mut Rng::with_seed(1));
        assert!(headers(&list).is_empty());
        list.restore(before);
        assert_eq!(ids(&list), [1, 2, 3]);
        assert_eq!(headers(&list), ["One"]);
    }

    #[test]
    fn the_song_playing_and_the_q_marks_stay_on_their_songs() {
        let mut list = list(album(1, "One", "Abba", "2001-01-01", 4));
        list.set_cursor(EntryId(2));
        list.toggle_mark(EntryId(3));
        list.reorder(Order::Shuffled, &mut Rng::with_seed(5));
        assert_eq!(list.cursor(), Some(EntryId(2)));
        assert_eq!(list.mark_number(EntryId(3)), Some(1));
    }
}
