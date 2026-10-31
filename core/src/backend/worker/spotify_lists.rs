//! The worker's part in the account's Spotify playlists: reading them,
//! taking songs or playlists out and putting them back, and copying
//! songs to them.

use super::Worker;
use crate::backend::{Command, Event};
use crate::browse::{Removal, Unlisted};
use crate::model::catalog::{Place, SpotifyTarget};
use crate::spotify::{catalog, playlists};

impl Worker {
    /// Reads and edits the account's Spotify playlists.
    pub(super) fn handle_spotify_lists(&mut self, command: Command) {
        match command {
            Command::SpotifyPlaylists => {
                self.on_session(move |session| async move {
                    let result = catalog::playlists(&session)
                        .await
                        .map_err(|e| format!("{e:#}"));
                    Event::SpotifyPlaylists(result)
                });
            }
            Command::RemoveFromSpotify { removal, revision } => {
                self.edit_spotify(removal, Some(revision));
            }
            Command::PutBackOnSpotify(removal) => self.edit_spotify(removal, None),
            Command::UnlistSpotifyPlaylist {
                playlist,
                name,
                notice,
            } => {
                self.on_session(move |session| async move {
                    let result = match playlists::unlist(&session, &playlist).await {
                        Ok((at, item)) => Ok(Unlisted {
                            playlist,
                            name,
                            at,
                            item,
                            notice,
                        }),
                        Err(e) => Err(format!("{e:#}")),
                    };
                    Event::SpotifyUnlisted {
                        result,
                        undo: false,
                    }
                });
            }
            Command::RelistSpotifyPlaylist(unlisted) => {
                self.on_session(move |session| async move {
                    let result =
                        match playlists::relist(&session, unlisted.at, &unlisted.item).await {
                            Ok(()) => Ok(unlisted),
                            Err(e) => Err(format!("{e:#}")),
                        };
                    Event::SpotifyUnlisted { result, undo: true }
                });
            }
            Command::CopyToSpotify { target, songs } => {
                self.on_session(move |session| async move {
                    let result = match &target {
                        SpotifyTarget::New { name, private } => {
                            playlists::create(&session, name, *private, &songs).await
                        }
                        SpotifyTarget::Playlist { uri, .. } => {
                            playlists::add(&session, uri, &songs)
                                .await
                                .map(|()| uri.clone())
                        }
                    };
                    Event::CopiedToSpotify {
                        target,
                        songs: songs.len(),
                        result: result.map_err(|e| format!("{e:#}")),
                    }
                });
            }
            _ => {}
        }
    }

    /// Removes songs from a Spotify playlist or Liked Songs, as they were
    /// at `revision`; or with None puts them back.
    fn edit_spotify(&mut self, removal: Removal, revision: Option<Vec<u8>>) {
        self.on_session(move |session| async move {
            let rows: Vec<(usize, String)> = removal
                .songs
                .iter()
                .filter_map(|(place, uri)| match place {
                    Place::At(at) => Some((*at, uri.clone())),
                    Place::Liked(_) => None,
                })
                .collect();
            let liked: Vec<(String, i64)> = removal
                .songs
                .iter()
                .filter_map(|(place, uri)| match place {
                    Place::Liked(at) => Some((uri.clone(), *at)),
                    Place::At(_) => None,
                })
                .collect();
            let playlist = &removal.playlist;
            let result = if playlist == catalog::LIKED_SONGS {
                match revision {
                    Some(_) => {
                        let uris: Vec<String> = liked.into_iter().map(|(uri, _)| uri).collect();
                        playlists::unlike(&session, &uris).await
                    }
                    None => playlists::like(&session, &liked).await,
                }
            } else {
                match &revision {
                    Some(revision) => {
                        playlists::remove_at(&session, playlist, revision.clone(), &rows)
                            .await
                            .map(|_| ())
                    }
                    None => playlists::put_back(&session, playlist, &rows).await,
                }
            };
            Event::SpotifyEdited {
                removal,
                undo: revision.is_none(),
                result: result.map_err(|e| format!("{e:#}")),
            }
        });
    }
}
