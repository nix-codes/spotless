//! The controller's part in Last.fm: connecting, hearts, and songs loved
//! there found on Spotify, to add or play.

use super::Controller;
use crate::action::{LastFmAction, PlaybackAction};
use crate::backend::Command;
use crate::lastfm::{LastFmState, LastFmStatus, LovedSong, api::Keys};
use crate::model::{PlaylistId, TrackRef};

impl Controller {
    /// Connects Last.fm or not, loves songs, and adds or plays songs loved.
    pub(super) fn apply_lastfm(&mut self, action: LastFmAction) {
        let state = &mut self.state;
        match action {
            LastFmAction::ConnectLastFm(api_key, secret) => {
                state.error = None;
                state.notice = Some("Allow Spotless in the Last.fm page that opens.".into());
                self.send(Command::LastFmConnect(Keys {
                    api_key: api_key.trim().into(),
                    secret: secret.trim().into(),
                }));
            }
            LastFmAction::DisconnectLastFm => {
                state.notice = Some("Last.fm disconnected.".into());
                self.send(Command::LastFmDisconnect);
            }
            LastFmAction::SetLoved {
                artist,
                title,
                loved,
            } => {
                // Shown at once; Last.fm's refusal sets it back.
                state.loved.set(&artist, &title, loved);
                self.send(Command::LastFmLove {
                    artist,
                    title,
                    loved,
                });
            }
            LastFmAction::AddLoved(songs) => {
                let target = self.state.browse.target;
                self.find_loved(songs, FoundFor::Insert(target, None));
            }
            LastFmAction::DropLoved(songs, at) => {
                let target = self.state.browse.target;
                self.find_loved(songs, FoundFor::Insert(target, Some(at)));
            }
            LastFmAction::PlayLoved(song) => self.find_loved(vec![song], FoundFor::Play),
        }
    }

    pub(super) fn lastfm_changed(&mut self, lastfm: LastFmState) {
        if self.state.lastfm.status == LastFmStatus::SigningIn
            && let LastFmStatus::Connected { user } = &lastfm.status
        {
            self.state.notice = Some(format!("Scrobbling to Last.fm as {user}."));
        }
        self.state.lastfm = lastfm;
    }

    /// Asks for loved songs on Spotify, for `then` once found.
    pub(super) fn find_loved(&mut self, songs: Vec<LovedSong>, then: FoundFor) {
        if songs.is_empty() {
            return;
        }
        if songs.len() > 1 {
            self.state.notice = Some(format!("Finding {} songs on Spotify…", songs.len()));
        }
        self.last_find += 1;
        let id = self.last_find;
        self.finding.push((id, songs.clone(), then));
        self.send(Command::FindSongs { id, songs });
    }

    /// Adds or plays loved songs found on Spotify, saying which weren't.
    pub(super) fn songs_found(&mut self, id: u64, result: Result<Vec<Option<TrackRef>>, String>) {
        let Some(at) = self.finding.iter().position(|(asked, ..)| *asked == id) else {
            return;
        };
        let (_, songs, then) = self.finding.remove(at);
        let found = match result {
            Ok(found) => found,
            Err(error) => {
                self.state.error = Some(format!("Songs couldn't be found on Spotify: {error}"));
                return;
            }
        };
        let tracks: Vec<TrackRef> = found.into_iter().flatten().collect();
        let missing = songs.len() - tracks.len();
        match then {
            FoundFor::Play => {
                if let Some(track) = tracks.into_iter().next() {
                    self.apply(PlaybackAction::PlayNow(track));
                }
            }
            FoundFor::Insert(list, at) => {
                if !tracks.is_empty() {
                    self.insert_tracks(list, at, tracks);
                }
            }
        }
        if missing == 0 {
            return;
        }
        let state = &mut self.state;
        let not_found = match (&songs[..], missing) {
            ([song], _) => format!("{} - {} isn't on Spotify", song.artist, song.title),
            (all, n) if n == all.len() => format!("None of the {n} songs are on Spotify"),
            (all, 1) => format!("1 of the {} songs isn't on Spotify", all.len()),
            (all, n) => format!("{n} of the {} songs aren't on Spotify", all.len()),
        };
        state.notice = Some(match state.notice.take() {
            Some(added) if missing < songs.len() => format!("{added} · {not_found}"),
            _ => not_found,
        });
    }
}

/// What songs loved on Last.fm are found on Spotify for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FoundFor {
    /// A playlist, at an index or at the end.
    Insert(PlaylistId, Option<usize>),
    /// Playing the song, as PlayNow.
    Play,
}

impl FoundFor {
    pub(super) fn goes_to(self, list: PlaylistId) -> bool {
        matches!(self, Self::Insert(to, _) if to == list)
    }
}
