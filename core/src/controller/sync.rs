//! The controller's part in synced playlists (see `model::sync`): when
//! each is sent, what comes back, and the Spotless folder's playlists
//! found on Spotify.
//!
//! A synced playlist's changes are sent once they rest, as they're saved,
//! a sync at a time per playlist. Its position is written when it isn't
//! playing, as the app is left, and else at most every ten minutes, so a
//! playlist sees a few writes a day.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::Controller;
use crate::backend::{Command, SessionState};
use crate::model::sync::{EPOCH, ItemId, Link, Position, merge};
use crate::model::{AlbumRef, Entry, EntryId, PlaylistId, TrackRef};
use crate::playback::engine::Status;
use crate::spotify::synced::{Changed, Done, Job};

/// While a synced playlist plays, its position is written at most this
/// often.
const POSITION_EVERY: Duration = Duration::from_secs(600);

/// A playlist made synced this recently may not be in the Spotless folder
/// as read yet.
const JUST_CREATED: Duration = Duration::from_secs(60);

/// How a synced playlist stands, for its tab's mark.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncStatus {
    UpToDate,
    /// Changes wait to be sent, as Spotless is offline or sending them.
    Waiting,
    /// Spotify refused them, for this reason.
    Failed(String),
}

/// What the controller keeps of each synced playlist while it runs.
#[derive(Debug, Default)]
pub(super) struct Syncing {
    /// A sync was sent and hasn't answered.
    in_flight: bool,
    /// Another is due once it does.
    again: bool,
    /// When its position was last written.
    written: Option<Instant>,
    /// When it was made synced.
    pub(super) created: Option<Instant>,
}

impl Controller {
    /// Sends the synced playlists' changes, and their positions when due.
    /// `leaving`: the app closes or goes to the background, so positions
    /// are written whatever plays.
    pub(super) fn sync_all(&mut self, leaving: bool) {
        for id in self.synced_ids() {
            self.sync_if_due(id, leaving, false);
        }
    }

    fn synced_ids(&self) -> Vec<PlaylistId> {
        self.state
            .library
            .playlists()
            .iter()
            .filter(|list| list.sync.is_some())
            .map(|list| list.id)
            .collect()
    }

    /// Sends playlist `id`'s changes, if it has any, or whatever it has
    /// with `force` (Spotify's side may have changed).
    fn sync_if_due(&mut self, id: PlaylistId, leaving: bool, force: bool) {
        let position = self.position_due(id, leaving);
        let Some(list) = self.state.library.get(id) else {
            return;
        };
        let Some(link) = &list.sync else {
            return;
        };
        // Still being made.
        if link.uri.is_empty() {
            return;
        }
        let songs_changed = list
            .entries()
            .iter()
            .map(|entry| entry.item.as_ref())
            .ne(link.base.iter().map(Some));
        let renamed = list.name != link.name;
        if !(force || songs_changed || renamed || position.is_some()) {
            return;
        }
        if songs_changed || renamed {
            self.state.sync.insert(id, SyncStatus::Waiting);
        }
        if !matches!(self.state.session, SessionState::Ready { .. }) {
            self.state.sync.insert(id, SyncStatus::Waiting);
            return;
        }
        let syncing = self.syncing.entry(id).or_default();
        if syncing.in_flight {
            syncing.again = true;
            return;
        }
        syncing.in_flight = true;
        if position.is_some() {
            syncing.written = Some(Instant::now());
        }
        let job = Job {
            uri: link.uri.clone(),
            base: link.base.clone(),
            local: list
                .entries()
                .iter()
                .map(|entry| (entry.id, entry.item.clone(), entry.track.uri.clone()))
                .collect(),
            rename: renamed.then(|| list.name.clone()),
            position,
        };
        self.send(Command::Sync { list: id, job });
    }

    /// Playlist `id`'s position, if it's to be written now.
    fn position_due(&self, id: PlaylistId, leaving: bool) -> Option<Position> {
        let list = self.state.library.get(id)?;
        let link = list.sync.as_ref()?;
        // Not before Spotify's has been read: until then it isn't where
        // the playlist is up to, but where an empty tab is.
        if link.revision.is_empty() {
            return None;
        }
        let position = list.position(saved_now())?;
        if link
            .position
            .as_ref()
            .is_some_and(|known| known.same_place(&position))
        {
            return None;
        }
        let playing = self.state.transport.playing_list() == Some(id)
            && self.state.player.status == Status::Playing;
        let lately = self
            .syncing
            .get(&id)
            .and_then(|syncing| syncing.written)
            .is_some_and(|at| at.elapsed() < POSITION_EVERY);
        (leaving || !playing || !lately).then_some(position)
    }

    /// Takes in a sync's answer: the songs as Spotify has them now, with
    /// this device's changes made since, and Spotify's position if newer.
    pub(super) fn synced(&mut self, id: PlaylistId, result: Result<Done, String>) {
        let syncing = self.syncing.entry(id).or_default();
        syncing.in_flight = false;
        let again = std::mem::take(&mut syncing.again);
        let done = match result {
            Ok(done) => done,
            Err(error) => {
                let status = if matches!(self.state.session, SessionState::Ready { .. }) {
                    SyncStatus::Failed(error)
                } else {
                    SyncStatus::Waiting
                };
                self.state.sync.insert(id, status);
                return;
            }
        };
        let state = &mut self.state;
        let playing_here = state.transport.playing_list() == Some(id);
        let library = &mut state.library;
        let Some(list) = library.get_mut(id) else {
            return;
        };
        if list.sync.is_none() {
            return;
        }

        // The songs sent get their IDs; what was sent is what this
        // device's changes since are told from.
        let given: Vec<(EntryId, ItemId)> = done
            .sent
            .iter()
            .filter_map(|(entry, item)| Some((*entry, item.clone()?)))
            .collect();
        list.set_items(&given);
        let base: Vec<ItemId> = done
            .sent
            .iter()
            .filter_map(|(_, item)| item.clone())
            .collect();
        let remote: Vec<ItemId> = done
            .remote
            .songs
            .iter()
            .map(|(item, _)| item.clone())
            .collect();
        let merged = merge(&base, &list.sync_rows(), &remote);
        let mut by_item: HashMap<ItemId, Entry> = list
            .entries()
            .iter()
            .filter_map(|entry| Some((entry.item.clone()?, entry.clone())))
            .collect();
        let by_id: HashMap<EntryId, Entry> = list
            .entries()
            .iter()
            .map(|entry| (entry.id, entry.clone()))
            .collect();
        let mut tracks: HashMap<ItemId, Option<TrackRef>> = done.tracks.into_iter().collect();
        let uris: HashMap<&ItemId, &str> = done
            .remote
            .songs
            .iter()
            .map(|(item, uri)| (item, uri.as_str()))
            .collect();
        let mut arrived: Vec<(ItemId, TrackRef)> = Vec::new();
        let slots: Vec<Result<Entry, ItemId>> = merged
            .iter()
            .filter_map(|slot| match slot {
                crate::model::sync::Slot::Item(item) => Some(match by_item.remove(item) {
                    Some(entry) => Ok(entry),
                    None => {
                        let track = tracks
                            .remove(item)
                            .flatten()
                            .unwrap_or_else(|| unavailable(uris.get(item).copied().unwrap_or("")));
                        arrived.push((item.clone(), track));
                        Err(item.clone())
                    }
                }),
                crate::model::sync::Slot::New(entry) => by_id.get(entry).cloned().map(Ok),
            })
            .collect();
        let mut new_entries: HashMap<ItemId, Entry> = arrived
            .into_iter()
            .map(|(item, track)| {
                let mut entry = library.entry(track);
                entry.item = Some(item.clone());
                (item, entry)
            })
            .collect();
        let entries: Vec<Entry> = slots
            .into_iter()
            .filter_map(|slot| match slot {
                Ok(entry) => Some(entry),
                Err(item) => new_entries.remove(&item),
            })
            .collect();
        let Some(list) = library.get_mut(id) else {
            return;
        };
        list.replace_entries(entries);

        // Its name: Spotify's, unless renamed here since it was sent.
        let link = list.sync.get_or_insert_with(Link::default);
        let renamed_here = list.name != link.name;
        if !renamed_here && !done.remote.name.is_empty() {
            list.name = done.remote.name.clone();
        }
        let link = list.sync.get_or_insert_with(Link::default);
        link.name = done.remote.name.clone();
        link.base = remote;
        link.revision = ItemId::from_bytes(&done.remote.revision).0;

        // The position: Spotify's, if newer, unless it plays here.
        if let Some(theirs) = done.position {
            let newer = link
                .position
                .as_ref()
                .is_none_or(|known| theirs.saved_at > known.saved_at);
            if newer {
                link.position = Some(theirs.clone());
                let ours = list.position(theirs.saved_at);
                if !playing_here && ours.is_none_or(|ours| !ours.same_place(&theirs)) {
                    list.set_position(&theirs);
                }
            }
        }

        let requests = state.transport.edited(&mut state.library, id);
        self.changed(requests);
        self.state.sync.insert(id, SyncStatus::UpToDate);
        self.sync_if_due(id, false, again);
    }

    /// Spotify told of a playlist's change: a synced one's is read, unless
    /// it's the revision Spotless has.
    pub(super) fn playlist_changed(&mut self, changed: Changed) {
        let revision = ItemId::from_bytes(&changed.revision).0;
        let Some(list) = self.state.library.synced(&changed.uri) else {
            return;
        };
        if list
            .sync
            .as_ref()
            .is_some_and(|link| link.revision != revision)
        {
            let id = list.id;
            self.sync_if_due(id, false, true);
        }
    }

    /// Signed in, or the account's list of playlists changed: reads the
    /// Spotless folder, for playlists new to this device or gone from it,
    /// then catches up with every synced playlist.
    pub(super) fn sync_on_connect(&mut self) {
        self.send(Command::SyncFolder);
    }

    /// Reconnected: as signed in, if any playlist is synced.
    pub(super) fn sync_on_reconnect(&mut self) {
        if !self.synced_ids().is_empty() {
            self.sync_on_connect();
        }
    }

    /// Takes in the Spotless folder's playlists: one gone from it was
    /// deleted on another device, and one this device never had opens.
    pub(super) fn sync_folder_read(&mut self, result: Result<Vec<(String, String)>, String>) {
        let folder = match result {
            Ok(folder) => folder,
            Err(error) => {
                log::warn!("unable to read the Spotless folder: {error}");
                return self.sync_caught_up();
            }
        };
        let in_folder: HashSet<&str> = folder.iter().map(|(uri, _)| uri.as_str()).collect();
        // Seen in the folder, one just made is there: gone from it later,
        // it was deleted.
        for list in self.state.library.playlists() {
            if let Some(link) = &list.sync
                && in_folder.contains(link.uri.as_str())
                && let Some(syncing) = self.syncing.get_mut(&list.id)
            {
                syncing.created = None;
            }
        }
        let gone: Vec<PlaylistId> = self
            .state
            .library
            .playlists()
            .iter()
            .filter(|list| {
                list.sync.as_ref().is_some_and(|link| {
                    !link.uri.is_empty() && !in_folder.contains(link.uri.as_str())
                })
            })
            .map(|list| list.id)
            .filter(|id| {
                self.syncing
                    .get(id)
                    .and_then(|syncing| syncing.created)
                    .is_none_or(|at| at.elapsed() > JUST_CREATED)
            })
            .collect();
        for id in gone {
            self.unsync(id);
            if let Some(list) = self.state.library.get(id) {
                self.state.notice = Some(format!(
                    "{} was taken off Spotify on another device; kept here, not synced.",
                    list.name
                ));
            }
        }
        // One being made here may be in the folder before it's known here.
        let making = (self.state.library.playlists().iter())
            .any(|list| list.sync.as_ref().is_some_and(|link| link.uri.is_empty()));
        for (uri, name) in folder.iter().filter(|_| !making) {
            let library = &self.state.library;
            if library.synced(uri).is_none() && !library.synced_seen.contains(uri) {
                self.open_synced(uri.clone(), name.clone());
            }
        }
        self.sync_caught_up();
    }

    /// Reads every synced playlist again, for what changed meanwhile.
    fn sync_caught_up(&mut self) {
        for id in self.synced_ids() {
            self.sync_if_due(id, false, true);
        }
    }

    /// Opens a playlist of the Spotless folder as a synced tab, last.
    pub(super) fn open_synced(&mut self, uri: String, name: String) {
        if let Some(list) = self.state.library.synced(&uri) {
            self.state.viewed = list.id;
            return;
        }
        let library = &mut self.state.library;
        let id = library.create(name.clone());
        if !library.synced_seen.contains(&uri) {
            library.synced_seen.push(uri.clone());
        }
        if let Some(list) = library.get_mut(id) {
            list.sync = Some(Link {
                uri,
                name,
                ..Link::default()
            });
        }
        self.state.sync.insert(id, SyncStatus::Waiting);
        self.state.viewed = id;
        self.mark_unsaved();
        self.sync_if_due(id, false, true);
    }

    /// Starts syncing playlist `id`, or stops.
    pub(super) fn set_sync(&mut self, id: PlaylistId, on: bool) {
        let Some(list) = self.state.library.get(id) else {
            return;
        };
        if on == list.sync.is_some() {
            return;
        }
        let signed_in = matches!(self.state.session, SessionState::Ready { .. });
        if !on {
            // Its Spotify playlist is deleted, so that must be possible now.
            if !signed_in {
                self.state.error = Some("Sign in to stop syncing a playlist.".into());
                return;
            }
            let name = list.name.clone();
            let uri = list.sync.as_ref().map(|link| link.uri.clone());
            self.unsync(id);
            match uri.filter(|uri| !uri.is_empty()) {
                Some(uri) => {
                    self.desynced.insert(uri.clone());
                    self.send(Command::UnlistSpotifyPlaylist {
                        playlist: uri,
                        name: name.clone(),
                        notice: format!(
                            "{name} isn't synced any more: it's gone from Spotify, and only here."
                        ),
                    });
                }
                // Not made on Spotify yet: nothing to delete there.
                None => {
                    self.state.notice = Some(format!("{name} isn't synced any more."));
                }
            }
            return;
        }
        if !signed_in {
            self.state.error = Some("Sign in to sync a playlist.".into());
            return;
        }
        let name = list.name.clone();
        if let Some(list) = self.state.library.get_mut(id) {
            // Made on Spotify first: the URI follows.
            list.sync = Some(Link::default());
        }
        self.state.sync.insert(id, SyncStatus::Waiting);
        self.mark_unsaved();
        self.send(Command::CreateSynced { list: id, name });
    }

    /// Playlist `id`'s Spotify playlist was made, or not.
    pub(super) fn sync_created(&mut self, id: PlaylistId, result: Result<String, String>) {
        let library = &mut self.state.library;
        let Some(list) = library.get_mut(id) else {
            return;
        };
        match result {
            // Sync was turned off meanwhile: it goes again.
            Ok(uri) if list.sync.is_none() => {
                let name = list.name.clone();
                self.desynced.insert(uri.clone());
                self.send(Command::UnlistSpotifyPlaylist {
                    playlist: uri,
                    notice: format!("{name} isn't synced any more."),
                    name,
                });
            }
            Ok(uri) => {
                list.sync = Some(Link {
                    uri: uri.clone(),
                    // Sent as a rename would be, should it change meanwhile.
                    name: list.name.clone(),
                    ..Link::default()
                });
                library.synced_seen.push(uri);
                self.syncing.entry(id).or_default().created = Some(Instant::now());
                self.state.notice = Some("Synced: it's the same on your other devices.".into());
                self.mark_unsaved();
                self.sync_if_due(id, false, true);
            }
            Err(error) => {
                list.sync = None;
                self.state.sync.remove(&id);
                self.state.error = Some(format!("Couldn't sync the playlist: {error}"));
            }
        }
    }

    /// Waits, at most `most`, for the syncs sent to answer: as the app
    /// closes, so that what [`Controller::save`] sent arrives.
    pub fn wait_for_syncs(&mut self, most: Duration) {
        let until = Instant::now() + most;
        while self.syncing.values().any(|syncing| syncing.in_flight) && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(50));
            self.receive();
        }
    }

    /// Playlist `id` stays here as it is, not synced.
    fn unsync(&mut self, id: PlaylistId) {
        if let Some(list) = self.state.library.get_mut(id) {
            list.sync = None;
            list.clear_items();
        }
        self.syncing.remove(&id);
        self.state.sync.remove(&id);
        self.mark_unsaved();
    }
}

/// Seconds since [`EPOCH`], as a position is saved at.
fn saved_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |t| t.as_secs())
        .saturating_sub(EPOCH)
}

/// A song Spotify doesn't know any more, kept so that it isn't taken out
/// of the other devices' lists.
fn unavailable(uri: &str) -> TrackRef {
    TrackRef {
        uri: uri.to_string(),
        title: "Unavailable song".into(),
        artists: Vec::new(),
        album: AlbumRef {
            uri: String::new(),
            name: String::new(),
        },
        duration_ms: 0,
        source: None,
    }
}
