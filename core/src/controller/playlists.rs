//! The controller's part in the playlists (the tabs): making, closing,
//! renaming and moving them, and adding, moving and removing their songs.

use super::Controller;
use crate::action::Action;
use crate::backend::Command;
use crate::model::{EntryId, PlaylistId, TrackRef};

impl Controller {
    /// Makes, closes, renames or moves a playlist, or edits its songs.
    pub(super) fn apply_playlists(&mut self, action: Action) {
        let viewed = self.state.local_viewed();
        let state = &mut self.state;
        let library = &mut state.library;
        let requests = match action {
            Action::AddLink(text) => {
                state.error = None;
                state.notice = Some("Reading the link…".into());
                let playlist = if state.browse.open {
                    state.browse.target
                } else {
                    viewed
                };
                return self.send(Command::AddLink { text, playlist });
            }
            Action::SelectPlaylist(list) => {
                if library.get(list).is_some() {
                    state.viewed = list;
                    self.mark_unsaved();
                }
                return;
            }
            Action::NewPlaylist => {
                let name = library.fresh_name();
                state.viewed = library.create(name);
                return self.mark_unsaved();
            }
            Action::NewPlaylistWith(tracks) => {
                let name = library.fresh_name();
                let list = library.create(name);
                state.viewed = list;
                self.insert_tracks(list, None, tracks);
                return self.mark_unsaved();
            }
            Action::ClosePlaylist(list) => {
                let index = library.playlists().iter().position(|l| l.id == list);
                let requests = state.transport.deleting(list);
                if !library.delete(list) {
                    return;
                }
                // The tab before it takes its place.
                let neighbour = index
                    .and_then(|i| library.playlists().get(i.saturating_sub(1)))
                    .map_or(library.first(), |l| l.id);
                if state.viewed == list {
                    state.viewed = neighbour;
                }
                if state.browse.target == list {
                    state.browse.target = state.viewed;
                }
                self.pending_adds.retain(|(_, target, _)| *target != list);
                self.finding.retain(|(_, _, then)| !then.goes_to(list));
                requests
            }
            Action::RenamePlaylist(list, name) => {
                library.rename(list, &name);
                let to_sync = state.sync_name_taken == Some(list);
                self.mark_unsaved();
                // Renamed for want of a name free in the Spotless folder:
                // synced now, or asked again.
                if to_sync {
                    self.set_sync(list, true, false);
                }
                return;
            }
            Action::MovePlaylist(list, index) => {
                library.move_playlist(list, index);
                return self.mark_unsaved();
            }
            Action::Remove(list, entries) => {
                if let Some(playlist) = library.get_mut(list) {
                    playlist.remove(&entries);
                }
                state.transport.edited(library, list)
            }
            Action::Move(entries, index) => {
                if let Some(list) = library.get_mut(viewed) {
                    list.move_to(&entries, index);
                }
                state.transport.edited(library, viewed)
            }
            Action::ToggleMarks(entries) => {
                if let Some(list) = library.get_mut(viewed) {
                    for entry in entries {
                        list.toggle_mark(entry);
                    }
                }
                state.transport.edited(library, viewed)
            }
            Action::CopyTo(list, tracks) => {
                self.insert_tracks(list, None, tracks);
                return;
            }
            Action::MoveTo(from, to, entries) => return self.move_tracks(from, to, entries),
            _ => return,
        };
        self.carry_out(requests);
    }

    /// Adds songs to a playlist before the row at `at`, or at the end. The
    /// Spotify tab's go to the end of the queue.
    pub(super) fn insert_tracks(
        &mut self,
        playlist: PlaylistId,
        at: Option<usize>,
        tracks: Vec<TrackRef>,
    ) -> bool {
        if playlist == PlaylistId::SPOTIFY {
            return self.queue_tracks(tracks);
        }
        let state = &mut self.state;
        let entries: Vec<_> = tracks
            .into_iter()
            .map(|track| state.library.entry(track))
            .collect();
        let ids: Vec<_> = entries.iter().map(|entry| entry.id).collect();
        let Some(list) = state.library.get_mut(playlist) else {
            return false;
        };
        list.insert(at.unwrap_or(usize::MAX), entries);
        state.notice = Some(match ids.len() {
            1 => format!("Added 1 song to {}", list.name),
            n => format!("Added {n} songs to {}", list.name),
        });
        state.browse.flash(ids);
        let requests = state.transport.edited(&mut state.library, playlist);
        self.carry_out(requests);
        true
    }

    /// Takes songs out of one playlist and adds them to the end of another,
    /// once that one has them.
    pub(super) fn move_tracks(&mut self, from: PlaylistId, to: PlaylistId, entries: Vec<EntryId>) {
        if from == to {
            return;
        }
        if from == PlaylistId::SPOTIFY {
            return self.move_from_spotify(to, entries);
        }
        let Some(list) = self.state.library.get(from) else {
            return;
        };
        let tracks: Vec<TrackRef> = entries
            .iter()
            .filter_map(|&id| list.get(id))
            .map(|entry| entry.track.clone())
            .collect();
        let count = tracks.len();
        if count == 0 || !self.insert_tracks(to, None, tracks) {
            return;
        }
        let state = &mut self.state;
        if let Some(list) = state.library.get_mut(from) {
            list.remove(&entries);
        }
        let name = match state.library.get(to) {
            Some(list) => list.name.clone(),
            None => "Spotify's queue".to_string(),
        };
        state.notice = Some(match count {
            1 => format!("Moved 1 song to {name}"),
            n => format!("Moved {n} songs to {name}"),
        });
        let requests = state.transport.edited(&mut state.library, from);
        self.carry_out(requests);
    }

    /// Copies songs to come of the account's queue to a playlist and takes
    /// them out of the queue, unless another device has it.
    fn move_from_spotify(&mut self, to: PlaylistId, rows: Vec<EntryId>) {
        let Some(queue) = &self.state.spotify else {
            return;
        };
        if !queue.movable(&rows) {
            return;
        }
        let tracks: Vec<TrackRef> = rows
            .iter()
            .filter_map(|&id| queue.row(id))
            .map(|row| row.track().clone())
            .collect();
        let count = tracks.len();
        if !self.insert_tracks(to, None, tracks) {
            return;
        }
        if let Some(list) = self.state.library.get(to) {
            let name = &list.name;
            self.state.notice = Some(match count {
                1 => format!("Moved 1 song to {name}"),
                n => format!("Moved {n} songs to {name}"),
            });
        }
        self.apply_spotify(Action::Remove(PlaylistId::SPOTIFY, rows));
    }
}
