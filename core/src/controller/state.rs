//! What the interface shows, read off the controller: the song the
//! player shows, the tabs, and what's left to play.

use std::collections::HashMap;
use std::time::Instant;

use super::SyncStatus;
use crate::backend::{SearchState, SessionState, Storage};
use crate::browse::{Browse, Loadable};
use crate::lastfm::{LastFmState, Loved};
use crate::model::{Library, Playlist, PlaylistId, TrackRef};
use crate::playback::engine::{PlayerSnapshot, Status, TrackInfo};
use crate::playback::outputs::Output;
use crate::playback::transport::Transport;
use crate::spotify::queue::SpotifyQueue;
use crate::store::Settings;

/// What the interface shows. It reads it; only the controller changes it.
pub struct State {
    pub session: SessionState,
    pub player: PlayerSnapshot,
    /// The song waits, paused where it stopped, for the connection to
    /// Spotify: `player` shows it so, while the engine holds nothing.
    pub offline: bool,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub library: Library,
    pub(crate) transport: Transport,
    /// The playlist shown in the editor.
    pub viewed: PlaylistId,
    pub settings: Settings,
    pub browse: Browse,
    pub search: SearchState,
    pub lastfm: LastFmState,
    /// The songs loved on Last.fm, for their hearts.
    pub loved: Loved,
    /// What the caches hold, once measured.
    pub storage: Option<Storage>,
    /// The audio outputs, once listed; None when they can't be.
    pub outputs: Loadable<Option<Vec<Output>>>,
    /// The account's queue, for the Spotify tab; None while Spotless isn't
    /// in Spotify Connect.
    pub spotify: Option<SpotifyQueue>,
    /// The queue as a playlist, for what lists playlists' songs.
    pub spotify_list: Playlist,
    /// Where the Spotify tab stands among the tabs.
    pub spotify_at: usize,
    /// When the queue was asked to play here: the player shows it loading until
    /// it plays here, rather than the local song it paused.
    pub spotify_starting: Option<Instant>,
    /// The player is the queue's: it was started last, before any local
    /// song. Paused, the queue keeps it until a local song plays.
    pub spotify_owns: bool,
    /// How each synced playlist stands, once it has been sent or read.
    pub sync: HashMap<PlaylistId, SyncStatus>,
    /// A playlist that wasn't synced because the Spotless folder has one
    /// by its name: asked whether to rename it or replace that one. It's
    /// synced once renamed.
    pub sync_name_taken: Option<PlaylistId>,
}

impl State {
    /// The player shows the account's queue, and its buttons act on it:
    /// it was started last, a device plays it or has it paused, here or
    /// elsewhere, and no local song plays.
    pub fn spotify_playing(&self) -> bool {
        self.spotify_starting.is_some()
            || self.spotify_owns
                && self.player.status != Status::Playing
                && self.spotify.as_ref().is_some_and(SpotifyQueue::active)
    }

    /// What's left to play of playlist `id` from its place, in ms: from
    /// where its song is, playing or not. None while that is all of it,
    /// with nothing played yet.
    pub fn left_ms(&self, id: PlaylistId) -> Option<u64> {
        let list = self.library.get(id)?;
        let playing = self.transport.playing_list() == Some(id);
        let at = if playing {
            self.player.position_now()
        } else {
            list.resume_ms
        };
        let stop_after = playing && self.transport.stop_after();
        let total: u64 = list
            .entries()
            .iter()
            .map(|entry| u64::from(entry.track.duration_ms))
            .sum();
        let left = self.transport.playback.left_ms(list, at, stop_after);
        (left < total).then_some(left)
    }

    /// What the player shows: the queue's song while it is the player's.
    pub fn now_playing(&self) -> PlayerSnapshot {
        match &self.spotify {
            Some(queue) if self.spotify_starting.is_some() => PlayerSnapshot {
                status: Status::Loading,
                ..queue.snapshot()
            },
            Some(queue) if self.spotify_playing() => queue.snapshot(),
            // Stopped, it shows what Play starts, if anything.
            _ if self.idle() => self
                .waiting()
                .map(|(_, waiting)| waiting)
                .unwrap_or_default(),
            _ => self.player.clone(),
        }
    }

    /// Nothing plays, nor waits offline.
    fn idle(&self) -> bool {
        !self.transport.active() && !self.offline && !self.spotify_playing()
    }

    /// While nothing plays: the playlist Play starts, and its song there
    /// as the player shows it: paused where Play goes on from (as after
    /// closing the app mid-song), else stopped. None when Play would start
    /// nothing.
    pub fn waiting(&self) -> Option<(PlaylistId, PlayerSnapshot)> {
        if !self.idle() {
            return None;
        }
        if self.viewed == PlaylistId::SPOTIFY {
            let queue = self.spotify.as_ref().filter(|queue| queue.now.is_some())?;
            let waiting = PlayerSnapshot {
                status: Status::Stopped,
                ..queue.snapshot()
            };
            return Some((PlaylistId::SPOTIFY, waiting));
        }
        let list = self.library.get(self.viewed)?;
        let id = list.cursor().or(list.first())?;
        let entry = list.get(id)?;
        let track = self.player_track(&entry.track);
        let position_ms = if list.cursor() == Some(id) {
            list.resume_ms
        } else {
            0
        };
        let waiting = PlayerSnapshot {
            status: if position_ms > 0 {
                Status::Paused
            } else {
                Status::Stopped
            },
            track: Some(track),
            position_ms,
            ..PlayerSnapshot::default()
        };
        Some((list.id, waiting))
    }

    /// The playlist a song plays from, the Spotify tab's included: not
    /// the queue's while it has no song, as after a clear, or is stopped.
    pub fn playing_list(&self) -> Option<PlaylistId> {
        if self.spotify_playing() {
            let playing = self.spotify_starting.is_some()
                || self.spotify.as_ref().is_some_and(|queue| {
                    queue.now.is_some() && queue.shown_status() != Status::Stopped
                });
            playing.then_some(PlaylistId::SPOTIFY)
        } else {
            self.transport.playing_list()
        }
    }

    /// Playback stops once the playing song ends (Winamp's Stop after
    /// current).
    pub fn stop_after(&self) -> bool {
        self.transport.stop_after()
    }

    /// The playlist whose song waits, paused, for the connection to
    /// Spotify.
    pub fn offline_list(&self) -> Option<PlaylistId> {
        self.transport.playback.playing().filter(|_| self.offline)
    }

    /// The tabs in order, the Spotify tab's included while it shows.
    pub fn tabs(&self) -> Vec<PlaylistId> {
        let mut tabs: Vec<_> = self.library.playlists().iter().map(|l| l.id).collect();
        if self.spotify.is_some() {
            tabs.insert(self.spotify_at.min(tabs.len()), PlaylistId::SPOTIFY);
        }
        tabs
    }

    /// A playlist by ID, the Spotify tab's included.
    pub fn list(&self, id: PlaylistId) -> Option<&Playlist> {
        if id == PlaylistId::SPOTIFY {
            self.spotify.as_ref().map(|_| &self.spotify_list)
        } else {
            self.library.get(id)
        }
    }

    /// A playlist's song as the player shows it: the engine's, which has
    /// the cover, while it's the engine's song.
    pub(super) fn player_track(&self, track: &TrackRef) -> TrackInfo {
        match &self.player.track {
            Some(shown) if shown.uri == track.uri => shown.clone(),
            _ => TrackInfo::from(track),
        }
    }

    /// The playlist the player's buttons and edits act on: the one shown,
    /// or with the Spotify tab shown, the one local playback played.
    pub(super) fn local_viewed(&self) -> PlaylistId {
        match self.viewed {
            PlaylistId::SPOTIFY => self
                .transport
                .playing_list()
                .unwrap_or(self.library.first()),
            viewed => viewed,
        }
    }
}
