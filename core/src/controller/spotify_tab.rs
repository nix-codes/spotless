//! The controller's part in the Spotify tab: Spotless in Spotify Connect,
//! showing the account's queue, with the player's buttons acting on it
//! while it's the player's.

use std::time::{Duration, Instant};

use super::Controller;
use crate::action::Action;
use crate::backend::{Command, SpotifyRequest};
use crate::model::{Playlist, PlaylistId, TrackRef};
use crate::playback::engine::Status;
use crate::spotify::queue::{Row, SpotifyQueue};

/// How long after seeking the queue its shown position outlasts Spotify's
/// reports, which may still be of the position before.
const SEEK_SHOWN: Duration = Duration::from_millis(1_500);
/// How long the queue asked to play here shows loading before giving up.
const SPOTIFY_START_WAIT: Duration = Duration::from_secs(10);

impl Controller {
    /// The account's queue as Spotify last told, or None out of Connect.
    pub(super) fn spotify_changed(&mut self, mut queue: Option<SpotifyQueue>) {
        if let (Some(new), Some(old)) = (&mut queue, &self.state.spotify)
            && self
                .spotify_seeked
                .is_some_and(|at| at.elapsed() < SEEK_SHOWN)
            && new.now == old.now
        {
            new.position_ms = old.position_ms;
            new.reported_at = old.reported_at;
        }
        if queue
            .as_ref()
            .is_some_and(|q| q.here && q.status == Status::Playing)
        {
            self.state.spotify_starting = None;
        }
        // The queue takes the player as it starts playing, on any
        // device, or as it is first found with no local song.
        let old = self.state.spotify.as_ref();
        if let Some(new) = &queue
            && match old {
                Some(old) => {
                    new.status == Status::Playing
                        && (old.status != Status::Playing || old.device_id != new.device_id)
                }
                None => new.active() && self.state.player.status == Status::Stopped,
            }
        {
            self.state.spotify_owns = true;
        }
        self.state.spotify_list = queue.as_ref().map_or_else(
            || Playlist::new(PlaylistId::SPOTIFY, "Spotify"),
            SpotifyQueue::as_playlist,
        );
        self.state.spotify = queue;
    }

    /// Gives up on the queue asked to play here once it has taken too
    /// long. Returns how long until then, while it's awaited.
    pub(super) fn starting_tick(&mut self) -> Option<Duration> {
        let starting = self.state.spotify_starting.map(|at| {
            SPOTIFY_START_WAIT
                .checked_sub(at.elapsed())
                .unwrap_or_default()
        });
        if starting.is_some_and(|wait| wait.is_zero()) {
            self.state.spotify_starting = None;
            self.state.notice = Some("Spotify didn't hand the queue over.".into());
            // The local song it paused shows again, unless the queue plays.
            self.state.spotify_owns = self
                .state
                .spotify
                .as_ref()
                .is_some_and(|q| q.status == Status::Playing);
        }
        starting.filter(|wait| !wait.is_zero())
    }

    /// Joins or leaves Spotify Connect as the Spotify tab now shows or not.
    /// Gone, it was never started: the player and the tab shown move on.
    pub(super) fn spotify_tab_changed(&mut self) {
        let state = &mut self.state;
        let shown = state.settings.spotify_tab_shown();
        if !shown {
            state.spotify_starting = None;
            state.spotify_owns = false;
            if state.viewed == PlaylistId::SPOTIFY {
                state.viewed = state.library.first();
            }
        }
        self.mark_unsaved();
        self.send(Command::SpotifyTab(shown));
    }

    /// Does what the Spotify tab, or the player while the tab plays, asked
    /// for. Hands back any other action.
    pub(super) fn apply_spotify(&mut self, action: Action) -> Option<Action> {
        match action {
            // Closed, Spotless leaves Spotify Connect; the queue stays the
            // account's, to go on with elsewhere.
            Action::ClosePlaylist(PlaylistId::SPOTIFY) => {
                self.state.settings.spotify_tab = false;
                self.spotify_tab_changed();
                return None;
            }
            // Shown once its device has joined.
            Action::OpenSpotifyTab => {
                self.state.settings.spotify_tab = true;
                self.state.viewed = PlaylistId::SPOTIFY;
                self.spotify_tab_changed();
                return None;
            }
            // While the Spotify tab shows, the index counts it too.
            Action::MovePlaylist(list, index) if self.state.spotify.is_some() => {
                let state = &mut self.state;
                let mut tabs = state.tabs();
                tabs.retain(|&id| id != list);
                tabs.insert(index.min(tabs.len()), list);
                if let Some(at) = tabs.iter().position(|&id| id == PlaylistId::SPOTIFY) {
                    state.spotify_at = at;
                    tabs.remove(at);
                }
                for (at, id) in tabs.into_iter().enumerate() {
                    state.library.move_playlist(id, at);
                }
                self.mark_unsaved();
                return None;
            }
            // The system wants the sound gone (Android): the queue playing
            // here pauses, besides the local song letting go.
            Action::Release
                if self.state.spotify_playing()
                    && self
                        .state
                        .spotify
                        .as_ref()
                        .is_some_and(|q| q.playing_here()) =>
            {
                self.state.spotify_starting = None;
                self.send(Command::Spotify(SpotifyRequest::Pause));
                return Some(Action::Release);
            }
            Action::SetSpotifyTabEnabled(on) => {
                self.state.settings.spotify_tab_enabled = on;
                if on {
                    self.state.settings.spotify_tab = true;
                }
                self.spotify_tab_changed();
                return None;
            }
            _ => {}
        }
        let state = &mut self.state;
        // The player's buttons act on the queue, wherever it plays.
        let here = state.spotify_playing();
        let shown = state.viewed == PlaylistId::SPOTIFY;
        let request = match action {
            Action::Play if here => SpotifyRequest::Play,
            // The local song paused shows: Play goes on with it.
            Action::Play if shown && state.player.status == Status::Paused => {
                return Some(Action::TogglePause);
            }
            Action::Play if shown => SpotifyRequest::PlayHere(None),
            Action::TogglePause if here => SpotifyRequest::TogglePause,
            Action::Stop if here => SpotifyRequest::Stop,
            Action::Next if here => SpotifyRequest::Next,
            Action::Previous if here => SpotifyRequest::Previous,
            Action::Seek(position) if here => {
                if let Some(queue) = &mut state.spotify {
                    queue.position_ms = position;
                    queue.reported_at = Some(Instant::now());
                    self.spotify_seeked = Some(Instant::now());
                }
                SpotifyRequest::Seek(position)
            }
            Action::SelectPlaylist(PlaylistId::SPOTIFY) => {
                if state.spotify.is_some() {
                    state.viewed = PlaylistId::SPOTIFY;
                }
                return None;
            }
            // Played here from that song: what played before can't be.
            Action::PlayEntry(PlaylistId::SPOTIFY, row) => {
                match state.spotify.as_ref()?.row(row)? {
                    Row::Now(_) => SpotifyRequest::PlayHere(None),
                    Row::Next(song) => SpotifyRequest::PlayHere(Some(song.track.uri.clone())),
                    Row::Before(_) => return None,
                }
            }
            // Held elsewhere, the tab only shows the queue.
            Action::ClearSpotify if state.spotify.as_ref()?.held_elsewhere() => return None,
            Action::ClearSpotify => SpotifyRequest::Clear,
            // The player shows the queue, paused here once it comes.
            Action::TakeOverSpotify if state.spotify.as_ref()?.held_elsewhere() => {
                state.spotify_owns = true;
                SpotifyRequest::TakeOver
            }
            Action::TakeOverSpotify => return None,
            Action::Remove(PlaylistId::SPOTIFY, rows) => {
                let queue = state.spotify.as_ref()?;
                let songs = rows
                    .iter()
                    .filter_map(|&id| match queue.row(id) {
                        Some(Row::Next(song)) => Some((song.index, song.track.uri.clone())),
                        _ => None,
                    })
                    .collect();
                SpotifyRequest::Remove(songs)
            }
            Action::Move(..) | Action::ToggleMarks(_) if shown => return None,
            Action::PlayNow(track) if state.browse.target == PlaylistId::SPOTIFY => {
                self.insert_tracks(PlaylistId::SPOTIFY, None, vec![track]);
                return None;
            }
            other => {
                // Something else to play instead.
                if matches!(
                    other,
                    Action::Play | Action::PlayEntry(..) | Action::PlayNow(_)
                ) {
                    state.spotify_starting = None;
                    state.spotify_owns = false;
                }
                return Some(other);
            }
        };
        let playing_here = state
            .spotify
            .as_ref()
            .is_some_and(|q| q.here && q.status == Status::Playing);
        if matches!(request, SpotifyRequest::PlayHere(_))
            && state.spotify.is_some()
            && !playing_here
        {
            state.spotify_starting = Some(Instant::now());
            state.spotify_owns = true;
        } else if !matches!(request, SpotifyRequest::Add(_) | SpotifyRequest::Remove(_)) {
            // Any other button on the player ends the wait for the queue.
            state.spotify_starting = None;
        }
        self.send(Command::Spotify(request));
        None
    }

    /// Adds songs to the end of the account's queue, while it takes them.
    pub(super) fn queue_tracks(&mut self, tracks: Vec<TrackRef>) -> bool {
        let Some(queue) = &self.state.spotify else {
            return false;
        };
        if !queue.takes_songs() {
            let device = queue.device.as_deref().unwrap_or_default();
            self.state.error = Some(format!(
                "Spotify only takes songs for the queue from the device playing it ({device}). TAKE OVER the queue first, or play a song of it here."
            ));
            return false;
        }
        self.state.notice = Some(match tracks.len() {
            1 => "Added 1 song to Spotify's queue".to_string(),
            n => format!("Added {n} songs to Spotify's queue"),
        });
        let uris = tracks.into_iter().map(|track| track.uri).collect();
        self.send(Command::Spotify(SpotifyRequest::Add(uris)));
        true
    }
}
