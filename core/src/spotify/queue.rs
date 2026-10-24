//! The account's queue, as Spotify has it, for the Spotify tab.
//!
//! Unlike a playlist, it is used up: songs leave it as they play. It is
//! shown the way Spotify's own apps show it: the song playing, the songs
//! queued, then the rest of the album or playlist it plays from.

use std::time::Instant;

use crate::model::{Entry, EntryId, Playlist, PlaylistId, TrackRef};
use crate::playback::engine::{PlayerSnapshot, Status, TrackInfo};

/// Paused this near a song's start, the queue shows as stopped: Spotify may
/// report a seek to the start a little past it.
const STOPPED_WITHIN_MS: u32 = 500;

/// The Spotify tab's rows are numbered from here down, apart from any
/// playlist's entries.
const FIRST_ROW: u64 = u64::MAX;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpotifyQueue {
    /// The device playing or paused, by name; None when none is.
    pub device: Option<String>,
    /// That device's ID.
    pub device_id: Option<String>,
    /// That device is Spotless.
    pub here: bool,
    pub status: Status,
    pub now: Option<TrackRef>,
    /// The position when it was last reported, at `reported_at`.
    pub position_ms: u32,
    pub reported_at: Option<Instant>,
    /// What plays next, in order.
    pub parts: Vec<Part>,
    /// What played before, the latest last.
    pub before: Vec<TrackRef>,
}

/// A run of the songs to come, by where they come from.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    /// "Next in queue", "Next from: …".
    pub title: String,
    /// Added by someone, rather than by what plays.
    pub queued: bool,
    pub songs: Vec<Queued>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Queued {
    pub track: TrackRef,
    /// Where Spotify's list of songs to come has it, to remove it by.
    pub index: usize,
}

/// A row of the Spotify tab.
#[derive(Clone, Debug, PartialEq)]
pub enum Row<'a> {
    Before(&'a TrackRef),
    Now(&'a TrackRef),
    Next(&'a Queued),
}

impl Row<'_> {
    pub fn track(&self) -> &TrackRef {
        match self {
            Row::Before(track) | Row::Now(track) => track,
            Row::Next(queued) => &queued.track,
        }
    }
}

impl SpotifyQueue {
    /// Spotless plays it, or has it paused: the queue can be changed.
    pub fn playing_here(&self) -> bool {
        self.here && self.status != Status::Stopped
    }

    /// A device plays it, or has it paused.
    pub fn active(&self) -> bool {
        self.device.is_some() && self.status != Status::Stopped
    }

    /// The status the player shows: paused at the song's start is stopped,
    /// as Spotify has no stop. The queue stays the player's all the same.
    pub fn shown_status(&self) -> Status {
        match self.status {
            Status::Paused if self.position_ms < STOPPED_WITHIN_MS => Status::Stopped,
            status => status,
        }
    }

    /// The tab's rows in order: before, now, then each part.
    pub fn rows(&self) -> impl Iterator<Item = Row<'_>> {
        self.before
            .iter()
            .map(Row::Before)
            .chain(self.now.iter().map(Row::Now))
            .chain(
                self.parts
                    .iter()
                    .flat_map(|part| part.songs.iter().map(Row::Next)),
            )
    }

    /// The ID the tab gives the row at `index`.
    pub fn row_id(index: usize) -> EntryId {
        EntryId(FIRST_ROW - index as u64)
    }

    /// The row with this ID.
    pub fn row(&self, id: EntryId) -> Option<Row<'_>> {
        let index = FIRST_ROW.checked_sub(id.0)?;
        self.rows().nth(usize::try_from(index).ok()?)
    }

    /// The songs to come, counted for the tab.
    pub fn len(&self) -> usize {
        self.now.iter().count() + self.parts.iter().map(|p| p.songs.len()).sum::<usize>()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The queue as a playlist, for what lists playlists' songs (the
    /// slim column, dragging rows): its cursor is the song playing.
    pub fn as_playlist(&self) -> Playlist {
        let mut list = Playlist::new(PlaylistId::SPOTIFY, "Spotify");
        let entries = self
            .rows()
            .enumerate()
            .map(|(index, row)| Entry {
                id: Self::row_id(index),
                track: row.track().clone(),
            })
            .collect();
        list.append(entries);
        list.set_cursor(Self::row_id(self.before.len()));
        list
    }

    /// The player's view of it, whichever device plays it.
    pub fn snapshot(&self) -> PlayerSnapshot {
        let track = self.now.as_ref().map(|track| {
            let artists: Vec<&str> = track.artists.iter().map(|a| a.name.as_str()).collect();
            TrackInfo {
                uri: track.uri.clone(),
                title: track.title.clone(),
                artists: artists.join(", "),
                main_artist: artists.first().copied().unwrap_or_default().into(),
                album: track.album.name.clone(),
                duration_ms: track.duration_ms,
                cover_url: None,
            }
        });
        // Spotify reports the position only when something changes, where
        // the player caps counting on at its next report's due: counted on
        // here, as of now.
        let mut position_ms = self.position_ms;
        if self.status == Status::Playing
            && let Some(at) = self.reported_at
        {
            position_ms = position_ms.saturating_add(at.elapsed().as_millis() as u32);
        }
        PlayerSnapshot {
            status: self.shown_status(),
            track,
            position_ms,
            reported_at: Some(Instant::now()),
            error: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AlbumRef;

    fn track(name: &str) -> TrackRef {
        TrackRef {
            uri: format!("spotify:track:{name}"),
            title: name.into(),
            artists: Vec::new(),
            album: AlbumRef {
                uri: String::new(),
                name: String::new(),
            },
            duration_ms: 1000,
        }
    }

    fn queue() -> SpotifyQueue {
        SpotifyQueue {
            now: Some(track("now")),
            before: vec![track("a"), track("b")],
            parts: vec![
                Part {
                    title: "Next in queue".into(),
                    queued: true,
                    songs: vec![Queued {
                        track: track("q"),
                        index: 0,
                    }],
                },
                Part {
                    title: "Next from: X".into(),
                    queued: false,
                    songs: vec![Queued {
                        track: track("c"),
                        index: 2,
                    }],
                },
            ],
            ..SpotifyQueue::default()
        }
    }

    #[test]
    fn rows_run_from_before_to_the_parts_and_are_found_by_id() {
        let queue = queue();
        let titles: Vec<_> = queue.rows().map(|r| r.track().title.clone()).collect();
        assert_eq!(titles, ["a", "b", "now", "q", "c"]);
        assert_eq!(
            queue.row(SpotifyQueue::row_id(2)),
            Some(Row::Now(&track("now")))
        );
        assert!(matches!(
            queue.row(SpotifyQueue::row_id(4)),
            Some(Row::Next(Queued { index: 2, .. }))
        ));
        assert_eq!(queue.row(EntryId(7)), None);
        assert_eq!(queue.len(), 3);
    }

    #[test]
    fn as_a_playlist_its_cursor_is_the_song_playing() {
        let list = queue().as_playlist();
        assert_eq!(list.len(), 5);
        assert_eq!(list.cursor(), Some(SpotifyQueue::row_id(2)));
    }

    #[test]
    fn the_players_view_counts_on_past_the_reports_due() {
        let mut queue = queue();
        queue.now.as_mut().unwrap().duration_ms = 600_000;
        queue.status = Status::Playing;
        queue.position_ms = 1_000;
        queue.reported_at = Some(Instant::now() - std::time::Duration::from_secs(10));
        let position = queue.snapshot().position_now();
        assert!((11_000..12_000).contains(&position), "{position}");
    }

    #[test]
    fn paused_at_the_start_it_shows_as_stopped_but_stays_the_players() {
        let mut queue = queue();
        queue.device = Some("Spotless".into());
        queue.status = Status::Paused;
        queue.position_ms = 120;
        assert_eq!(queue.snapshot().status, Status::Stopped);
        assert!(queue.active());
        queue.position_ms = 30_000;
        assert_eq!(queue.snapshot().status, Status::Paused);
    }
}
