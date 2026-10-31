//! The controller's part in the Spotify tab: Spotless in Spotify Connect,
//! showing the account's queue, with the player's buttons acting on it
//! while it's the player's.

use std::time::{Duration, Instant};

use super::Controller;
use crate::action::{PlaybackAction, PlaylistsAction, SpotifyTabAction};
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

    /// The Spotify tab's own actions.
    pub(super) fn apply_spotify_tab(&mut self, action: SpotifyTabAction) {
        let state = &mut self.state;
        let request = match action {
            // Shown once its device has joined.
            SpotifyTabAction::OpenSpotifyTab => {
                state.settings.spotify_tab = true;
                state.viewed = PlaylistId::SPOTIFY;
                return self.spotify_tab_changed();
            }
            SpotifyTabAction::SetSpotifyTabEnabled(on) => {
                state.settings.spotify_tab_enabled = on;
                if on {
                    state.settings.spotify_tab = true;
                }
                return self.spotify_tab_changed();
            }
            // Held elsewhere, the tab only shows the queue.
            SpotifyTabAction::ClearSpotify => match &state.spotify {
                Some(queue) if !queue.held_elsewhere() => SpotifyRequest::Clear,
                _ => return,
            },
            // The player shows the queue, paused here once it comes.
            SpotifyTabAction::TakeOverSpotify => match &state.spotify {
                Some(queue) if queue.held_elsewhere() => {
                    state.spotify_owns = true;
                    SpotifyRequest::TakeOver
                }
                _ => return,
            },
        };
        self.send_to_queue(request);
    }

    /// Does what the player's buttons asked of the queue while it's the
    /// player's, or while the Spotify tab shows. Hands back what's the
    /// local player's to do.
    pub(super) fn queue_playback(&mut self, action: PlaybackAction) -> Option<PlaybackAction> {
        let state = &mut self.state;
        // The system wants the sound gone (Android): the queue playing
        // here pauses, besides the local song letting go.
        if action == PlaybackAction::Release {
            if state.spotify_playing() && state.spotify.as_ref().is_some_and(|q| q.playing_here()) {
                state.spotify_starting = None;
                self.send(Command::Spotify(SpotifyRequest::Pause));
            }
            return Some(action);
        }
        // The player's buttons act on the queue, wherever it plays.
        let here = state.spotify_playing();
        let shown = state.viewed == PlaylistId::SPOTIFY;
        let request = match action {
            PlaybackAction::Play if here => SpotifyRequest::Play,
            // The local song paused shows: Play goes on with it.
            PlaybackAction::Play if shown && state.player.status == Status::Paused => {
                return Some(PlaybackAction::TogglePause);
            }
            PlaybackAction::Play if shown => SpotifyRequest::PlayHere(None),
            PlaybackAction::TogglePause if here => SpotifyRequest::TogglePause,
            PlaybackAction::Stop if here => SpotifyRequest::Stop,
            PlaybackAction::Next if here => SpotifyRequest::Next,
            PlaybackAction::Previous if here => SpotifyRequest::Previous,
            PlaybackAction::Seek(position) if here => {
                if let Some(queue) = &mut state.spotify {
                    queue.position_ms = position;
                    queue.reported_at = Some(Instant::now());
                    self.spotify_seeked = Some(Instant::now());
                }
                SpotifyRequest::Seek(position)
            }
            // Played here from that song: what played before can't be.
            PlaybackAction::PlayEntry(PlaylistId::SPOTIFY, row) => {
                match state.spotify.as_ref()?.row(row)? {
                    Row::Now(_) => SpotifyRequest::PlayHere(None),
                    Row::Next(song) => SpotifyRequest::PlayHere(Some(song.track.uri.clone())),
                    Row::Before(_) => return None,
                }
            }
            PlaybackAction::PlayNow(track) if state.browse.target == PlaylistId::SPOTIFY => {
                self.insert_tracks(PlaylistId::SPOTIFY, None, vec![track]);
                return None;
            }
            other => {
                // Something else to play instead.
                if matches!(
                    other,
                    PlaybackAction::Play
                        | PlaybackAction::PlayEntry(..)
                        | PlaybackAction::PlayNow(_)
                ) {
                    state.spotify_starting = None;
                    state.spotify_owns = false;
                }
                return Some(other);
            }
        };
        self.send_to_queue(request);
        None
    }

    /// Does what an edit of the tabs or the songs asked of the Spotify
    /// tab. Hands back what's the playlists' to do.
    pub(super) fn queue_playlists(&mut self, action: PlaylistsAction) -> Option<PlaylistsAction> {
        let state = &mut self.state;
        let shown = state.viewed == PlaylistId::SPOTIFY;
        match action {
            // Closed, Spotless leaves Spotify Connect; the queue stays the
            // account's, to go on with elsewhere.
            PlaylistsAction::ClosePlaylist(PlaylistId::SPOTIFY) => {
                state.settings.spotify_tab = false;
                self.spotify_tab_changed();
            }
            // While the Spotify tab shows, the index counts it too.
            PlaylistsAction::MovePlaylist(list, index) if state.spotify.is_some() => {
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
            }
            PlaylistsAction::SelectPlaylist(PlaylistId::SPOTIFY) => {
                if state.spotify.is_some() {
                    state.viewed = PlaylistId::SPOTIFY;
                }
            }
            PlaylistsAction::Remove(PlaylistId::SPOTIFY, rows) => {
                let queue = state.spotify.as_ref()?;
                let songs = rows
                    .iter()
                    .filter_map(|&id| match queue.row(id) {
                        Some(Row::Next(song)) => Some((song.index, song.track.uri.clone())),
                        _ => None,
                    })
                    .collect();
                self.send_to_queue(SpotifyRequest::Remove(songs));
            }
            PlaylistsAction::Move(..) | PlaylistsAction::ToggleMarks(_) if shown => {}
            other => return Some(other),
        }
        None
    }

    /// Sends the queue a request, and starts or ends the wait for it to
    /// play here.
    fn send_to_queue(&mut self, request: SpotifyRequest) {
        let state = &mut self.state;
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
