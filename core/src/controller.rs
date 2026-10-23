//! The application's state and what changes it, whatever draws it. The
//! interface reads [`State`], hands its [`Action`]s to
//! [`Controller::apply`], and calls [`Controller::receive`] and
//! [`Controller::tick`] whenever the backend wakes it.
//!
//! What only a desktop has (the window, the tray, media keys, skins and
//! textures) stays with the interface, in `app.rs`.

use std::time::{Duration, Instant};

use crate::action::Action;
use crate::backend::{Backend, Command, Event, SearchState, SessionState, Storage};
use crate::browse::{Browse, Loadable, Page};
use crate::browser::{self, Browser};
use crate::lastfm::{LastFmState, LastFmStatus, api::Keys};
use crate::model::{ArtistRef, Library, Playback, PlaylistId, TrackRef};
use crate::paths::Paths;
use crate::playback::engine::{PlayerSnapshot, Status, TrackInfo};
use crate::playback::outputs::Output;
use crate::playback::transport::{Request, Transport};
use crate::store::{self, Settings, Tabs};

/// Changes are saved this long after the last one.
const SAVE_DELAY: Duration = Duration::from_secs(1);

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
    pub transport: Transport,
    /// The playlist shown in the editor.
    pub viewed: PlaylistId,
    pub settings: Settings,
    pub browse: Browse,
    pub search: SearchState,
    pub lastfm: LastFmState,
    /// What the caches hold, once measured.
    pub storage: Option<Storage>,
    /// The audio outputs, once listed; None when they can't be.
    pub outputs: Loadable<Option<Vec<Output>>>,
}

pub struct Controller {
    paths: Paths,
    backend: Option<Backend>,
    state: State,
    /// When the first unsaved change was made.
    unsaved: Option<Instant>,
    /// Albums whose songs go to a playlist (at an index, or at the end)
    /// once they are read.
    pending_adds: Vec<(String, PlaylistId, Option<usize>)>,
}

impl Controller {
    /// Loads the playlists and settings and starts the backend, which calls
    /// `wake` from its own thread whenever it has news. Approval pages open
    /// in the system's browser.
    pub fn new(paths: Paths, wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self::with_browser(paths, browser::system(), wake)
    }

    /// Like [`Self::new`], with `browser` opening the approval pages.
    pub fn with_browser(
        paths: Paths,
        browser: Browser,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let settings = Settings::load(&paths);
        let started = Backend::start(
            paths.clone(),
            volume_level(settings.volume),
            settings.levelling,
            browser,
            wake,
        );
        Self::with_backend(paths, settings, started)
    }

    fn with_backend(paths: Paths, settings: Settings, started: anyhow::Result<Backend>) -> Self {
        let (library, tabs) = store::load_library(&paths);
        let history = store::load_history(&paths);
        let viewed = tabs.viewed.or(tabs.playing).unwrap_or(PlaylistId::DEFAULT);
        let mut playback = Playback::new(seed());
        playback.repeat = settings.repeat;
        playback.set_shuffle(settings.shuffle);
        let (backend, session) = match started {
            Ok(backend) => (Some(backend), SessionState::Connecting),
            Err(error) => (None, SessionState::Failed(format!("{error:#}"))),
        };
        let mut browse = Browse::new(viewed, settings.covers);
        browse.recent_searches = history.searches;
        browse.recent = history.artists;
        Self {
            paths,
            backend,
            state: State {
                session,
                player: PlayerSnapshot::default(),
                offline: false,
                error: None,
                notice: None,
                library,
                transport: Transport::new(playback),
                viewed,
                browse,
                settings,
                search: SearchState::SignedOut,
                lastfm: LastFmState::default(),
                storage: None,
                outputs: Loadable::Loading,
            },
            unsaved: None,
            pending_adds: Vec::new(),
        }
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    /// The settings, for the ones only the interface knows about (the tray,
    /// the skin, the zoom). They are saved soon after.
    pub fn settings_mut(&mut self) -> &mut Settings {
        self.mark_unsaved();
        &mut self.state.settings
    }

    /// Takes in what the backend has sent since, and hands back the events
    /// that are the interface's to deal with: the covers.
    pub fn receive(&mut self) -> Vec<Event> {
        let Some(backend) = &self.backend else {
            return Vec::new();
        };
        let events: Vec<Event> = backend.events.try_iter().collect();
        let mut theirs = Vec::new();
        for event in events {
            match event {
                Event::Session(session) => {
                    self.state.error = None;
                    if !matches!(session, SessionState::Ready { .. }) {
                        // The engine is gone with the session.
                        self.state.transport.stop(&mut self.state.library);
                        self.state.player = PlayerSnapshot::default();
                    }
                    self.state.session = session;
                }
                // The engine holds nothing while a song waits offline.
                Event::Player(_) if self.state.offline => {}
                Event::Player(snapshot) => {
                    if snapshot.error.is_some() {
                        self.state.error = snapshot.error.clone();
                    }
                    self.state.player = snapshot;
                }
                Event::Followed(entry) => {
                    let requests = self
                        .state
                        .transport
                        .followed(&mut self.state.library, entry);
                    self.changed(requests);
                }
                Event::Ended => {
                    let requests = self.state.transport.ended(&mut self.state.library);
                    self.changed(requests);
                }
                // Offline, every song would fail in turn: wait on this one
                // instead of skipping through the playlist.
                Event::Unavailable { offline: true } => self.wait_offline(0),
                Event::Cut { position_ms } => self.wait_offline(position_ms),
                Event::Unavailable { offline: false } => {
                    let requests = self.state.transport.unavailable(&mut self.state.library);
                    self.changed(requests);
                }
                Event::Reconnected if self.state.offline => self.resume_offline(),
                Event::Reconnected => {}
                Event::Tracks { playlist, tracks } => self.insert_tracks(playlist, None, tracks),
                Event::Search(search) => self.state.search = search,
                Event::LastFm(lastfm) => {
                    if self.state.lastfm.status == LastFmStatus::SigningIn
                        && let LastFmStatus::Connected { user } = &lastfm.status
                    {
                        self.state.notice = Some(format!("Scrobbling to Last.fm as {user}."));
                    }
                    self.state.lastfm = lastfm;
                }
                Event::Storage(storage) => self.state.storage = Some(storage),
                Event::Outputs(outputs) => self.state.outputs = Loadable::Ready(outputs),
                Event::SearchResults { query, results } => {
                    let loaded = match results {
                        Ok(results) => Loadable::Ready(results),
                        Err(error) => Loadable::Failed(error),
                    };
                    self.state.browse.searches.insert(query, loaded);
                }
                Event::Discography { artist, result } => {
                    let loaded = match result {
                        Ok(discography) => {
                            self.state.browse.visited(discography.artist.clone());
                            self.mark_unsaved();
                            Loadable::Ready(discography)
                        }
                        Err(error) => Loadable::Failed(error),
                    };
                    self.state.browse.artists.insert(artist, loaded);
                }
                Event::SimilarArtists { artist, result } => {
                    let loaded = match result {
                        Ok(names) => Loadable::Ready(names),
                        Err(error) => Loadable::Failed(error),
                    };
                    self.state.browse.similar_artists.insert(artist, loaded);
                }
                Event::ArtistFound { name, result } => self.artist_found(name, result),
                Event::Album { album, result } => {
                    let pending: Vec<(PlaylistId, Option<usize>)> = self
                        .pending_adds
                        .iter()
                        .filter(|(uri, _, _)| *uri == album)
                        .map(|(_, list, at)| (*list, *at))
                        .collect();
                    self.pending_adds.retain(|(uri, _, _)| *uri != album);
                    match result {
                        Ok(tracks) => {
                            for (list, at) in pending {
                                self.insert_tracks(list, at, tracks.clone());
                            }
                            self.state
                                .browse
                                .albums
                                .insert(album, Loadable::Ready(tracks));
                        }
                        Err(error) => {
                            if !pending.is_empty() {
                                self.state.error = Some(error.clone());
                            }
                            self.state
                                .browse
                                .albums
                                .insert(album, Loadable::Failed(error));
                        }
                    }
                }
                Event::Error(message) => self.state.error = Some(message),
                Event::Cover { .. } => theirs.push(event),
            }
        }
        theirs
    }

    /// The playing song stopped at `position_ms`, or didn't load, for want
    /// of a connection: it waits there, shown paused, until reconnected or
    /// played.
    /// librespot may say so twice: the second finds it waiting already.
    fn wait_offline(&mut self, position_ms: u32) {
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
        self.changed(requests);
    }

    /// Plays the song that waited offline, from where it stopped.
    fn resume_offline(&mut self) {
        self.state.offline = false;
        let requests = self.state.transport.resume(&mut self.state.library);
        self.changed(requests);
    }

    /// Does what the interface asked for.
    pub fn apply(&mut self, action: Action) {
        if self.state.offline {
            match action {
                // Shown paused: playing goes on where it stopped.
                Action::Play | Action::TogglePause => return self.resume_offline(),
                Action::Stop
                | Action::Release
                | Action::Next
                | Action::Previous
                | Action::PlayEntry(..) => {
                    self.state.offline = false;
                    self.state.player = PlayerSnapshot::default();
                }
                _ => {}
            }
        }
        let paused = self.state.player.status == Status::Paused;
        let position = self.state.player.position_now();
        let state = &mut self.state;
        let library = &mut state.library;
        let viewed = state.viewed;
        let requests = match action {
            Action::SignIn => return self.send(Command::SignIn),
            Action::SignOut => return self.send(Command::SignOut),
            Action::SetLevelling(on) => {
                state.settings.levelling = on;
                self.mark_unsaved();
                return self.send(Command::Levelling(on));
            }
            Action::ConnectLastFm(api_key, secret) => {
                state.error = None;
                state.notice = Some("Allow Spotless in the Last.fm page that opens.".into());
                return self.send(Command::LastFmConnect(Keys {
                    api_key: api_key.trim().into(),
                    secret: secret.trim().into(),
                }));
            }
            Action::DisconnectLastFm => {
                state.notice = Some("Last.fm disconnected.".into());
                return self.send(Command::LastFmDisconnect);
            }
            Action::SetAudioCache(on) => {
                state.settings.audio_cache = on;
                return self.mark_unsaved();
            }
            Action::SetAudioCacheSize(mb) => {
                state.settings.audio_cache_mb = mb;
                return self.mark_unsaved();
            }
            Action::MeasureStorage => return self.send(Command::MeasureStorage),
            Action::NetworkBack => return self.send(Command::NetworkBack),
            Action::ListOutputs => return self.send(Command::ListOutputs),
            Action::SetOutput(output) => {
                state.settings.audio_output = output.clone();
                self.mark_unsaved();
                return self.send(Command::Output(output));
            }
            Action::ClearCache(store) => return self.send(Command::ClearCache(store)),
            Action::SetSearchApp(client_id) => {
                let client_id = Some(client_id.trim().to_string()).filter(|id| !id.is_empty());
                state.error = None;
                state.notice = Some(match (&client_id, &state.session) {
                    (None, _) => "Search's app removed.".into(),
                    (Some(_), SessionState::Ready { .. }) => {
                        "Saved. Approve search in the browser page that opens.".into()
                    }
                    (Some(_), _) => "Saved. Search is approved when you sign in.".into(),
                });
                state.settings.set_web_client_id(client_id.clone());
                self.mark_unsaved();
                return self.send(Command::SearchApp(client_id));
            }
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
            Action::ClosePlaylist(list) => {
                let index = library.playlists().iter().position(|l| l.id == list);
                let requests = state.transport.deleting(list);
                if !library.delete(list) {
                    return;
                }
                // The tab before it takes its place.
                let neighbour = index
                    .and_then(|i| library.playlists().get(i.saturating_sub(1)))
                    .map_or(PlaylistId::DEFAULT, |l| l.id);
                if state.viewed == list {
                    state.viewed = neighbour;
                }
                if state.browse.target == list {
                    state.browse.target = state.viewed;
                }
                self.pending_adds.retain(|(_, target, _)| *target != list);
                requests
            }
            Action::RenamePlaylist(list, name) => {
                library.rename(list, &name);
                return self.mark_unsaved();
            }
            Action::MovePlaylist(list, index) => {
                library.move_playlist(list, index);
                return self.mark_unsaved();
            }
            Action::Seek(ms) => return self.send(Command::Seek(ms)),
            Action::Volume(volume) => {
                state.settings.volume = volume;
                self.mark_unsaved();
                return self.send(Command::Volume(volume_level(volume)));
            }
            Action::Play => state.transport.play(library, viewed, paused, position),
            Action::TogglePause => state.transport.toggle_pause(paused),
            Action::Stop => state.transport.stop(library),
            Action::Release => state.transport.release(library, position),
            Action::Next => state.transport.next(library, viewed),
            Action::Previous => state.transport.prev(library, viewed, position),
            Action::SetShuffle(on) => {
                state.settings.shuffle = on;
                state.transport.set_shuffle(library, on)
            }
            Action::SetRepeat(on) => {
                state.settings.repeat = on;
                state.transport.set_repeat(library, on)
            }
            Action::PlayEntry(list, entry) => {
                state.error = None;
                state.transport.play_entry(library, list, entry, position)
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
            Action::PlayNow(track) => self.play_now(track, position),
            browse => return self.apply_browse(browse),
        };
        self.changed(requests);
    }

    fn apply_browse(&mut self, action: Action) {
        let browse = &mut self.state.browse;
        match action {
            Action::ToggleBrowse => {
                browse.open = !browse.open;
                if browse.open {
                    browse.target = self.state.viewed;
                }
            }
            Action::CloseBrowse => browse.open = false,
            Action::Back => browse.back(),
            Action::Search(query) => {
                let query = query.trim().to_string();
                if query.is_empty() {
                    return;
                }
                // A pasted Spotify link adds its songs instead.
                if crate::spotify::catalog::parse_uri(&query).is_ok() {
                    return self.apply(Action::AddLink(query));
                }
                browse.go(Page::Search(query.clone()));
                if self.state.search == SearchState::NoApp {
                    return;
                }
                browse.searched(&query);
                let cached = matches!(browse.searches.get(&query), Some(Loadable::Ready(_)));
                // Unapproved search asks for its approval first, by itself.
                if !cached {
                    browse.searches.insert(query.clone(), Loadable::Loading);
                    self.send(Command::Search(query));
                }
                self.mark_unsaved();
            }
            Action::ForgetSearch(query) => {
                match query {
                    Some(query) => browse.forget_search(&query),
                    None => browse.recent_searches.clear(),
                }
                self.mark_unsaved();
            }
            Action::OpenArtist(artist) => {
                browse.go(Page::Artist(artist.uri.clone()));
                match browse.artists.get(&artist.uri) {
                    Some(Loadable::Ready(_) | Loadable::Loading) => {
                        browse.visited(artist);
                        self.mark_unsaved();
                    }
                    _ => {
                        browse.artists.insert(artist.uri.clone(), Loadable::Loading);
                        self.send(Command::Discography(artist.uri));
                    }
                }
            }
            Action::ToggleAlbum(album) => {
                if !browse.expanded.remove(&album) {
                    browse.expanded.insert(album.clone());
                    self.read_album(album);
                }
            }
            Action::ExpandAlbums(albums) => {
                if albums.is_empty() {
                    browse.expanded.clear();
                }
                for album in albums {
                    self.state.browse.expanded.insert(album.clone());
                    self.read_album(album);
                }
            }
            Action::SetTab(tab) => {
                browse.tab = tab;
                browse.similar = false;
            }
            Action::ShowSimilar(artist) => {
                browse.similar = true;
                if matches!(
                    browse.similar_artists.get(&artist.uri),
                    Some(Loadable::Ready(_) | Loadable::Loading)
                ) {
                    return;
                }
                let api_key = self.state.lastfm.api_key.clone();
                if !crate::lastfm::api::is_key(&api_key) {
                    browse.similar_artists.insert(
                        artist.uri,
                        Loadable::Failed("Similar artists come from Last.fm: add its API key in Settings, Last.fm.".into()),
                    );
                    return;
                }
                browse
                    .similar_artists
                    .insert(artist.uri.clone(), Loadable::Loading);
                self.send(Command::SimilarArtists { artist, api_key });
            }
            Action::OpenSimilar(name) => {
                // Without search, the search page says how to set it up.
                if self.state.search == SearchState::NoApp {
                    return self.apply(Action::Search(name));
                }
                if browse.finding.as_ref().is_some_and(|(n, _)| *n == name) {
                    return;
                }
                browse.finding = Some((name.clone(), browse.page().clone()));
                self.send(Command::FindArtist(name));
            }
            Action::ToggleSort => browse.newest_first = !browse.newest_first,
            Action::SetTarget(list) => browse.target = list,
            Action::TogglePlaylists => {
                self.state.settings.playlists = !self.state.settings.playlists;
                self.mark_unsaved();
            }
            Action::ToggleCovers => {
                browse.show_covers = !browse.show_covers;
                self.state.settings.covers = browse.show_covers;
                self.mark_unsaved();
            }
            Action::AddTracks(tracks) => self.insert_tracks(self.state.browse.target, None, tracks),
            Action::DropTracks(tracks, at) => {
                self.insert_tracks(self.state.browse.target, Some(at), tracks)
            }
            Action::AddAlbum(album) => self.add_album(album, None),
            Action::DropAlbum(album, at) => self.add_album(album, Some(at)),
            _ => {}
        }
    }

    /// Opens the similar artist found, unless the user has gone elsewhere
    /// since. Spotify knowing no artist by that name searches for it.
    fn artist_found(&mut self, name: String, result: Result<Option<ArtistRef>, String>) {
        let browse = &mut self.state.browse;
        match browse.finding.take() {
            Some((wanted, from)) if wanted == name => {
                if *browse.page() != from {
                    return;
                }
            }
            other => {
                browse.finding = other;
                return;
            }
        }
        match result {
            Ok(Some(artist)) => self.apply(Action::OpenArtist(artist)),
            Ok(None) => self.apply(Action::Search(name)),
            Err(error) => self.state.error = Some(error),
        }
    }

    fn add_album(&mut self, album: String, at: Option<usize>) {
        let target = self.state.browse.target;
        match self.state.browse.albums.get(&album) {
            Some(Loadable::Ready(tracks)) => {
                let tracks = tracks.clone();
                self.insert_tracks(target, at, tracks);
            }
            _ => {
                self.pending_adds.push((album.clone(), target, at));
                self.state.notice = Some("Reading the album…".into());
                self.read_album(album);
            }
        }
    }

    /// Reads an album's songs unless they are read or being read.
    fn read_album(&mut self, album: String) {
        if matches!(
            self.state.browse.albums.get(&album),
            Some(Loadable::Ready(_) | Loadable::Loading)
        ) {
            return;
        }
        self.state
            .browse
            .albums
            .insert(album.clone(), Loadable::Loading);
        self.send(Command::Album(album));
    }

    /// Adds songs to a playlist before the row at `at`, or at the end.
    fn insert_tracks(&mut self, playlist: PlaylistId, at: Option<usize>, tracks: Vec<TrackRef>) {
        let state = &mut self.state;
        let entries: Vec<_> = tracks
            .into_iter()
            .map(|track| state.library.entry(track))
            .collect();
        let ids: Vec<_> = entries.iter().map(|entry| entry.id).collect();
        let Some(list) = state.library.get_mut(playlist) else {
            return;
        };
        list.insert(at.unwrap_or(usize::MAX), entries);
        state.notice = Some(match ids.len() {
            1 => format!("Added 1 song to {}", list.name),
            n => format!("Added {n} songs to {}", list.name),
        });
        state.browse.flash(ids);
        let requests = state.transport.edited(&mut state.library, playlist);
        self.changed(requests);
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

    /// Saves once changes have rested for a moment. Returns how long until
    /// the next save is due, while one is.
    pub fn tick(&mut self) -> Option<Duration> {
        let since = self.unsaved?;
        match SAVE_DELAY.checked_sub(since.elapsed()) {
            Some(wait) if !wait.is_zero() => Some(wait),
            _ => {
                self.save();
                None
            }
        }
    }

    /// The model changed: send what the engine must do, and save soon.
    fn changed(&mut self, requests: Vec<Request>) {
        for request in requests {
            self.send(Command::Player(request));
        }
        self.mark_unsaved();
    }

    fn mark_unsaved(&mut self) {
        self.unsaved.get_or_insert_with(Instant::now);
    }

    pub fn send(&self, command: Command) {
        if let Some(backend) = &self.backend {
            backend.send(command);
        }
    }

    pub fn save(&mut self) {
        self.unsaved = None;
        let state = &mut self.state;
        state
            .transport
            .remember_position(&mut state.library, state.player.position_now());
        let tabs = Tabs {
            playing: state.transport.playback.playing(),
            viewed: Some(state.viewed),
        };
        if let Err(error) = store::save_library(&self.paths, &state.library, tabs) {
            log::error!("{error:#}");
            state.error = Some("Couldn't save the playlists.".into());
        }
        if let Err(error) = state.settings.save(&self.paths) {
            log::error!("{error:#}");
        }
        let history = store::History {
            searches: state.browse.recent_searches.clone(),
            artists: state.browse.recent.clone(),
        };
        if let Err(error) = store::save_history(&self.paths, &history) {
            log::error!("{error:#}");
        }
    }
}

/// The slider's 0–100 as librespot's 0–65535.
fn volume_level(percent: u8) -> u16 {
    (u32::from(percent.min(100)) * u32::from(u16::MAX) / 100) as u16
}

/// A different shuffle every run.
fn seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use tokio::sync::mpsc::UnboundedReceiver;

    use super::*;
    use crate::model::playlist::tests::entry;

    /// A controller on empty folders, with a backend the test plays.
    struct Rig {
        controller: Controller,
        commands: UnboundedReceiver<Command>,
        events: mpsc::Sender<Event>,
        root: std::path::PathBuf,
    }

    impl Rig {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir()
                .join(format!("spotless-controller-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            let paths = Paths::from_dirs(root.join("config"), root.join("cache")).unwrap();
            let (backend, commands, events) = Backend::detached();
            let settings = Settings::load(&paths);
            let controller = Controller::with_backend(paths, settings, Ok(backend));
            Self {
                controller,
                commands,
                events,
                root,
            }
        }

        /// The backend tells this, and the controller takes it in.
        fn tell(&mut self, event: Event) {
            self.events.send(event).unwrap();
            self.controller.receive();
        }

        fn sent(&mut self) -> Vec<Command> {
            std::iter::from_fn(|| self.commands.try_recv().ok()).collect()
        }

        /// The titles of the viewed playlist's songs.
        fn songs(&self) -> Vec<String> {
            let state = self.controller.state();
            state
                .library
                .get(state.viewed)
                .unwrap()
                .entries()
                .iter()
                .map(|e| e.track.title.clone())
                .collect()
        }
    }

    impl Drop for Rig {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn tracks(ids: impl IntoIterator<Item = u64>) -> Vec<TrackRef> {
        ids.into_iter().map(|id| entry(id).track).collect()
    }

    #[test]
    fn volume_spans_librespots_range() {
        assert_eq!(volume_level(0), 0);
        assert_eq!(volume_level(100), u16::MAX);
        assert_eq!(volume_level(250), u16::MAX);
    }

    #[test]
    fn an_album_dropped_before_it_is_read_lands_where_it_was_dropped() {
        let mut rig = Rig::new("drop");
        rig.controller.apply(Action::AddTracks(tracks([1, 2])));
        rig.controller
            .apply(Action::DropAlbum("spotify:album:a".into(), 1));
        assert!(
            rig.sent()
                .iter()
                .any(|c| matches!(c, Command::Album(a) if a == "spotify:album:a"))
        );
        assert_eq!(rig.songs(), ["Song 1", "Song 2"]);
        rig.tell(Event::Album {
            album: "spotify:album:a".into(),
            result: Ok(tracks([7, 8])),
        });
        assert_eq!(rig.songs(), ["Song 1", "Song 7", "Song 8", "Song 2"]);
    }

    #[test]
    fn a_similar_artist_opens_once_found_unless_the_user_moved_on() {
        let mut rig = Rig::new("similar");
        let artist = |name: &str| ArtistRef {
            uri: format!("spotify:artist:{name}"),
            name: name.into(),
            image: None,
        };
        let page = |rig: &Rig| rig.controller.state().browse.page().clone();
        rig.controller.apply(Action::OpenArtist(artist("boc")));
        rig.controller.apply(Action::OpenSimilar("Tycho".into()));
        assert!(
            rig.sent()
                .iter()
                .any(|c| matches!(c, Command::FindArtist(n) if n == "Tycho"))
        );
        rig.tell(Event::ArtistFound {
            name: "Tycho".into(),
            result: Ok(Some(artist("tycho"))),
        });
        assert_eq!(page(&rig), Page::Artist("spotify:artist:tycho".into()));

        rig.controller.apply(Action::OpenSimilar("Bibio".into()));
        rig.controller.apply(Action::Back);
        rig.tell(Event::ArtistFound {
            name: "Bibio".into(),
            result: Ok(Some(artist("bibio"))),
        });
        assert_eq!(page(&rig), Page::Artist("spotify:artist:boc".into()));

        // Spotify has nobody by that name: search for it instead.
        rig.controller
            .apply(Action::OpenSimilar("Ulrich Schnauss".into()));
        rig.tell(Event::ArtistFound {
            name: "Ulrich Schnauss".into(),
            result: Ok(None),
        });
        assert_eq!(page(&rig), Page::Search("Ulrich Schnauss".into()));
    }

    #[test]
    fn closing_a_tab_shows_the_one_before_it() {
        let mut rig = Rig::new("close");
        rig.controller.apply(Action::NewPlaylist);
        let second = rig.controller.state().viewed;
        rig.controller.apply(Action::NewPlaylist);
        let third = rig.controller.state().viewed;
        assert_ne!(second, third);
        rig.controller.apply(Action::ClosePlaylist(third));
        assert_eq!(rig.controller.state().viewed, second);
        assert!(rig.controller.state().library.get(third).is_none());
    }

    #[test]
    fn losing_the_session_stops_the_queue() {
        let mut rig = Rig::new("session");
        rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
        rig.controller.apply(Action::AddTracks(tracks([1, 2])));
        rig.controller.apply(Action::Play);
        assert!(
            rig.sent()
                .iter()
                .any(|c| matches!(c, Command::Player(Request::Load { .. })))
        );
        assert_eq!(
            rig.controller.state().transport.playing_list(),
            Some(PlaylistId::DEFAULT)
        );
        rig.tell(Event::Session(SessionState::SignedOut));
        assert_eq!(rig.controller.state().transport.playing_list(), None);
        assert_eq!(rig.controller.state().player, PlayerSnapshot::default());
    }

    #[test]
    fn offline_waits_on_the_song_and_plays_it_once_reconnected() {
        let mut rig = Rig::new("offline");
        rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
        rig.controller.apply(Action::AddTracks(tracks([1, 2, 3])));
        rig.controller.apply(Action::Play);
        rig.sent();
        rig.tell(Event::Unavailable { offline: true });
        let sent = rig.sent();
        assert!(
            sent.iter()
                .any(|c| matches!(c, Command::Player(Request::Stop)))
        );
        assert!(
            !sent
                .iter()
                .any(|c| matches!(c, Command::Player(Request::Load { .. })))
        );
        rig.tell(Event::Reconnected);
        assert!(rig.sent().iter().any(|c| matches!(
            c,
            Command::Player(Request::Load { uri, position_ms: 0, .. }) if uri.ends_with(":1")
        )));
        // Only once.
        rig.tell(Event::Reconnected);
        assert!(rig.sent().is_empty());
    }

    #[test]
    fn a_song_cut_offline_goes_on_where_it_stopped_once_reconnected() {
        let mut rig = Rig::new("cut");
        rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
        rig.controller.apply(Action::AddTracks(tracks([1, 2, 3])));
        rig.controller.apply(Action::Play);
        rig.sent();
        rig.tell(Event::Cut {
            position_ms: 95_000,
        });
        assert!(
            rig.sent()
                .iter()
                .any(|c| matches!(c, Command::Player(Request::Stop)))
        );
        rig.tell(Event::Reconnected);
        assert!(rig.sent().iter().any(|c| matches!(
            c,
            Command::Player(Request::Load { uri, position_ms: 95_000, .. }) if uri.ends_with(":1")
        )));
    }

    #[test]
    fn a_song_waiting_offline_shows_paused_and_play_goes_on_from_there() {
        let mut rig = Rig::new("paused");
        rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
        rig.controller.apply(Action::AddTracks(tracks([1, 2])));
        rig.controller.apply(Action::Play);
        rig.tell(Event::Cut {
            position_ms: 95_000,
        });
        // librespot's second word of it, and the engine's own news of
        // stopping, don't undo it.
        rig.tell(Event::Cut {
            position_ms: 95_000,
        });
        rig.tell(Event::Player(PlayerSnapshot::default()));
        let state = rig.controller.state();
        assert!(state.offline);
        assert_eq!(state.error, None);
        assert_eq!(state.player.status, Status::Paused);
        assert_eq!(state.player.position_ms, 95_000);
        assert_eq!(state.player.track.as_ref().unwrap().title, "Song 1");
        rig.sent();
        rig.controller.apply(Action::TogglePause);
        assert!(!rig.controller.state().offline);
        assert!(rig.sent().iter().any(|c| matches!(
            c,
            Command::Player(Request::Load { uri, position_ms: 95_000, .. }) if uri.ends_with(":1")
        )));
        rig.tell(Event::Reconnected);
        assert!(rig.sent().is_empty());
    }

    #[test]
    fn a_stop_while_offline_stays_stopped_when_reconnected() {
        let mut rig = Rig::new("offline-stop");
        rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
        rig.controller.apply(Action::AddTracks(tracks([1, 2])));
        rig.controller.apply(Action::Play);
        rig.tell(Event::Unavailable { offline: true });
        rig.controller.apply(Action::Stop);
        rig.sent();
        rig.tell(Event::Reconnected);
        assert!(rig.sent().is_empty());
    }

    #[test]
    fn searches_are_remembered_across_starts_but_links_are_not() {
        let mut rig = Rig::new("history");
        rig.controller.apply(Action::Search(" boards ".into()));
        rig.controller.apply(Action::Search("autechre".into()));
        rig.controller.apply(Action::Search(
            "spotify:track:4uLU6hMCjMI75M1A2tKUQC".into(),
        ));
        assert!(
            rig.sent()
                .iter()
                .any(|c| matches!(c, Command::Search(q) if q == "boards"))
        );
        rig.controller
            .apply(Action::ForgetSearch(Some("autechre".into())));
        rig.controller.apply(Action::Search("aphex".into()));
        rig.controller.save();
        let history = store::load_history(&rig.controller.paths);
        assert_eq!(history.searches, ["aphex", "boards"]);
        let (backend, _, _) = Backend::detached();
        let paths = rig.controller.paths.clone();
        let settings = Settings::load(&paths);
        let again = Controller::with_backend(paths, settings, Ok(backend));
        assert_eq!(again.state().browse.recent_searches, ["aphex", "boards"]);
    }

    #[test]
    fn changes_are_saved_once_they_rest() {
        let mut rig = Rig::new("save");
        assert_eq!(rig.controller.tick(), None);
        rig.controller.apply(Action::AddTracks(tracks([1])));
        assert!(rig.controller.tick().is_some());
        rig.controller.unsaved = Some(Instant::now() - SAVE_DELAY);
        assert_eq!(rig.controller.tick(), None);
        let (library, _) = store::load_library(&rig.controller.paths);
        let saved = library.get(PlaylistId::DEFAULT).unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved.entries()[0].track.title, "Song 1");
    }
}
