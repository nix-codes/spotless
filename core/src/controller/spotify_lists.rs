//! The controller's part in the account's Spotify playlists, in Browse's
//! YOUR SPOTIFY: reading them, taking songs or playlists out with UNDO,
//! and copying songs to them.

use super::Controller;
use crate::action::Action;
use crate::backend::{Command, SessionState};
use crate::browse::{Loadable, Removal, Undo, Unlisted};
use crate::model::TrackRef;
use crate::model::catalog::{Place, SpotifyPlaylist, SpotifyTarget};

impl Controller {
    /// Reads, opens and edits the account's Spotify playlists.
    pub(super) fn apply_spotify_lists(&mut self, action: Action) {
        match action {
            Action::ReadSpotifyPlaylists => self.read_spotify_playlists(),
            Action::OpenSpotifyPlaylist(uri) => {
                self.state.browse.spotify_open = Some(uri.clone());
                self.read_album(uri);
                self.mark_unsaved();
            }
            Action::RemoveFromSpotify(uri, rows) => self.remove_from_spotify(uri, rows),
            Action::UnlistSpotifyPlaylist(uri) => self.unlist_spotify_playlist(uri),
            Action::UndoSpotifyRemoval => match self.state.browse.undo.take() {
                Some(Undo::Songs(removal)) => {
                    self.state.notice =
                        Some(format!("Putting the songs back in “{}”…", removal.name));
                    self.send(Command::PutBackOnSpotify(removal));
                }
                Some(Undo::Playlist(unlisted)) => {
                    self.state.notice = Some(format!("Putting “{}” back…", unlisted.name));
                    self.send(Command::RelistSpotifyPlaylist(unlisted));
                }
                None => {}
            },
            Action::CopyToSpotify(target, tracks) => self.copy_to_spotify(target, tracks),
            _ => {}
        }
    }

    /// Takes songs off the page at once, and out of the Spotify playlist,
    /// by their places when it was read: Spotify fits removals made since
    /// to how it is now.
    fn remove_from_spotify(&mut self, uri: String, mut rows: Vec<usize>) {
        let browse = &mut self.state.browse;
        if !matches!(self.state.session, SessionState::Ready { .. }) || !browse.editable(&uri) {
            return;
        }
        let name = browse
            .spotify_playlist(&uri)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        let (Some(Loadable::Ready(tracks)), Some((revision, places))) =
            (browse.albums.get_mut(&uri), browse.places.get_mut(&uri))
        else {
            return;
        };
        rows.sort_unstable();
        rows.dedup();
        rows.retain(|&row| row < tracks.len() && row < places.len());
        if rows.is_empty() {
            return;
        }
        let songs: Vec<(Place, String)> = rows
            .iter()
            .map(|&row| (places[row], tracks[row].uri.clone()))
            .collect();
        for &row in rows.iter().rev() {
            tracks.remove(row);
            places.remove(row);
        }
        let revision = revision.clone();
        let notice = format!("Removed {} from “{name}”.", count_songs(songs.len()));
        let removal = Removal {
            playlist: uri,
            name,
            songs,
            notice,
        };
        self.send(Command::RemoveFromSpotify { removal, revision });
    }

    pub(super) fn spotify_edited(
        &mut self,
        removal: Removal,
        undo: bool,
        result: Result<(), String>,
    ) {
        match result {
            Ok(()) if undo => {
                self.state.notice = Some(format!(
                    "Put {} back in “{}”.",
                    count_songs(removal.songs.len()),
                    removal.name
                ));
            }
            Ok(()) => {
                self.state.notice = Some(removal.notice.clone());
                self.state.browse.undo = Some(Undo::Songs(removal.clone()));
            }
            Err(error) => self.state.error = Some(error),
        }
        // Spotify's places from now on (an undo needs them), and the count.
        self.reread_songs(removal.playlist);
        self.read_spotify_playlists();
    }

    /// Takes a Spotify playlist off YOUR SPOTIFY at once, the one beside
    /// it shown if it was, and out of the account's list.
    fn unlist_spotify_playlist(&mut self, uri: String) {
        let browse = &mut self.state.browse;
        if !matches!(self.state.session, SessionState::Ready { .. }) || !browse.unlistable(&uri) {
            return;
        }
        let Some(Loadable::Ready(playlists)) = &mut browse.spotify_playlists else {
            return;
        };
        // As the list shows them: the account's own, then those followed.
        let order: Vec<&SpotifyPlaylist> = playlists
            .iter()
            .filter(|p| p.owner.is_none())
            .chain(playlists.iter().filter(|p| p.owner.is_some()))
            .collect();
        let Some(at) = order.iter().position(|p| p.uri == uri) else {
            return;
        };
        let playlist = order[at].clone();
        let beside = order
            .get(at + 1)
            .or_else(|| at.checked_sub(1).and_then(|before| order.get(before)))
            .map(|p| p.uri.clone());
        playlists.retain(|p| p.uri != uri);
        if browse.spotify_open.as_ref() == Some(&uri) {
            browse.spotify_open = beside.clone();
            if let Some(beside) = beside {
                self.read_album(beside);
            }
            self.mark_unsaved();
        }
        let name = playlist.name;
        let synced = self.state.library.synced_seen.contains(&uri);
        let notice = match playlist.owner {
            None if synced => format!("Deleted “{name}” on Spotify and every device."),
            None => format!("Deleted “{name}” on Spotify."),
            Some(_) => format!("Unfollowed “{name}”."),
        };
        self.send(Command::UnlistSpotifyPlaylist {
            playlist: uri,
            name,
            notice,
        });
    }

    pub(super) fn spotify_unlisted(&mut self, result: Result<Unlisted, String>, undo: bool) {
        match result {
            Ok(unlisted) if undo => {
                self.state.notice = Some(format!("Put “{}” back.", unlisted.name));
            }
            Ok(unlisted) if self.desynced.remove(&unlisted.playlist) => {
                self.state.notice = Some(unlisted.notice);
            }
            Ok(unlisted) => {
                self.state.notice = Some(unlisted.notice.clone());
                // A synced one open here closes: it's gone everywhere.
                if let Some(list) = self.state.library.synced(&unlisted.playlist) {
                    let id = list.id;
                    self.apply(Action::ClosePlaylist(id));
                }
                self.state.browse.undo = Some(Undo::Playlist(unlisted));
            }
            Err(error) => self.state.error = Some(error),
        }
        // Spotify's list from now on: a playlist put back, or not taken
        // out, shows again.
        self.read_spotify_playlists();
    }

    /// Reads a Spotify playlist's songs again, still showing those read.
    fn reread_songs(&mut self, uri: String) {
        match self.state.browse.albums.get(&uri) {
            Some(Loadable::Ready(_)) => self.send(Command::Album(uri)),
            _ => {
                self.state.browse.albums.remove(&uri);
                self.read_album(uri);
            }
        }
    }

    fn copy_to_spotify(&mut self, target: SpotifyTarget, tracks: Vec<TrackRef>) {
        if !matches!(self.state.session, SessionState::Ready { .. }) {
            self.state.error = Some("Sign in to Spotify first.".into());
            return;
        }
        let songs: Vec<String> = tracks
            .into_iter()
            .map(|track| track.uri)
            .filter(|uri| uri.starts_with("spotify:track:"))
            .collect();
        if songs.is_empty() {
            return;
        }
        self.state.notice = Some(format!("Copying to “{}” on Spotify…", target.name()));
        self.send(Command::CopyToSpotify { target, songs });
    }

    pub(super) fn copied_to_spotify(
        &mut self,
        target: SpotifyTarget,
        songs: usize,
        result: Result<String, String>,
    ) {
        match result {
            Ok(_) => {
                self.state.notice = Some(format!(
                    "Copied {} to “{}” on Spotify.",
                    count_songs(songs),
                    target.name()
                ));
                // The new count makes the songs read before stale.
                self.read_spotify_playlists();
            }
            Err(error) => self.state.error = Some(error),
        }
    }

    /// Reads the account's Spotify playlists again, as they may have
    /// changed on Spotify, still showing those read before meanwhile.
    pub(super) fn read_spotify_playlists(&mut self) {
        let browse = &mut self.state.browse;
        if !matches!(self.state.session, SessionState::Ready { .. })
            || matches!(browse.spotify_playlists, Some(Loadable::Loading))
        {
            return;
        }
        if !matches!(browse.spotify_playlists, Some(Loadable::Ready(_))) {
            browse.spotify_playlists = Some(Loadable::Loading);
        }
        self.send(Command::SpotifyPlaylists);
    }

    /// Keeps the playlists read, and forgets the songs read before of
    /// those that may have changed: all but the ones shown, unless their
    /// number changed.
    pub(super) fn spotify_playlists_read(&mut self, result: Result<Vec<SpotifyPlaylist>, String>) {
        let playlists = match result {
            Ok(playlists) => playlists,
            Err(error) => {
                // Those read before stay.
                if !matches!(
                    self.state.browse.spotify_playlists,
                    Some(Loadable::Ready(_))
                ) {
                    self.state.browse.spotify_playlists = Some(Loadable::Failed(error));
                }
                return;
            }
        };
        for playlist in &playlists {
            let browse = &mut self.state.browse;
            let shown = browse.expanded.contains(&playlist.uri)
                || browse.spotify_open.as_ref() == Some(&playlist.uri);
            let stale = match browse.albums.get(&playlist.uri) {
                Some(Loadable::Ready(songs)) => !shown || songs.len() != playlist.tracks,
                Some(Loadable::Failed(_)) => true,
                Some(Loadable::Loading) | None => false,
            };
            if stale && shown {
                self.reread_songs(playlist.uri.clone());
            } else if stale {
                browse.albums.remove(&playlist.uri);
                browse.places.remove(&playlist.uri);
            }
        }
        self.state.browse.spotify_playlists = Some(Loadable::Ready(playlists));
    }
}

/// "1 song", "3 songs".
fn count_songs(count: usize) -> String {
    match count {
        1 => "1 song".to_string(),
        n => format!("{n} songs"),
    }
}
