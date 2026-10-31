//! The controller's part in playing: the player's buttons, what the engine
//! tells, and a song that waits offline for the connection to Spotify.

use super::{Controller, volume_level};
use crate::action::PlaybackAction;
use crate::backend::Command;
use crate::model::TrackRef;
use crate::playback::engine::{PlayerSnapshot, Status, TrackInfo};
use crate::playback::transport::Request;

impl Controller {
    /// While a song waits offline, Play goes on with it, and playing or
    /// stopping anything else gives the wait up. True when that was all
    /// there was to do.
    fn offline_first(&mut self, action: &PlaybackAction) -> bool {
        if self.state.offline {
            match action {
                // Shown paused: playing goes on where it stopped.
                PlaybackAction::Play | PlaybackAction::TogglePause => {
                    self.resume_offline();
                    return true;
                }
                PlaybackAction::Stop
                | PlaybackAction::Release
                | PlaybackAction::Next
                | PlaybackAction::Previous
                | PlaybackAction::PlayEntry(..) => {
                    self.state.offline = false;
                    self.state.player = PlayerSnapshot::default();
                }
                _ => {}
            }
        }
        if matches!(
            action,
            PlaybackAction::Stop
                | PlaybackAction::Release
                | PlaybackAction::Next
                | PlaybackAction::Previous
                | PlaybackAction::PlayEntry(..)
        ) {
            self.resuming = false;
        }
        false
    }

    /// The player's buttons, and songs played from a playlist or Browse:
    /// a song waiting offline first, then the queue while it's the
    /// player's (`spotify_tab.rs`), then the local player.
    pub(super) fn apply_playback(&mut self, action: PlaybackAction) {
        if self.offline_first(&action) {
            return;
        }
        let Some(action) = self.queue_playback(action) else {
            return;
        };
        let paused = self.state.player.status == Status::Paused;
        let position = self.state.player.position_now();
        // Nothing loaded, but a song shows paused: Pause plays it, and
        // seeking moves where it goes on from.
        let waiting_paused = self
            .state
            .waiting()
            .is_some_and(|(_, waiting)| waiting.status == Status::Paused);
        let viewed = self.state.local_viewed();
        let state = &mut self.state;
        let library = &mut state.library;
        let requests = match action {
            PlaybackAction::Seek(ms) if waiting_paused => {
                if let Some(list) = library.get_mut(viewed) {
                    // At 0 it would show stopped.
                    list.resume_ms = ms.max(1);
                }
                return self.mark_unsaved();
            }
            PlaybackAction::Seek(ms) => return self.send(Command::Seek(ms)),
            PlaybackAction::Volume(volume) => {
                state.settings.volume = volume;
                self.mark_unsaved();
                return self.send(Command::Volume(volume_level(volume)));
            }
            PlaybackAction::Play => state.transport.play(library, viewed, paused, position),
            PlaybackAction::TogglePause if waiting_paused => {
                state.transport.play(library, viewed, false, position)
            }
            PlaybackAction::TogglePause => state.transport.toggle_pause(paused),
            PlaybackAction::Stop => state.transport.stop(library),
            PlaybackAction::ToggleStopAfter => state.transport.toggle_stop_after(library),
            PlaybackAction::Release => state.transport.release(library, position),
            PlaybackAction::Next => state.transport.next(library, viewed),
            PlaybackAction::Previous => state.transport.prev(library, viewed, position),
            PlaybackAction::SetShuffle(on) => {
                state.settings.shuffle = on;
                state.transport.set_shuffle(library, on)
            }
            PlaybackAction::SetRepeat(on) => {
                state.settings.repeat = on;
                state.transport.set_repeat(library, on)
            }
            PlaybackAction::PlayEntry(list, entry) => {
                state.error = None;
                state.transport.play_entry(library, list, entry, position)
            }
            PlaybackAction::PlayNow(track) => self.play_now(track, position),
        };
        self.carry_out(requests);
    }

    /// What the engine tells of the player.
    pub(super) fn player_changed(&mut self, snapshot: PlayerSnapshot) {
        // The engine holds nothing while a song waits offline.
        if self.state.offline {
            return;
        }
        if self.resuming && snapshot.track.is_none() && snapshot.error.is_none() {
            if snapshot.status == Status::Playing {
                self.state.player.status = Status::Playing;
            }
            return;
        }
        self.resuming = false;
        if snapshot.error.is_some() {
            self.state.error = snapshot.error.clone();
        }
        // A local song playing takes the player.
        if snapshot.status == Status::Playing {
            self.state.spotify_owns = false;
        }
        self.state.player = snapshot;
    }

    /// The engine started the followed song by itself.
    pub(super) fn song_followed(&mut self, entry: crate::model::EntryId) {
        let requests = self
            .state
            .transport
            .followed(&mut self.state.library, entry);
        self.carry_out(requests);
    }

    /// The engine finished a song with nothing to follow.
    pub(super) fn song_ended(&mut self) {
        let requests = self.state.transport.ended(&mut self.state.library);
        self.carry_out(requests);
    }

    /// The loaded song can't be played, and not for want of a connection.
    pub(super) fn song_unavailable(&mut self) {
        self.resuming = false;
        let requests = self.state.transport.unavailable(&mut self.state.library);
        self.carry_out(requests);
    }

    /// The playing song stopped at `position_ms`, or didn't load, for want
    /// of a connection: it waits there, shown paused, until reconnected or
    /// played.
    /// librespot may say so twice: the second finds it waiting already.
    pub(super) fn wait_offline(&mut self, position_ms: u32) {
        self.resuming = false;
        let state = &mut self.state;
        let track = state.transport.current(&state.library).map(|entry| {
            match &state.player.track {
                // The engine's has the cover.
                Some(track) if track.uri == entry.track.uri => track.clone(),
                _ => TrackInfo::from(&entry.track),
            }
        });
        let requests = state.transport.offline(&mut state.library, position_ms);
        if requests.is_empty() {
            return;
        }
        state.offline = true;
        state.error = None;
        state.player = PlayerSnapshot {
            status: Status::Paused,
            track,
            position_ms,
            ..PlayerSnapshot::default()
        };
        self.carry_out(requests);
    }

    /// Plays the song that waited offline, from where it stopped.
    pub(super) fn resume_offline(&mut self) {
        self.state.offline = false;
        self.resuming = true;
        self.state.player.status = Status::Loading;
        let requests = self.state.transport.resume(&mut self.state.library);
        self.carry_out(requests);
    }

    /// Inserts `track` after the target playlist's current song and plays it.
    fn play_now(&mut self, track: TrackRef, position: u32) -> Vec<Request> {
        let state = &mut self.state;
        let target = state.browse.target;
        let entry = state.library.entry(track);
        let id = entry.id;
        let Some(list) = state.library.get_mut(target) else {
            return Vec::new();
        };
        let at = list
            .cursor()
            .and_then(|cursor| list.index_of(cursor))
            .map_or(list.len(), |i| i + 1);
        list.insert(at, vec![entry]);
        state.browse.flash([id]);
        state.error = None;
        let mut requests = state.transport.edited(&mut state.library, target);
        requests.extend(
            state
                .transport
                .play_entry(&mut state.library, target, id, position),
        );
        requests
    }

    /// The model changed: send what the engine must do, and save soon.
    pub(super) fn carry_out(&mut self, requests: Vec<Request>) {
        for request in requests {
            // Until the engine says more, the player shows the song
            // loading, not the one it had.
            if let Request::Load {
                entry, position_ms, ..
            } = &request
            {
                let state = &mut self.state;
                let loading = state
                    .transport
                    .current(&state.library)
                    .filter(|current| current.id == *entry);
                if let Some(current) = loading {
                    let track = match state.player.track.take() {
                        Some(track) if track.uri == current.track.uri => track,
                        _ => TrackInfo::from(&current.track),
                    };
                    state.player = PlayerSnapshot {
                        status: Status::Loading,
                        track: Some(track),
                        position_ms: *position_ms,
                        ..PlayerSnapshot::default()
                    };
                }
            }
            self.send(Command::Player(request));
        }
        self.mark_unsaved();
    }
}
