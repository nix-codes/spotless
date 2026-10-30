//! The application's state and what changes it, whatever draws it. The
//! interface reads [`State`], hands its [`Action`]s to
//! [`Controller::apply`], and calls [`Controller::receive`] and
//! [`Controller::tick`] whenever the backend wakes it.
//!
//! What only a desktop has (the window, the tray, media keys, skins and
//! textures) stays with the interface, in `app.rs`.

use std::collections::HashMap;
use std::time::{Duration, Instant};

mod sync;

pub use sync::SyncStatus;

use crate::action::Action;
use crate::backend::{Backend, Command, Event, SearchState, SessionState, SpotifyRequest, Storage};
use crate::browse::{AddAll, Browse, Loadable, Page, Removal, Undo, Unlisted};
use crate::browser::{self, Browser};
use crate::lastfm::{LastFmState, LastFmStatus, Loved, LovedSong, api::Keys};
use crate::model::catalog::{Place, SpotifyPlaylist, SpotifyTarget};
use crate::model::playlist::from_source;
use crate::model::{ArtistRef, EntryId, Library, Playback, Playlist, PlaylistId, Source, TrackRef};
use crate::paths::Paths;
use crate::playback::engine::{PlayerSnapshot, Status, TrackInfo};
use crate::playback::equaliser::{self, Preset};
use crate::playback::outputs::Output;
use crate::playback::transport::{Request, Transport};
use crate::spotify::queue::{Row, SpotifyQueue};
use crate::store::{self, Settings, Tabs};

/// Changes are saved this long after the last one.
const SAVE_DELAY: Duration = Duration::from_secs(1);
/// How long after seeking the queue its shown position outlasts Spotify's
/// reports, which may still be of the position before.
const SEEK_SHOWN: Duration = Duration::from_millis(1_500);
/// How long the queue asked to play here shows loading before giving up.
const SPOTIFY_START_WAIT: Duration = Duration::from_secs(10);

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
        let track = match &self.player.track {
            // The engine's has the cover.
            Some(track) if track.uri == entry.track.uri => track.clone(),
            _ => TrackInfo::from(&entry.track),
        };
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
    /// Albums whose songs go to the end of a playlist together, in order,
    /// once every one is read.
    pending_batches: Vec<(Vec<String>, PlaylistId)>,
    /// Songs loved on Last.fm being found on Spotify, by the ID they were
    /// asked for with, and where they go once found.
    finding: Vec<(u64, Vec<LovedSong>, FoundFor)>,
    /// The ID the last were asked for with.
    last_find: u64,
    /// The song that waited offline is loading again: it stays shown until
    /// the engine has it (the new player starts out knowing no song).
    resuming: bool,
    /// When the queue was last seeked: its position is shown at once, and
    /// reports from before the seek reached Spotify are kept from undoing it.
    spotify_seeked: Option<Instant>,
    /// The synced playlists' syncs.
    syncing: HashMap<PlaylistId, sync::Syncing>,
}

/// What songs loved on Last.fm are found on Spotify for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FoundFor {
    /// A playlist, at an index or at the end.
    Insert(PlaylistId, Option<usize>),
    /// Playing the song, as PlayNow.
    Play,
}

impl FoundFor {
    fn goes_to(self, list: PlaylistId) -> bool {
        matches!(self, Self::Insert(to, _) if to == list)
    }
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
            settings.spotify_tab_shown(),
            browser,
            wake,
        );
        Self::with_backend(paths, settings, started)
    }

    /// Spotless as its screenshots show it ([`crate::demo`]): signed in,
    /// the made-up library with a song playing and the made-up artist
    /// ready to open in Browse, and nothing connected to Spotify. Its
    /// settings are those in `paths`; anything saved goes there too.
    pub fn demo(paths: Paths) -> Self {
        let settings = Settings::load(&paths);
        let mut controller = Self::with_backend(paths, settings, Err(anyhow::anyhow!("a demo")));
        let demo = crate::demo::Demo::new();
        let state = &mut controller.state;
        state.session = SessionState::Ready {
            user: crate::demo::USER.into(),
        };
        state.search = SearchState::Ready;
        state.settings.covers = true;
        state.browse.show_covers = true;
        state.library = demo.library;
        state.viewed = demo.playing;
        state.browse.artists.insert(
            crate::demo::ARTIST_URI.into(),
            Loadable::Ready(crate::demo::discography()),
        );
        for (uri, songs) in crate::demo::release_songs() {
            state.browse.albums.insert(uri, Loadable::Ready(songs));
        }
        state.browse.spotify_playlists = Some(Loadable::Ready(crate::demo::spotify_playlists()));
        // Late Trains, of the account's on Spotify, holds some of the
        // playing songs.
        let late_trains = crate::demo::spotify_playlists()
            .into_iter()
            .find(|p| p.name == "Late Trains");
        if let (Some(list), Some(late_trains)) = (state.library.get(demo.playing), late_trains) {
            let songs: Vec<TrackRef> = list
                .entries()
                .iter()
                .take(12)
                .map(|e| e.track.clone())
                .collect();
            let places = (0..songs.len()).map(Place::At).collect();
            let uri = late_trains.uri;
            state
                .browse
                .places
                .insert(uri.clone(), (Vec::new(), places));
            state.browse.albums.insert(uri, Loadable::Ready(songs));
        }
        // Started as a double-click would, with the engine's part told
        // by hand: there is no engine.
        let _ = state
            .transport
            .play_entry(&mut state.library, demo.playing, demo.now, 0);
        let track = state
            .library
            .get(demo.playing)
            .and_then(|list| list.get(demo.now))
            .map(|entry| TrackInfo::from(&entry.track));
        state.player = PlayerSnapshot {
            status: Status::Playing,
            track,
            position_ms: crate::demo::POSITION_MS,
            reported_at: None,
            error: None,
        };
        controller
    }

    fn with_backend(paths: Paths, settings: Settings, started: anyhow::Result<Backend>) -> Self {
        let (mut library, tabs) = store::load_library(&paths);
        let history = store::load_history(&paths);
        let viewed = tabs.viewed.or(tabs.playing).unwrap_or(PlaylistId::DEFAULT);
        let mut playback = Playback::new(seed());
        playback.repeat = settings.repeat;
        playback.set_shuffle(settings.shuffle);
        if let Some(list) = tabs.playing.and_then(|id| library.get_mut(id)) {
            playback.restore(list);
        }
        let (backend, session) = match started {
            Ok(backend) => (Some(backend), SessionState::Connecting),
            Err(error) => (None, SessionState::Failed(format!("{error:#}"))),
        };
        equaliser::set(settings.equaliser);
        let mut browse = Browse::new(viewed, settings.covers);
        browse.recent_searches = history.searches;
        browse.recent = history.artists;
        browse.shown = history.tab;
        browse.spotify_open = history.spotify_playlist;
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
                loved: Loved::default(),
                storage: None,
                outputs: Loadable::Loading,
                spotify: None,
                spotify_list: Playlist::new(PlaylistId::SPOTIFY, "Spotify"),
                spotify_at: tabs.spotify_at,
                spotify_starting: None,
                spotify_owns: false,
                sync: HashMap::new(),
            },
            unsaved: None,
            pending_adds: Vec::new(),
            pending_batches: Vec::new(),
            finding: Vec::new(),
            last_find: 0,
            resuming: false,
            spotify_seeked: None,
            syncing: HashMap::new(),
        }
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    /// Shows this in the notice line, for what only the interface does
    /// (copying a link).
    pub fn tell(&mut self, notice: impl Into<String>) {
        self.state.notice = Some(notice.into());
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
                    // The engine is gone with the session. Before a song
                    // is loaded (as at the start) there is nothing to stop,
                    // and where Play goes on from is kept.
                    if !matches!(session, SessionState::Ready { .. })
                        && self.state.transport.active()
                    {
                        self.state.transport.stop(&mut self.state.library);
                        self.state.player = PlayerSnapshot::default();
                    }
                    let ready = matches!(session, SessionState::Ready { .. });
                    self.state.session = session;
                    if ready && self.state.browse.open {
                        self.read_spotify_playlists();
                    }
                    if ready {
                        self.sync_on_connect();
                    }
                }
                // The engine holds nothing while a song waits offline.
                Event::Player(_) if self.state.offline => {}
                Event::Player(snapshot)
                    if self.resuming && snapshot.track.is_none() && snapshot.error.is_none() =>
                {
                    if snapshot.status == Status::Playing {
                        self.state.player.status = Status::Playing;
                    }
                }
                Event::Player(snapshot) => {
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
                Event::Spotify(mut queue) => {
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
                                    && (old.status != Status::Playing
                                        || old.device_id != new.device_id)
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
                Event::PlayingElsewhere(device) => {
                    self.state.notice = Some(format!("Paused: Spotify is playing on {device}."));
                }
                Event::PausedElsewhere(device) => {
                    self.state.notice = Some(format!("Paused Spotify on {device}."));
                }
                // Offline, every song would fail in turn: wait on this one
                // instead of skipping through the playlist.
                Event::Unavailable { offline: true } => self.wait_offline(0),
                Event::Cut { position_ms } => self.wait_offline(position_ms),
                Event::Unavailable { offline: false } => {
                    self.resuming = false;
                    let requests = self.state.transport.unavailable(&mut self.state.library);
                    self.changed(requests);
                }
                Event::Reconnected => {
                    if self.state.offline {
                        self.resume_offline();
                    }
                    self.sync_on_reconnect();
                }
                Event::Tracks { playlist, tracks } => {
                    self.insert_tracks(playlist, None, tracks);
                }
                Event::Search(search) => self.state.search = search,
                Event::LastFm(lastfm) => {
                    if self.state.lastfm.status == LastFmStatus::SigningIn
                        && let LastFmStatus::Connected { user } = &lastfm.status
                    {
                        self.state.notice = Some(format!("Scrobbling to Last.fm as {user}."));
                    }
                    self.state.lastfm = lastfm;
                }
                Event::Loved(loved) => self.state.loved = loved,
                Event::Storage(storage) => self.state.storage = Some(storage),
                Event::Outputs(outputs) => self.state.outputs = Loadable::Ready(outputs),
                Event::SearchResults { query, results } => {
                    let loaded = match results {
                        Ok(results) => Loadable::Ready(results),
                        Err(error) => Loadable::Failed(error),
                    };
                    self.state.browse.searches.insert(query, loaded);
                }
                Event::MoreSongs { query, result } => {
                    let Some(Loadable::Ready(results)) = self.state.browse.searches.get_mut(&query)
                    else {
                        continue;
                    };
                    results.reading_songs = false;
                    match result {
                        Ok(page) => {
                            // Spotify's pages can overlap: a song shows once.
                            for song in page.songs {
                                if !results.songs.iter().any(|s| s.uri == song.uri) {
                                    results.songs.push(song);
                                }
                            }
                            results.songs_read += page.read;
                            results.more_songs = page.more;
                        }
                        Err(error) => self.state.error = Some(error),
                    }
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
                    self.read_for_find();
                }
                Event::SimilarArtists { artist, result } => {
                    let loaded = match result {
                        Ok(names) => Loadable::Ready(names),
                        Err(error) => Loadable::Failed(error),
                    };
                    self.state.browse.similar_artists.insert(artist, loaded);
                }
                Event::ArtistFound { name, result } => self.artist_found(name, result),
                Event::SongsFound { id, result } => self.songs_found(id, result),
                Event::Album { album, result } => {
                    let pending: Vec<(PlaylistId, Option<usize>)> = self
                        .pending_adds
                        .iter()
                        .filter(|(uri, _, _)| *uri == album)
                        .map(|(_, list, at)| (*list, *at))
                        .collect();
                    self.pending_adds.retain(|(uri, _, _)| *uri != album);
                    match result {
                        Ok(songs) => {
                            for (list, at) in pending {
                                let tracks = self.added_from(&album, songs.tracks.clone());
                                self.insert_tracks(list, at, tracks);
                            }
                            let browse = &mut self.state.browse;
                            if songs.places.is_empty() {
                                browse.places.remove(&album);
                            } else {
                                browse
                                    .places
                                    .insert(album.clone(), (songs.revision, songs.places));
                            }
                            browse.albums.insert(album, Loadable::Ready(songs.tracks));
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
                    self.add_batches_read();
                }
                Event::SpotifyPlaylists(result) => self.spotify_playlists_read(result),
                Event::SpotifyEdited {
                    removal,
                    undo,
                    result,
                } => self.spotify_edited(removal, undo, result),
                Event::SpotifyUnlisted { result, undo } => self.spotify_unlisted(result, undo),
                Event::CopiedToSpotify {
                    target,
                    songs,
                    result,
                } => self.copied_to_spotify(target, songs, result),
                Event::Synced { list, result } => self.synced(list, result),
                Event::SyncCreated { list, result } => self.sync_created(list, result),
                Event::SyncFolder(result) => self.sync_folder_read(result),
                Event::PlaylistChanged(changed) => self.playlist_changed(changed),
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
        self.changed(requests);
    }

    /// Plays the song that waited offline, from where it stopped.
    fn resume_offline(&mut self) {
        self.state.offline = false;
        self.resuming = true;
        self.state.player.status = Status::Loading;
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
        if matches!(
            action,
            Action::Stop
                | Action::Release
                | Action::Next
                | Action::Previous
                | Action::PlayEntry(..)
        ) {
            self.resuming = false;
        }
        let Some(action) = self.apply_spotify(action) else {
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
        let state = &mut self.state;
        let library = &mut state.library;
        // With the Spotify tab shown, local playback goes on in the
        // playlist it played.
        let viewed = match state.viewed {
            PlaylistId::SPOTIFY => state
                .transport
                .playing_list()
                .unwrap_or(PlaylistId::DEFAULT),
            viewed => viewed,
        };
        let requests = match action {
            Action::SignIn => return self.send(Command::SignIn),
            Action::SignOut => return self.send(Command::SignOut),
            Action::SetPhosphor(phosphor) => {
                state.settings.phosphor = phosphor;
                return self.mark_unsaved();
            }
            Action::SetGrouping(grouping) => {
                state.settings.grouping = grouping;
                return self.mark_unsaved();
            }
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
            Action::SetLoved {
                artist,
                title,
                loved,
            } => {
                // Shown at once; Last.fm's refusal sets it back.
                state.loved.set(&artist, &title, loved);
                return self.send(Command::LastFmLove {
                    artist,
                    title,
                    loved,
                });
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
                    .map_or(PlaylistId::DEFAULT, |l| l.id);
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
            Action::SetSync(list, on) => return self.set_sync(list, on),
            Action::OpenSynced(uri, name) => {
                self.open_synced(uri, name);
                return;
            }
            Action::RenamePlaylist(list, name) => {
                library.rename(list, &name);
                return self.mark_unsaved();
            }
            Action::MovePlaylist(list, index) => {
                library.move_playlist(list, index);
                return self.mark_unsaved();
            }
            Action::Seek(ms) if waiting_paused => {
                if let Some(list) = library.get_mut(viewed) {
                    // At 0 it would show stopped.
                    list.resume_ms = ms.max(1);
                }
                return self.mark_unsaved();
            }
            Action::Seek(ms) => return self.send(Command::Seek(ms)),
            Action::Volume(volume) => {
                state.settings.volume = volume;
                self.mark_unsaved();
                return self.send(Command::Volume(volume_level(volume)));
            }
            Action::Play => state.transport.play(library, viewed, paused, position),
            Action::TogglePause if waiting_paused => {
                state.transport.play(library, viewed, false, position)
            }
            Action::TogglePause => state.transport.toggle_pause(paused),
            Action::Stop => state.transport.stop(library),
            Action::ToggleStopAfter => state.transport.toggle_stop_after(library),
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

    /// Joins or leaves Spotify Connect as the Spotify tab now shows or not.
    /// Gone, it was never started: the player and the tab shown move on.
    fn spotify_tab_changed(&mut self) {
        let state = &mut self.state;
        let shown = state.settings.spotify_tab_shown();
        if !shown {
            state.spotify_starting = None;
            state.spotify_owns = false;
            if state.viewed == PlaylistId::SPOTIFY {
                state.viewed = state
                    .library
                    .playlists()
                    .first()
                    .map_or(PlaylistId::DEFAULT, |list| list.id);
            }
        }
        self.mark_unsaved();
        self.send(Command::SpotifyTab(shown));
    }

    /// Does what the Spotify tab, or the player while the tab plays, asked
    /// for. Hands back any other action.
    fn apply_spotify(&mut self, action: Action) -> Option<Action> {
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
            Action::CopyTo(list, tracks) => {
                self.insert_tracks(list, None, tracks);
                return None;
            }
            Action::MoveTo(from, to, entries) => {
                self.move_tracks(from, to, entries);
                return None;
            }
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

    fn apply_browse(&mut self, action: Action) {
        let browse = &mut self.state.browse;
        match action {
            Action::ToggleBrowse => {
                browse.open = !browse.open;
                if browse.open {
                    browse.target = self.state.viewed;
                    self.read_spotify_playlists();
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
            Action::MoreSongs(query) => {
                if let Some(Loadable::Ready(results)) = browse.searches.get_mut(&query)
                    && results.more_songs
                    && !results.reading_songs
                {
                    results.reading_songs = true;
                    let offset = results.songs_read;
                    self.send(Command::MoreSongs { query, offset });
                }
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
                browse.popular = false;
                self.read_for_find();
            }
            Action::ShowPopular => {
                browse.popular = true;
                browse.similar = false;
            }
            Action::Find(find) => {
                browse.find = find;
                self.read_for_find();
            }
            Action::ShowSimilar(artist) => {
                browse.similar = true;
                browse.popular = false;
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
            Action::ToggleEqualiser => {
                self.state.settings.equaliser_shown = !self.state.settings.equaliser_shown;
                self.mark_unsaved();
            }
            Action::ToggleDeck => {
                self.state.settings.deck_shown = !self.state.settings.deck_shown;
                self.mark_unsaved();
            }
            Action::SetDeckMedia(media) => {
                self.state.settings.deck_media = media;
                self.mark_unsaved();
            }
            Action::SetCassetteMix(mix) => {
                self.state.settings.cassette_mix = mix;
                self.mark_unsaved();
            }
            Action::SetVinylLp(lp) => {
                self.state.settings.vinyl_lp = lp;
                self.mark_unsaved();
            }
            Action::ToggleTimeLeft => {
                self.state.settings.time_left = !self.state.settings.time_left;
                self.mark_unsaved();
            }
            Action::SetEqualiser(equaliser) => {
                let equaliser = equaliser.clamped();
                self.state.settings.equaliser = equaliser;
                equaliser::set(equaliser);
                self.mark_unsaved();
            }
            Action::SaveEqualiserPreset(name) => {
                let name = name.trim().to_string();
                if !name.is_empty() {
                    let presets = &mut self.state.settings.equaliser_presets;
                    let bands = self.state.settings.equaliser.bands;
                    match presets.iter_mut().find(|preset| preset.name == name) {
                        Some(preset) => preset.bands = bands,
                        None => presets.push(Preset { name, bands }),
                    }
                    self.mark_unsaved();
                }
            }
            Action::DeleteEqualiserPreset(name) => {
                self.state
                    .settings
                    .equaliser_presets
                    .retain(|preset| preset.name != name);
                self.mark_unsaved();
            }
            Action::ToggleCovers => {
                browse.show_covers = !browse.show_covers;
                self.state.settings.covers = browse.show_covers;
                self.mark_unsaved();
            }
            Action::AddTracks(tracks) => {
                self.insert_tracks(self.state.browse.target, None, tracks);
            }
            Action::DropTracks(tracks, at) => {
                self.insert_tracks(self.state.browse.target, Some(at), tracks);
            }
            Action::AddLoved(songs) => {
                let target = self.state.browse.target;
                self.find_loved(songs, FoundFor::Insert(target, None));
            }
            Action::DropLoved(songs, at) => {
                let target = self.state.browse.target;
                self.find_loved(songs, FoundFor::Insert(target, Some(at)));
            }
            Action::PlayLoved(song) => self.find_loved(vec![song], FoundFor::Play),
            Action::AddAlbum(album) => self.add_album(album, None),
            Action::AddAll => match self.state.browse.add_all() {
                Some(AddAll::Releases(albums)) => self.apply(Action::AddAlbums(albums)),
                Some(AddAll::Songs(songs)) => self.apply(Action::AddTracks(songs)),
                Some(AddAll::Reading) | None => {}
            },
            Action::AddAlbums(albums) => {
                let target = self.state.browse.target;
                for album in &albums {
                    self.read_album(album.clone());
                }
                self.pending_batches.push((albums, target));
                self.add_batches_read();
            }
            Action::DropAlbum(album, at) => self.add_album(album, Some(at)),
            Action::ReadSpotifyPlaylists => self.read_spotify_playlists(),
            Action::ShowBrowseTab(tab) => {
                let browse = &mut self.state.browse;
                browse.shown = tab;
                if !browse.open {
                    browse.open = true;
                    browse.target = self.state.viewed;
                    self.read_spotify_playlists();
                }
                self.mark_unsaved();
            }
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

    fn spotify_edited(&mut self, removal: Removal, undo: bool, result: Result<(), String>) {
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

    fn spotify_unlisted(&mut self, result: Result<Unlisted, String>, undo: bool) {
        match result {
            Ok(unlisted) if undo => {
                self.state.notice = Some(format!("Put “{}” back.", unlisted.name));
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

    fn copied_to_spotify(
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

    /// Asks for loved songs on Spotify, for `then` once found.
    fn find_loved(&mut self, songs: Vec<LovedSong>, then: FoundFor) {
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
    fn songs_found(&mut self, id: u64, result: Result<Vec<Option<TrackRef>>, String>) {
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
                    self.apply(Action::PlayNow(track));
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

    fn add_album(&mut self, album: String, at: Option<usize>) {
        let target = self.state.browse.target;
        match self.state.browse.albums.get(&album) {
            Some(Loadable::Ready(tracks)) => {
                let tracks = self.added_from(&album, tracks.clone());
                self.insert_tracks(target, at, tracks);
            }
            _ => {
                self.pending_adds.push((album.clone(), target, at));
                self.state.notice = Some("Reading the songs…".into());
                self.read_album(album);
            }
        }
    }

    /// Adds the batches of albums whose songs are all read now (those that
    /// couldn't be, left out), each in its order.
    fn add_batches_read(&mut self) {
        let albums = &self.state.browse.albums;
        let (read, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending_batches)
            .into_iter()
            .partition(|(batch, _)| {
                batch
                    .iter()
                    .all(|album| !matches!(albums.get(album), None | Some(Loadable::Loading)))
            });
        self.pending_batches = waiting;
        for (batch, list) in read {
            let mut tracks = Vec::new();
            let mut unread = 0;
            for album in batch {
                match self.state.browse.albums.get(&album) {
                    Some(Loadable::Ready(songs)) => {
                        tracks.extend(self.added_from(&album, songs.clone()));
                    }
                    _ => unread += 1,
                }
            }
            if !tracks.is_empty() {
                self.insert_tracks(list, None, tracks);
            }
            if unread > 0 {
                self.state.error = Some(match unread {
                    1 => "One of the releases couldn't be read.".into(),
                    n => format!("{n} of the releases couldn't be read."),
                });
            }
        }
        if !self.pending_batches.is_empty() {
            self.state.notice = Some("Reading the songs…".into());
        }
    }

    /// An album's or a Spotify playlist's songs, as added from it.
    fn added_from(&self, uri: &str, tracks: Vec<TrackRef>) -> Vec<TrackRef> {
        let name = match self.state.browse.spotify_playlist(uri) {
            Some(playlist) => playlist.name.clone(),
            None => match tracks.first() {
                Some(track) if track.album.uri == uri => track.album.name.clone(),
                _ => return tracks,
            },
        };
        let source = Source {
            uri: uri.to_string(),
            name,
        };
        from_source(tracks, &source)
    }

    /// Reads the account's Spotify playlists again, as they may have
    /// changed on Spotify, still showing those read before meanwhile.
    fn read_spotify_playlists(&mut self) {
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
    fn spotify_playlists_read(&mut self, result: Result<Vec<SpotifyPlaylist>, String>) {
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

    /// Reads an album's songs unless they are read or being read.
    /// With the find open on an artist's page, its tab's releases' songs
    /// are read, for the find to look through.
    fn read_for_find(&mut self) {
        let browse = &self.state.browse;
        let Page::Artist(uri) = browse.page() else {
            return;
        };
        let Some(Loadable::Ready(discography)) = browse.artists.get(uri) else {
            return;
        };
        // Popular songs are found among themselves.
        if browse.find.is_none() || browse.popular {
            return;
        }
        let albums: Vec<String> = discography
            .releases
            .iter()
            .filter(|release| browse.tab.includes(release.group))
            .map(|release| release.uri.clone())
            .collect();
        for album in albums {
            self.read_album(album);
        }
    }

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

    /// Adds songs to a playlist before the row at `at`, or at the end. The
    /// Spotify tab's go to the end of the queue.
    fn insert_tracks(
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
        self.changed(requests);
        true
    }

    /// Takes songs out of one playlist and adds them to the end of another,
    /// once that one has them.
    fn move_tracks(&mut self, from: PlaylistId, to: PlaylistId, entries: Vec<EntryId>) {
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
        self.changed(requests);
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

    /// Adds songs to the end of the account's queue, while it takes them.
    fn queue_tracks(&mut self, tracks: Vec<TrackRef>) -> bool {
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

    /// Saves once changes have rested for a moment, and gives up waiting
    /// for the queue to come here. Returns how long until the next of
    /// those is due, while one is.
    pub fn tick(&mut self) -> Option<Duration> {
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
        let starting = starting.filter(|wait| !wait.is_zero());
        let saving = self.save_tick();
        match (starting, saving) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    fn save_tick(&mut self) -> Option<Duration> {
        let since = self.unsaved?;
        match SAVE_DELAY.checked_sub(since.elapsed()) {
            Some(wait) if !wait.is_zero() => Some(wait),
            _ => {
                self.store();
                self.sync_all(false);
                None
            }
        }
    }

    /// The model changed: send what the engine must do, and save soon.
    fn changed(&mut self, requests: Vec<Request>) {
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

    fn mark_unsaved(&mut self) {
        self.unsaved.get_or_insert_with(Instant::now);
    }

    pub fn send(&self, command: Command) {
        if let Some(backend) = &self.backend {
            backend.send(command);
        }
    }

    /// Saves now, and sends the synced playlists' positions, as the app
    /// closes or goes to the background.
    pub fn save(&mut self) {
        self.store();
        self.sync_all(true);
    }

    fn store(&mut self) {
        self.unsaved = None;
        let state = &mut self.state;
        state
            .transport
            .remember_position(&mut state.library, state.player.position_now());
        let tabs = Tabs {
            playing: state.transport.playback.playing(),
            viewed: Some(state.viewed),
            spotify_at: state.spotify_at,
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
            tab: state.browse.shown,
            spotify_playlist: state.browse.spotify_open.clone(),
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

/// "1 song", "3 songs".
fn count_songs(count: usize) -> String {
    match count {
        1 => "1 song".to_string(),
        n => format!("{n} songs"),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use tokio::sync::mpsc::UnboundedReceiver;

    use super::*;
    use crate::model::catalog::Songs;
    use crate::model::playlist::tests::entry;
    use crate::model::{AlbumRef, Grouping};
    use crate::playback::equaliser::Equaliser;

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

    /// An album's songs, as read.
    fn songs(tracks: Vec<TrackRef>) -> Songs {
        Songs {
            tracks,
            ..Songs::default()
        }
    }

    #[test]
    fn volume_spans_librespots_range() {
        assert_eq!(volume_level(0), 0);
        assert_eq!(volume_level(100), u16::MAX);
        assert_eq!(volume_level(250), u16::MAX);
    }

    #[test]
    fn equaliser_presets_are_saved_by_name_and_deleted() {
        let mut rig = Rig::new("equaliser");
        let mut bands = [0.0; 10];
        bands[0] = 3.0;
        rig.controller.apply(Action::SetEqualiser(Equaliser {
            on: true,
            preamp: 40.0,
            bands,
        }));
        let settings = |rig: &Rig| rig.controller.state().settings.clone();
        assert_eq!(settings(&rig).equaliser.preamp, 12.0);
        rig.controller
            .apply(Action::SaveEqualiserPreset(" Mine ".into()));
        rig.controller.apply(Action::SaveEqualiserPreset("".into()));
        bands[0] = -3.0;
        rig.controller.apply(Action::SetEqualiser(Equaliser {
            on: true,
            preamp: 0.0,
            bands,
        }));
        rig.controller
            .apply(Action::SaveEqualiserPreset("Mine".into()));
        let presets = settings(&rig).equaliser_presets;
        assert_eq!(presets.len(), 1);
        assert_eq!(
            (presets[0].name.as_str(), presets[0].bands[0]),
            ("Mine", -3.0)
        );
        rig.controller
            .apply(Action::DeleteEqualiserPreset("Mine".into()));
        assert!(settings(&rig).equaliser_presets.is_empty());
        rig.controller.apply(Action::ToggleEqualiser);
        assert!(settings(&rig).equaliser_shown);
    }

    #[test]
    fn the_deck_shows_and_changes_media() {
        let mut rig = Rig::new("deck");
        let settings = |rig: &Rig| rig.controller.state().settings.clone();
        rig.controller.apply(Action::ToggleDeck);
        assert!(settings(&rig).deck_shown);
        rig.controller
            .apply(Action::SetDeckMedia(crate::store::DeckMedia::Reel));
        assert_eq!(settings(&rig).deck_media, crate::store::DeckMedia::Reel);
        rig.controller.apply(Action::SetCassetteMix(true));
        assert!(settings(&rig).cassette_mix);
        rig.controller.apply(Action::SetVinylLp(true));
        assert!(settings(&rig).vinyl_lp);
        rig.controller.apply(Action::ToggleDeck);
        assert!(!settings(&rig).deck_shown);
    }

    #[test]
    fn the_time_shows_what_is_left_and_back() {
        let mut rig = Rig::new("time-left");
        let time_left = |rig: &Rig| rig.controller.state().settings.time_left;
        assert!(!time_left(&rig), "what has played to start with");
        rig.controller.apply(Action::ToggleTimeLeft);
        assert!(time_left(&rig));
        rig.controller.apply(Action::ToggleTimeLeft);
        assert!(!time_left(&rig));
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
            result: Ok(songs(tracks([7, 8]))),
        });
        assert_eq!(rig.songs(), ["Song 1", "Song 7", "Song 8", "Song 2"]);
    }

    #[test]
    fn songs_added_from_an_album_or_a_spotify_playlist_keep_where_from() {
        let mut rig = Rig::new("sources");
        rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
        rig.tell(Event::SpotifyPlaylists(Ok(vec![SpotifyPlaylist {
            uri: "spotify:playlist:late".into(),
            name: "Late Trains".into(),
            tracks: 2,
            owner: None,
            private: false,
        }])));
        let on_album = |ids: [u64; 2]| {
            let mut tracks = tracks(ids);
            for track in &mut tracks {
                track.album = AlbumRef {
                    uri: "spotify:album:a".into(),
                    name: "Afterhours".into(),
                };
            }
            tracks
        };
        for (uri, read) in [
            ("spotify:album:a", on_album([1, 2])),
            ("spotify:playlist:late", tracks([3, 4])),
        ] {
            rig.controller.apply(Action::AddAlbum(uri.into()));
            rig.tell(Event::Album {
                album: uri.into(),
                result: Ok(songs(read)),
            });
        }
        let state = rig.controller.state();
        let sources: Vec<_> = state
            .library
            .get(state.viewed)
            .unwrap()
            .entries()
            .iter()
            .map(|e| {
                e.track
                    .source
                    .as_ref()
                    .map(|s| (s.uri.as_str(), s.name.as_str()))
            })
            .collect();
        let album = Some(("spotify:album:a", "Afterhours"));
        let late = Some(("spotify:playlist:late", "Late Trains"));
        assert_eq!(sources, [album, album, late, late]);
        // Kept when copied to another playlist.
        let copied: Vec<_> = state.library.get(state.viewed).unwrap().entries()[2..]
            .iter()
            .map(|e| e.track.clone())
            .collect();
        rig.controller.apply(Action::NewPlaylistWith(copied));
        let state = rig.controller.state();
        let list = state.library.get(state.viewed).unwrap();
        assert!(
            list.entries()
                .iter()
                .all(|e| e.track.source.as_ref().map(|s| s.name.as_str()) == Some("Late Trains"))
        );
        rig.controller.apply(Action::SetGrouping(Grouping::None));
        assert_eq!(rig.controller.state().settings.grouping, Grouping::None);
    }

    #[test]
    fn spotify_playlists_are_read_again_whenever_browse_opens() {
        let mut rig = Rig::new("spotify-playlists");
        let asked = |rig: &mut Rig| {
            rig.sent()
                .iter()
                .filter(|c| matches!(c, Command::SpotifyPlaylists))
                .count()
        };
        rig.controller.apply(Action::ToggleBrowse);
        assert_eq!(asked(&mut rig), 0, "not before signing in");
        rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
        assert_eq!(asked(&mut rig), 1);
        let playlist = |uri: &str, tracks| SpotifyPlaylist {
            uri: uri.into(),
            name: uri.into(),
            tracks,
            owner: None,
            private: false,
        };
        // Songs read before: one playlist shown, one not.
        for uri in ["spotify:playlist:shown", "spotify:playlist:hidden"] {
            rig.controller.apply(Action::ToggleAlbum(uri.into()));
            rig.tell(Event::Album {
                album: uri.into(),
                result: Ok(songs(tracks([1, 2]))),
            });
        }
        rig.controller
            .apply(Action::ToggleAlbum("spotify:playlist:hidden".into()));
        rig.tell(Event::SpotifyPlaylists(Ok(vec![
            playlist("spotify:playlist:shown", 2),
            playlist("spotify:playlist:hidden", 2),
        ])));
        let browse = &rig.controller.state().browse;
        assert!(matches!(&browse.spotify_playlists, Some(Loadable::Ready(p)) if p.len() == 2));
        assert!(browse.albums.contains_key("spotify:playlist:shown"));
        assert!(!browse.albums.contains_key("spotify:playlist:hidden"));

        rig.controller.apply(Action::ToggleBrowse);
        rig.controller.apply(Action::ToggleBrowse);
        assert_eq!(asked(&mut rig), 1);
        // A song added on Spotify: the songs shown are read again.
        rig.tell(Event::SpotifyPlaylists(Ok(vec![playlist(
            "spotify:playlist:shown",
            3,
        )])));
        assert!(
            rig.sent()
                .iter()
                .any(|c| matches!(c, Command::Album(a) if a == "spotify:playlist:shown"))
        );
        // A failed read keeps those read before.
        rig.controller.apply(Action::ToggleBrowse);
        rig.controller.apply(Action::ToggleBrowse);
        rig.tell(Event::SpotifyPlaylists(Err("offline".into())));
        let browse = &rig.controller.state().browse;
        assert!(matches!(&browse.spotify_playlists, Some(Loadable::Ready(p)) if p.len() == 1));
    }

    #[test]
    fn songs_removed_from_a_spotify_playlist_go_by_their_places_and_come_back() {
        let mut rig = Rig::new("remove-from-spotify");
        rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
        let playlist = |uri: &str, owner: Option<&str>| SpotifyPlaylist {
            uri: uri.into(),
            name: "Mine".into(),
            tracks: 4,
            owner: owner.map(String::from),
            private: false,
        };
        rig.tell(Event::SpotifyPlaylists(Ok(vec![
            playlist("spotify:playlist:mine", None),
            playlist("spotify:playlist:theirs", Some("Them")),
        ])));
        let uri = "spotify:playlist:mine".to_string();
        rig.controller
            .apply(Action::OpenSpotifyPlaylist(uri.clone()));
        assert_eq!(
            rig.controller.state().browse.spotify_open.as_ref(),
            Some(&uri)
        );
        // An episode at 1 and a song gone at 4 aren't shown.
        let places = [0, 2, 3, 5].map(Place::At).to_vec();
        rig.tell(Event::Album {
            album: uri.clone(),
            result: Ok(Songs {
                tracks: tracks([1, 2, 3, 4]),
                places,
                revision: vec![7],
            }),
        });
        rig.sent();
        rig.controller
            .apply(Action::RemoveFromSpotify(uri.clone(), vec![3, 1]));
        let shown = |rig: &Rig| match rig.controller.state().browse.albums.get(&uri) {
            Some(Loadable::Ready(tracks)) => tracks.iter().map(|t| t.title.clone()).collect(),
            _ => Vec::new(),
        };
        assert_eq!(shown(&rig), ["Song 1", "Song 3"]);
        let removal = match rig.sent().pop() {
            Some(Command::RemoveFromSpotify { removal, revision }) => {
                assert_eq!(revision, [7]);
                removal
            }
            other => panic!("sent {other:?}"),
        };
        let places: Vec<Place> = removal.songs.iter().map(|(place, _)| *place).collect();
        assert_eq!(places, [Place::At(2), Place::At(5)]);
        rig.tell(Event::SpotifyEdited {
            removal: removal.clone(),
            undo: false,
            result: Ok(()),
        });
        assert_eq!(
            rig.controller.state().notice.as_deref(),
            Some("Removed 2 songs from “Mine”.")
        );
        assert!(
            rig.sent()
                .iter()
                .any(|c| matches!(c, Command::Album(a) if *a == uri))
        );
        rig.controller.apply(Action::UndoSpotifyRemoval);
        assert!(
            rig.sent()
                .iter()
                .any(|c| matches!(c, Command::PutBackOnSpotify(r) if *r == removal))
        );
        assert!(rig.controller.state().browse.undo.is_none());

        // Others' playlists can't be changed.
        let theirs = "spotify:playlist:theirs".to_string();
        rig.tell(Event::Album {
            album: theirs.clone(),
            result: Ok(Songs {
                tracks: tracks([1]),
                places: vec![Place::At(0)],
                revision: vec![1],
            }),
        });
        rig.controller
            .apply(Action::RemoveFromSpotify(theirs, vec![0]));
        assert!(rig.sent().is_empty());
    }

    #[test]
    fn spotify_playlists_are_deleted_or_unfollowed_and_come_back() {
        let mut rig = Rig::new("unlist-spotify");
        rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
        let playlist = |uri: &str, name: &str, owner: Option<&str>| SpotifyPlaylist {
            uri: uri.into(),
            name: name.into(),
            tracks: 1,
            owner: owner.map(String::from),
            private: false,
        };
        // Listed as Spotify has them; shown the account's own first.
        rig.tell(Event::SpotifyPlaylists(Ok(vec![
            playlist(crate::spotify::catalog::LIKED_SONGS, "Liked Songs", None),
            playlist("spotify:playlist:theirs", "Theirs", Some("Them")),
            playlist("spotify:playlist:mine", "Mine", None),
        ])));
        rig.controller
            .apply(Action::OpenSpotifyPlaylist("spotify:playlist:mine".into()));
        rig.sent();
        // Liked Songs stays.
        rig.controller.apply(Action::UnlistSpotifyPlaylist(
            crate::spotify::catalog::LIKED_SONGS.into(),
        ));
        assert!(rig.sent().is_empty());

        rig.controller.apply(Action::UnlistSpotifyPlaylist(
            "spotify:playlist:mine".into(),
        ));
        let browse = &rig.controller.state().browse;
        let listed = |browse: &Browse| match &browse.spotify_playlists {
            Some(Loadable::Ready(p)) => p.iter().map(|p| p.name.clone()).collect(),
            _ => Vec::new(),
        };
        assert_eq!(listed(browse), ["Liked Songs", "Theirs"]);
        // The one after it in the list is shown instead.
        assert_eq!(
            browse.spotify_open.as_deref(),
            Some("spotify:playlist:theirs")
        );
        let sent = rig.sent();
        let notice = sent
            .iter()
            .find_map(|c| match c {
                Command::UnlistSpotifyPlaylist {
                    playlist, notice, ..
                } if playlist == "spotify:playlist:mine" => Some(notice.clone()),
                _ => None,
            })
            .expect("sent to Spotify");
        assert_eq!(notice, "Deleted “Mine” on Spotify.");
        let unlisted = Unlisted {
            playlist: "spotify:playlist:mine".into(),
            name: "Mine".into(),
            at: 1,
            item: vec![1],
            notice,
        };
        rig.tell(Event::SpotifyUnlisted {
            result: Ok(unlisted.clone()),
            undo: false,
        });
        assert_eq!(
            rig.controller.state().notice.as_deref(),
            Some("Deleted “Mine” on Spotify.")
        );
        assert!(
            rig.sent()
                .iter()
                .any(|c| matches!(c, Command::SpotifyPlaylists))
        );
        rig.controller.apply(Action::UndoSpotifyRemoval);
        assert!(
            rig.sent()
                .iter()
                .any(|c| matches!(c, Command::RelistSpotifyPlaylist(u) if *u == unlisted))
        );
        assert!(rig.controller.state().browse.undo.is_none());

        // Someone else's is unfollowed.
        rig.controller.apply(Action::UnlistSpotifyPlaylist(
            "spotify:playlist:theirs".into(),
        ));
        assert!(rig.sent().iter().any(|c| matches!(
            c,
            Command::UnlistSpotifyPlaylist { notice, .. } if notice == "Unfollowed “Theirs”."
        )));
    }

    #[test]
    fn songs_copied_to_spotify_are_sent_once_signed_in() {
        let mut rig = Rig::new("copy-to-spotify");
        let target = SpotifyTarget::New {
            name: "Garage".into(),
            private: true,
        };
        let copy = Action::CopyToSpotify(target.clone(), tracks([1, 2]));
        rig.controller.apply(copy.clone());
        assert!(rig.controller.state().error.is_some());
        rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
        rig.sent();
        rig.controller.apply(copy);
        let sent = rig.sent();
        assert!(sent.iter().any(|c| matches!(
            c,
            Command::CopyToSpotify { target: SpotifyTarget::New { name, private: true }, songs }
                if name == "Garage" && songs.len() == 2
        )));
        rig.tell(Event::CopiedToSpotify {
            target,
            songs: 2,
            result: Ok("spotify:playlist:new".into()),
        });
        assert_eq!(
            rig.controller.state().notice.as_deref(),
            Some("Copied 2 songs to “Garage” on Spotify.")
        );
        assert!(
            rig.sent()
                .iter()
                .any(|c| matches!(c, Command::SpotifyPlaylists))
        );
    }

    #[test]
    fn albums_added_together_arrive_together_in_their_order_whenever_read() {
        let mut rig = Rig::new("add-albums");
        let viewed = rig.controller.state().viewed;
        let uris = |rig: &Rig| -> Vec<String> {
            let list = rig.controller.state().library.get(viewed).unwrap();
            list.entries().iter().map(|e| e.track.uri.clone()).collect()
        };
        let before = uris(&rig);
        let albums = ["spotify:album:a", "spotify:album:b", "spotify:album:c"];
        rig.controller
            .apply(Action::AddAlbums(albums.map(String::from).to_vec()));
        // Read last first, one failing: nothing until all are answered.
        rig.tell(Event::Album {
            album: albums[2].into(),
            result: Ok(songs(tracks([5, 6]))),
        });
        rig.tell(Event::Album {
            album: albums[1].into(),
            result: Err("gone".into()),
        });
        assert_eq!(uris(&rig), before);
        rig.tell(Event::Album {
            album: albums[0].into(),
            result: Ok(songs(tracks([1, 2]))),
        });
        let added: Vec<String> = tracks([1, 2, 5, 6]).into_iter().map(|t| t.uri).collect();
        assert_eq!(uris(&rig)[before.len()..], added);
        assert_eq!(
            rig.controller.state().error.as_deref(),
            Some("One of the releases couldn't be read.")
        );
    }

    #[test]
    fn a_find_on_an_artist_page_reads_its_tabs_songs_and_a_new_page_closes_it() {
        use crate::model::catalog::{ReleaseGroup, ReleaseTab};
        let mut rig = Rig::new("find");
        let discography = crate::demo::discography();
        let uri = discography.artist.uri.clone();
        let albums = |rig: &mut Rig| -> Vec<String> {
            rig.sent()
                .into_iter()
                .filter_map(|c| match c {
                    Command::Album(album) => Some(album),
                    _ => None,
                })
                .collect()
        };
        rig.controller
            .apply(Action::OpenArtist(discography.artist.clone()));
        // Opened before the discography is read: read once it is.
        rig.controller.apply(Action::Find(Some(String::new())));
        assert!(albums(&mut rig).is_empty());
        rig.tell(Event::Discography {
            artist: uri.clone(),
            result: Ok(discography.clone()),
        });
        let tab = rig.controller.state().browse.tab;
        let of_tab: Vec<String> = discography
            .releases
            .iter()
            .filter(|r| tab.includes(r.group))
            .map(|r| r.uri.clone())
            .collect();
        assert_eq!(albums(&mut rig), of_tab);
        // Another tab reads its own, those not read already.
        rig.controller
            .apply(Action::SetTab(ReleaseTab::SinglesAndEps));
        let singles = albums(&mut rig);
        assert!(!singles.is_empty());
        assert!(singles.iter().all(|album| {
            discography
                .releases
                .iter()
                .any(|r| r.uri == *album && r.group == ReleaseGroup::Single)
        }));
        rig.controller.apply(Action::Search("x".into()));
        assert_eq!(rig.controller.state().browse.find, None);
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
    fn loved_songs_land_where_they_were_dropped_once_found() {
        let mut rig = Rig::new("loved");
        rig.controller.apply(Action::AddTracks(tracks([1, 2])));
        let loved = |title: &str| LovedSong {
            artist: "A".into(),
            title: title.into(),
            date: 0,
        };
        rig.controller.apply(Action::DropLoved(
            vec![loved("Song 7"), loved("Gone"), loved("Song 8")],
            1,
        ));
        let id = rig
            .sent()
            .into_iter()
            .find_map(|c| match c {
                Command::FindSongs { id, songs } if songs.len() == 3 => Some(id),
                _ => None,
            })
            .expect("asked for");
        // An answer to something else changes nothing.
        rig.tell(Event::SongsFound {
            id: id + 1,
            result: Ok(vec![Some(entry(9).track)]),
        });
        assert_eq!(rig.songs(), ["Song 1", "Song 2"]);
        let [seven, eight] = [7, 8].map(|id| Some(entry(id).track));
        rig.tell(Event::SongsFound {
            id,
            result: Ok(vec![seven, None, eight]),
        });
        assert_eq!(rig.songs(), ["Song 1", "Song 7", "Song 8", "Song 2"]);
        let notice = rig.controller.state().notice.clone().unwrap_or_default();
        assert!(
            notice.ends_with("1 of the 3 songs isn't on Spotify"),
            "{notice}"
        );
        // Told twice, it's added once.
        rig.tell(Event::SongsFound {
            id,
            result: Ok(vec![Some(entry(9).track)]),
        });
        assert_eq!(rig.songs().len(), 4);
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
    fn enter_on_a_song_to_come_plays_the_queue_here_from_it() {
        let mut rig = Rig::new("play-from");
        rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
        rig.tell(Event::Spotify(Some(SpotifyQueue {
            device: Some("Phone".into()),
            device_id: Some("phone".into()),
            status: Status::Playing,
            now: Some(entry(7).track),
            before: vec![entry(6).track],
            parts: vec![crate::spotify::queue::Part {
                title: "Next in queue".into(),
                queued: true,
                songs: vec![crate::spotify::queue::Queued {
                    track: entry(8).track,
                    index: 0,
                }],
            }],
            ..SpotifyQueue::default()
        })));
        rig.sent();
        let play = |rig: &mut Rig, row| {
            rig.controller.apply(Action::PlayEntry(
                PlaylistId::SPOTIFY,
                SpotifyQueue::row_id(row),
            ));
            rig.sent()
                .into_iter()
                .filter_map(|c| match c {
                    Command::Spotify(SpotifyRequest::PlayHere(song)) => Some(song),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(play(&mut rig, 0), []);
        assert_eq!(play(&mut rig, 1), [None]);
        assert_eq!(play(&mut rig, 2), [Some(entry(8).track.uri)]);
    }

    #[test]
    fn the_spotify_tab_closes_and_opens_again_and_is_remembered() {
        let mut rig = Rig::new("spotify-tab");
        // Off at first.
        assert!(!rig.controller.state().settings.spotify_tab_shown());
        rig.controller.apply(Action::SetSpotifyTabEnabled(true));
        rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
        rig.tell(Event::Spotify(Some(SpotifyQueue::default())));
        rig.controller
            .apply(Action::SelectPlaylist(PlaylistId::SPOTIFY));
        rig.sent();
        rig.controller
            .apply(Action::ClosePlaylist(PlaylistId::SPOTIFY));
        assert!(
            rig.sent()
                .iter()
                .any(|c| matches!(c, Command::SpotifyTab(false)))
        );
        let state = rig.controller.state();
        assert!(!state.settings.spotify_tab);
        assert_ne!(state.viewed, PlaylistId::SPOTIFY);
        rig.controller.apply(Action::OpenSpotifyTab);
        assert!(
            rig.sent()
                .iter()
                .any(|c| matches!(c, Command::SpotifyTab(true)))
        );
        assert!(rig.controller.state().settings.spotify_tab);
        // Off in Settings, it leaves Connect; on again, it opens.
        rig.controller.apply(Action::SetSpotifyTabEnabled(false));
        assert!(
            rig.sent()
                .iter()
                .any(|c| matches!(c, Command::SpotifyTab(false)))
        );
        rig.controller
            .apply(Action::ClosePlaylist(PlaylistId::SPOTIFY));
        rig.controller.apply(Action::SetSpotifyTabEnabled(true));
        assert!(
            rig.sent()
                .iter()
                .any(|c| matches!(c, Command::SpotifyTab(true)))
        );
        assert!(rig.controller.state().settings.spotify_tab_shown());
    }

    #[test]
    fn the_spotify_tab_moves_like_any_tab_and_keeps_its_place() {
        let mut rig = Rig::new("spotify-move");
        rig.controller.apply(Action::NewPlaylist);
        let second = rig.controller.state().viewed;
        rig.tell(Event::Spotify(Some(SpotifyQueue::default())));
        let spotify = PlaylistId::SPOTIFY;
        assert_eq!(
            rig.controller.state().tabs(),
            [spotify, PlaylistId::DEFAULT, second]
        );
        rig.controller.apply(Action::MovePlaylist(spotify, 2));
        assert_eq!(
            rig.controller.state().tabs(),
            [PlaylistId::DEFAULT, second, spotify]
        );
        rig.controller.apply(Action::MovePlaylist(second, 0));
        assert_eq!(
            rig.controller.state().tabs(),
            [second, PlaylistId::DEFAULT, spotify]
        );
        rig.controller.save();
        let paths = rig.controller.paths.clone();
        let settings = Settings::load(&paths);
        let mut again = Controller::with_backend(paths, settings, Ok(Backend::detached().0));
        assert_eq!(again.state().spotify_at, 2);
        again.state.spotify = Some(SpotifyQueue::default());
        assert_eq!(again.state().tabs(), [second, PlaylistId::DEFAULT, spotify]);
    }

    #[test]
    fn clear_asks_spotify_to_clear_and_ends_a_wait_for_the_queue() {
        let mut rig = Rig::new("clear");
        rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
        let elsewhere = SpotifyQueue {
            device: Some("Phone".into()),
            device_id: Some("phone".into()),
            status: Status::Paused,
            now: Some(entry(7).track),
            ..SpotifyQueue::default()
        };
        rig.tell(Event::Spotify(Some(elsewhere.clone())));
        rig.controller.apply(Action::PlayEntry(
            PlaylistId::SPOTIFY,
            SpotifyQueue::row_id(0),
        ));
        rig.tell(Event::Spotify(Some(SpotifyQueue {
            here: true,
            ..elsewhere
        })));
        rig.sent();
        rig.controller.apply(Action::ClearSpotify);
        assert!(
            rig.sent()
                .iter()
                .any(|c| matches!(c, Command::Spotify(SpotifyRequest::Clear)))
        );
        assert_eq!(rig.controller.state().spotify_starting, None);
    }

    #[test]
    fn a_cleared_queue_is_no_tab_playing() {
        let mut rig = Rig::new("cleared");
        rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
        let here = SpotifyQueue {
            device: Some("Spotless".into()),
            device_id: Some("spotless".into()),
            here: true,
            status: Status::Playing,
            now: Some(entry(7).track),
            position_ms: 30_000,
            ..SpotifyQueue::default()
        };
        rig.tell(Event::Spotify(Some(here.clone())));
        rig.controller.apply(Action::PlayEntry(
            PlaylistId::SPOTIFY,
            SpotifyQueue::row_id(0),
        ));
        rig.tell(Event::Spotify(Some(here.clone())));
        assert_eq!(
            rig.controller.state().playing_list(),
            Some(PlaylistId::SPOTIFY)
        );
        // Empty Track, paused, is shown as no song.
        rig.tell(Event::Spotify(Some(SpotifyQueue {
            status: Status::Paused,
            now: None,
            position_ms: 0,
            ..here
        })));
        assert_eq!(rig.controller.state().playing_list(), None);
    }

    #[test]
    fn a_queue_held_elsewhere_only_shows_until_taken_over() {
        let mut rig = Rig::new("take-over");
        rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
        rig.tell(Event::Spotify(Some(SpotifyQueue {
            device: Some("Phone".into()),
            device_id: Some("phone".into()),
            status: Status::Playing,
            now: Some(entry(7).track),
            ..SpotifyQueue::default()
        })));
        rig.sent();
        rig.controller.apply(Action::ClearSpotify);
        assert!(rig.sent().is_empty());
        rig.controller.apply(Action::TakeOverSpotify);
        assert!(
            rig.sent()
                .iter()
                .any(|c| matches!(c, Command::Spotify(SpotifyRequest::TakeOver)))
        );
        // The player shows the queue from now on.
        assert!(rig.controller.state().spotify_playing());
    }

    #[test]
    fn play_here_shows_the_queue_loading_until_it_plays_here() {
        let mut rig = Rig::new("play-here");
        rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
        let elsewhere = SpotifyQueue {
            device: Some("Phone".into()),
            device_id: Some("phone".into()),
            status: Status::Paused,
            now: Some(entry(7).track),
            ..SpotifyQueue::default()
        };
        rig.tell(Event::Spotify(Some(elsewhere.clone())));
        rig.controller.apply(Action::PlayEntry(
            PlaylistId::SPOTIFY,
            SpotifyQueue::row_id(0),
        ));
        let shown = rig.controller.state().now_playing();
        assert_eq!(shown.status, Status::Loading);
        assert_eq!(shown.track.unwrap().uri, entry(7).track.uri);
        rig.tell(Event::Spotify(Some(SpotifyQueue {
            here: true,
            status: Status::Playing,
            ..elsewhere
        })));
        assert_eq!(rig.controller.state().now_playing().status, Status::Playing);
    }

    #[test]
    fn songs_go_to_a_queue_no_device_holds_but_not_to_one_another_does() {
        let mut rig = Rig::new("queue-add");
        rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
        rig.tell(Event::Spotify(Some(SpotifyQueue::default())));
        rig.controller.apply(Action::SetTarget(PlaylistId::SPOTIFY));
        rig.sent();
        let added = |rig: &mut Rig| {
            rig.controller.apply(Action::AddTracks(tracks([1])));
            rig.sent()
                .iter()
                .any(|c| matches!(c, Command::Spotify(SpotifyRequest::Add(_))))
        };
        assert!(added(&mut rig));
        assert_eq!(rig.controller.state().error, None);
        rig.tell(Event::Spotify(Some(SpotifyQueue {
            device: Some("Phone".into()),
            device_id: Some("phone".into()),
            status: Status::Playing,
            now: Some(entry(7).track),
            ..SpotifyQueue::default()
        })));
        assert!(!added(&mut rig));
        assert!(
            rig.controller
                .state()
                .error
                .as_ref()
                .unwrap()
                .contains("(Phone)")
        );
    }

    #[test]
    fn release_pauses_the_queue_playing_here_but_not_elsewhere() {
        let mut rig = Rig::new("release");
        rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
        let elsewhere = SpotifyQueue {
            device: Some("Phone".into()),
            device_id: Some("phone".into()),
            status: Status::Playing,
            now: Some(entry(7).track),
            ..SpotifyQueue::default()
        };
        rig.tell(Event::Spotify(Some(elsewhere.clone())));
        rig.controller.apply(Action::PlayEntry(
            PlaylistId::SPOTIFY,
            SpotifyQueue::row_id(0),
        ));
        let paused = |rig: &mut Rig| {
            rig.sent()
                .iter()
                .any(|c| matches!(c, Command::Spotify(SpotifyRequest::Pause)))
        };
        rig.tell(Event::Spotify(Some(SpotifyQueue {
            here: true,
            ..elsewhere.clone()
        })));
        rig.sent();
        rig.controller.apply(Action::Release);
        assert!(paused(&mut rig));
        // Handed to another device, the queue is no sound here to let go.
        rig.tell(Event::Spotify(Some(elsewhere)));
        rig.sent();
        rig.controller.apply(Action::Release);
        assert!(!paused(&mut rig));
    }

    #[test]
    fn a_local_song_keeps_the_player_paused_until_the_queue_plays_again() {
        let mut rig = Rig::new("owner");
        rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
        let paused_here = SpotifyQueue {
            device: Some("Spotless".into()),
            device_id: Some("spotless".into()),
            here: true,
            status: Status::Paused,
            now: Some(entry(7).track),
            position_ms: 30_000,
            ..SpotifyQueue::default()
        };
        rig.tell(Event::Spotify(Some(paused_here.clone())));
        assert!(rig.controller.state().spotify_playing());
        rig.controller.apply(Action::AddTracks(tracks([1, 2])));
        let first = rig
            .controller
            .state()
            .library
            .get(PlaylistId::DEFAULT)
            .unwrap()
            .entries()[0]
            .id;
        rig.controller
            .apply(Action::PlayEntry(PlaylistId::DEFAULT, first));
        let local = |status| PlayerSnapshot {
            status,
            track: Some(TrackInfo {
                uri: "spotify:track:1".into(),
                title: String::new(),
                artists: String::new(),
                main_artist: String::new(),
                album: String::new(),
                duration_ms: 1000,
                cover_url: None,
            }),
            ..PlayerSnapshot::default()
        };
        rig.tell(Event::Player(local(Status::Playing)));
        rig.sent();
        // Space on the Spotify tab pauses the local song, which stays shown.
        rig.controller
            .apply(Action::SelectPlaylist(PlaylistId::SPOTIFY));
        rig.controller.apply(Action::TogglePause);
        assert!(
            rig.sent()
                .iter()
                .any(|c| matches!(c, Command::Player(Request::Pause)))
        );
        rig.tell(Event::Player(local(Status::Paused)));
        let state = rig.controller.state();
        assert!(!state.spotify_playing());
        assert_eq!(state.now_playing().track.unwrap().uri, "spotify:track:1");
        // Play on the player goes on with it, rather than the queue.
        rig.controller.apply(Action::Play);
        assert!(
            rig.sent()
                .iter()
                .any(|c| matches!(c, Command::Player(Request::Resume)))
        );
        rig.tell(Event::Player(local(Status::Paused)));
        // The queue takes the player back once it plays.
        rig.tell(Event::Spotify(Some(SpotifyQueue {
            status: Status::Playing,
            ..paused_here
        })));
        assert!(rig.controller.state().spotify_playing());
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
        // Until the new player knows the song, the song stays shown.
        rig.tell(Event::Player(PlayerSnapshot {
            status: Status::Loading,
            ..PlayerSnapshot::default()
        }));
        let player = &rig.controller.state().player;
        assert_eq!(player.status, Status::Loading);
        assert_eq!(player.position_ms, 95_000);
        assert_eq!(player.track.as_ref().unwrap().title, "Song 1");
        let known = PlayerSnapshot {
            status: Status::Playing,
            track: player.track.clone(),
            position_ms: 95_100,
            ..PlayerSnapshot::default()
        };
        rig.tell(Event::Player(known.clone()));
        assert_eq!(rig.controller.state().player, known);
        rig.tell(Event::Player(PlayerSnapshot::default()));
        assert_eq!(rig.controller.state().player, PlayerSnapshot::default());
    }

    #[test]
    fn a_song_left_playing_shows_paused_there_after_a_restart() {
        let mut rig = Rig::new("restart-paused");
        rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
        rig.controller.apply(Action::AddTracks(tracks([1, 2])));
        rig.controller.apply(Action::Play);
        rig.tell(Event::Player(PlayerSnapshot {
            status: Status::Playing,
            track: Some(TrackInfo::from(&entry(1).track)),
            position_ms: 95_000,
            ..PlayerSnapshot::default()
        }));
        rig.controller.save();

        let paths = rig.controller.paths.clone();
        let (backend, mut commands, events) = Backend::detached();
        let settings = Settings::load(&paths);
        let mut controller = Controller::with_backend(paths, settings, Ok(backend));
        events
            .send(Event::Session(SessionState::Connecting))
            .unwrap();
        controller.receive();
        events
            .send(Event::Session(SessionState::Ready { user: "u".into() }))
            .unwrap();
        controller.receive();
        let shown = controller.state().now_playing();
        assert_eq!(shown.status, Status::Paused);
        assert_eq!(shown.position_ms, 95_000);
        assert_eq!(shown.track.unwrap().title, "Song 1");
        controller.apply(Action::Seek(60_000));
        assert_eq!(controller.state().now_playing().position_ms, 60_000);
        controller.apply(Action::TogglePause);
        let sent: Vec<_> = std::iter::from_fn(|| commands.try_recv().ok()).collect();
        assert!(sent.iter().any(|c| matches!(
            c,
            Command::Player(Request::Load { uri, position_ms: 60_000, .. }) if uri.ends_with(":1")
        )));
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
    fn more_songs_reads_the_next_page_once_and_adds_the_new_ones() {
        use crate::model::catalog::SearchResults;
        use crate::spotify::webapi::SongsPage;
        let mut rig = Rig::new("more-songs");
        rig.controller.apply(Action::Search("boards".into()));
        rig.sent();
        rig.tell(Event::SearchResults {
            query: "boards".into(),
            results: Ok(SearchResults {
                songs: tracks([1, 2]),
                songs_read: 10,
                more_songs: true,
                ..SearchResults::default()
            }),
        });
        rig.controller.apply(Action::MoreSongs("boards".into()));
        // Pressed again while it reads: asked once.
        rig.controller.apply(Action::MoreSongs("boards".into()));
        let more: Vec<_> = rig
            .sent()
            .into_iter()
            .filter(|c| matches!(c, Command::MoreSongs { .. }))
            .collect();
        assert!(
            matches!(&more[..], [Command::MoreSongs { query, offset: 10 }] if query == "boards")
        );
        rig.tell(Event::MoreSongs {
            query: "boards".into(),
            result: Ok(SongsPage {
                songs: tracks([2, 3]),
                read: 10,
                more: false,
            }),
        });
        let Some(Loadable::Ready(results)) = rig.controller.state().browse.searches.get("boards")
        else {
            panic!("the search's results");
        };
        assert_eq!(results.songs, tracks([1, 2, 3]));
        assert_eq!(results.songs_read, 20);
        assert!(!results.more_songs && !results.reading_songs);
        rig.controller.apply(Action::MoreSongs("boards".into()));
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
    fn after_a_restart_the_song_play_goes_on_with_is_shown() {
        let mut rig = Rig::new("restart");
        rig.controller.apply(Action::AddTracks(tracks([1, 2])));
        let second = rig
            .controller
            .state()
            .library
            .get(PlaylistId::DEFAULT)
            .unwrap()
            .entries()[1]
            .id;
        rig.controller
            .apply(Action::PlayEntry(PlaylistId::DEFAULT, second));
        rig.controller.apply(Action::Stop);
        rig.controller.save();
        let (backend, _, _) = Backend::detached();
        let paths = rig.controller.paths.clone();
        let settings = Settings::load(&paths);
        let again = Controller::with_backend(paths, settings, Ok(backend));
        let shown = again.state().now_playing();
        assert_eq!(shown.status, Status::Stopped);
        assert_eq!(shown.track.unwrap().title, "Song 2");
        assert_eq!(
            again.state().transport.playback.playing(),
            Some(PlaylistId::DEFAULT)
        );
    }

    #[test]
    fn stopped_the_player_shows_the_song_play_starts() {
        let mut rig = Rig::new("waiting");
        rig.controller.apply(Action::AddTracks(tracks([1, 2])));
        let entries: Vec<_> = rig
            .controller
            .state()
            .library
            .get(PlaylistId::DEFAULT)
            .unwrap()
            .entries()
            .iter()
            .map(|entry| entry.id)
            .collect();
        rig.controller
            .apply(Action::PlayEntry(PlaylistId::DEFAULT, entries[0]));
        rig.controller.apply(Action::Stop);
        // The engine still has the stopped song.
        rig.tell(Event::Player(PlayerSnapshot {
            track: Some(TrackInfo::from(&entry(1).track)),
            ..PlayerSnapshot::default()
        }));
        let title = |rig: &Rig| {
            rig.controller
                .state()
                .now_playing()
                .track
                .map(|track| track.title)
        };
        assert_eq!(title(&rig).as_deref(), Some("Song 1"));
        rig.controller
            .apply(Action::Remove(PlaylistId::DEFAULT, vec![entries[0]]));
        assert_eq!(
            title(&rig).as_deref(),
            Some("Song 2"),
            "Play goes on with the next"
        );
        rig.controller.apply(Action::Play);
        assert_eq!(
            title(&rig).as_deref(),
            Some("Song 2"),
            "loading, before the engine says so"
        );
        rig.controller.apply(Action::Stop);
        rig.controller
            .apply(Action::Remove(PlaylistId::DEFAULT, vec![entries[1]]));
        assert_eq!(title(&rig), None, "an empty playlist plays nothing");
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

    fn titles(rig: &Rig, list: PlaylistId) -> Vec<String> {
        let library = &rig.controller.state().library;
        let list = library.get(list).unwrap();
        list.entries()
            .iter()
            .map(|entry| entry.track.title.clone())
            .collect()
    }

    #[test]
    fn songs_moved_to_another_playlist_leave_this_one() {
        let mut rig = Rig::new("move-to");
        rig.controller.apply(Action::AddTracks(tracks([1, 2, 3])));
        rig.controller.apply(Action::NewPlaylist);
        let other = rig.controller.state().viewed;
        let library = &rig.controller.state().library;
        let entries = library.get(PlaylistId::DEFAULT).unwrap().entries();
        let moved = vec![entries[2].id, entries[0].id];
        rig.controller
            .apply(Action::MoveTo(PlaylistId::DEFAULT, other, moved));
        assert_eq!(titles(&rig, PlaylistId::DEFAULT), ["Song 2"]);
        assert_eq!(titles(&rig, other), ["Song 3", "Song 1"]);
        assert_eq!(
            rig.controller.state().notice.as_deref(),
            Some("Moved 2 songs to Playlist 2")
        );
    }

    #[test]
    fn songs_moved_to_the_spotify_tab_stay_when_it_has_no_queue() {
        let mut rig = Rig::new("move-to-spotify");
        rig.controller.apply(Action::AddTracks(tracks([1, 2])));
        let library = &rig.controller.state().library;
        let first = library.get(PlaylistId::DEFAULT).unwrap().entries()[0].id;
        rig.controller.apply(Action::MoveTo(
            PlaylistId::DEFAULT,
            PlaylistId::SPOTIFY,
            vec![first],
        ));
        assert_eq!(titles(&rig, PlaylistId::DEFAULT), ["Song 1", "Song 2"]);
    }

    #[test]
    fn a_new_playlist_can_start_with_songs() {
        let mut rig = Rig::new("new-with");
        rig.controller
            .apply(Action::NewPlaylistWith(vec![entry(4).track]));
        let shown = rig.controller.state().viewed;
        assert_ne!(shown, PlaylistId::DEFAULT);
        assert_eq!(titles(&rig, shown), ["Song 4"]);
    }

    #[test]
    fn songs_to_come_move_out_of_the_queue_only_while_spotless_has_it() {
        let mut rig = Rig::new("move-from-spotify");
        rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
        let queue = SpotifyQueue {
            device: Some("Phone".into()),
            device_id: Some("phone".into()),
            status: Status::Playing,
            now: Some(entry(7).track),
            parts: vec![crate::spotify::queue::Part {
                title: "Next in queue".into(),
                queued: true,
                songs: vec![crate::spotify::queue::Queued {
                    track: entry(8).track,
                    index: 0,
                }],
            }],
            ..SpotifyQueue::default()
        };
        rig.tell(Event::Spotify(Some(queue.clone())));
        rig.sent();
        let move_rows = |rig: &mut Rig, rows: Vec<usize>| {
            let rows = rows.into_iter().map(SpotifyQueue::row_id).collect();
            rig.controller.apply(Action::MoveTo(
                PlaylistId::SPOTIFY,
                PlaylistId::DEFAULT,
                rows,
            ));
            rig.sent()
                .into_iter()
                .filter_map(|c| match c {
                    Command::Spotify(SpotifyRequest::Remove(songs)) => Some(songs),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        // Held by the phone: nothing moves.
        assert_eq!(move_rows(&mut rig, vec![1]), Vec::<Vec<_>>::new());
        assert!(titles(&rig, PlaylistId::DEFAULT).is_empty());
        // Held by no device: it moves, Spotless taking the queue over.
        rig.tell(Event::Spotify(Some(SpotifyQueue {
            device: None,
            device_id: None,
            status: Status::Stopped,
            ..queue.clone()
        })));
        assert_eq!(
            move_rows(&mut rig, vec![1]),
            [vec![(0, entry(8).track.uri)]]
        );
        rig.tell(Event::Spotify(Some(SpotifyQueue {
            device: Some("Spotless".into()),
            device_id: Some("spotless".into()),
            here: true,
            ..queue
        })));
        // The song playing can't be taken out.
        assert_eq!(move_rows(&mut rig, vec![0, 1]), Vec::<Vec<_>>::new());
        assert_eq!(
            move_rows(&mut rig, vec![1]),
            [vec![(0, entry(8).track.uri)]]
        );
        assert_eq!(titles(&rig, PlaylistId::DEFAULT), ["Song 8", "Song 8"]);
        assert_eq!(
            rig.controller.state().notice.as_deref(),
            Some("Moved 1 song to Default")
        );
    }

    mod synced {
        use super::*;
        use crate::model::sync::{ItemId, Position};
        use crate::spotify::synced::{Changed, Done, Job, Remote};

        const URI: &str = "spotify:playlist:gym";

        fn item(n: u64) -> ItemId {
            ItemId(format!("{n:04x}"))
        }

        fn ready(rig: &mut Rig) {
            rig.tell(Event::Session(SessionState::Ready { user: "me".into() }));
        }

        fn jobs(rig: &mut Rig) -> Vec<Job> {
            rig.sent()
                .into_iter()
                .filter_map(|command| match command {
                    Command::Sync { job, .. } => Some(job),
                    _ => None,
                })
                .collect()
        }

        /// Spotify's answer: these songs (by number: item and track
        /// alike), the job's songs given their items by track.
        fn done(job: &Job, songs: &[u64], position: Option<Position>) -> Done {
            let sent = job
                .local
                .iter()
                .map(|(id, item, uri)| {
                    let n: u64 = uri.rsplit(':').next().unwrap().parse().unwrap();
                    (*id, item.clone().or(Some(item_n(n, songs))))
                })
                .collect();
            Done {
                remote: Remote {
                    revision: songs.iter().map(|&n| n as u8).collect(),
                    name: "Gym".into(),
                    description: position.as_ref().map(Position::encode).unwrap_or_default(),
                    songs: songs
                        .iter()
                        .map(|&n| (item(n), format!("spotify:track:{n}")))
                        .collect(),
                },
                sent,
                tracks: Vec::new(),
                position,
            }
        }

        fn item_n(n: u64, _songs: &[u64]) -> ItemId {
            item(n)
        }

        /// A playlist "Gym" of songs 1 and 2, synced and sent.
        fn synced_gym(rig: &mut Rig) -> PlaylistId {
            synced_gym_named(rig, URI)
        }

        /// The same, as this Spotify playlist.
        fn synced_gym_named(rig: &mut Rig, uri: &str) -> PlaylistId {
            ready(rig);
            rig.controller
                .apply(Action::NewPlaylistWith(tracks([1, 2])));
            let list = rig.controller.state().viewed;
            rig.controller
                .apply(Action::RenamePlaylist(list, "Gym".into()));
            rig.sent();
            rig.controller.apply(Action::SetSync(list, true));
            assert!(rig.sent().iter().any(
                |c| matches!(c, Command::CreateSynced { list: l, name } if *l == list && name == "Gym")
            ));
            assert_eq!(
                rig.controller.state().sync.get(&list),
                Some(&SyncStatus::Waiting)
            );
            rig.tell(Event::SyncCreated {
                list,
                result: Ok(uri.into()),
            });
            let job = jobs(rig).pop().expect("sent whole");
            assert_eq!(job.uri, uri);
            assert!(job.base.is_empty());
            assert!(job.local.iter().all(|(_, item, _)| item.is_none()));
            // Not yet: its song isn't on Spotify.
            assert_eq!(job.position, None);
            rig.tell(Event::Synced {
                list,
                result: Ok(done(&job, &[1, 2], None)),
            });
            // Now that its songs have IDs, where it's up to follows.
            let job = jobs(rig).pop().expect("the position");
            let position = job.position.clone().expect("a position");
            assert_eq!(position.song, Some(item(1)));
            rig.tell(Event::Synced {
                list,
                result: Ok(done(&job, &[1, 2], Some(position))),
            });
            list
        }

        fn titles(rig: &Rig, list: PlaylistId) -> Vec<String> {
            super::titles(rig, list)
        }

        #[test]
        fn a_playlist_made_synced_is_sent_whole_then_its_position() {
            let mut rig = Rig::new("sync-whole");
            let list = synced_gym(&mut rig);
            let state = rig.controller.state();
            let gym = state.library.get(list).unwrap();
            let items: Vec<_> = gym.entries().iter().map(|e| e.item.clone()).collect();
            assert_eq!(items, [Some(item(1)), Some(item(2))]);
            assert_eq!(state.sync.get(&list), Some(&SyncStatus::UpToDate));
            assert!(state.library.synced_seen.contains(&URI.to_string()));
            // Unchanged, it isn't sent again.
            rig.controller.save();
            assert!(jobs(&mut rig).is_empty());
        }

        #[test]
        fn songs_from_another_device_arrive_with_its_newer_position() {
            let mut rig = Rig::new("sync-arrive");
            let list = synced_gym(&mut rig);
            jobs(&mut rig);
            rig.tell(Event::PlaylistChanged(Changed {
                uri: URI.into(),
                revision: vec![9],
            }));
            let job = jobs(&mut rig).pop().expect("read again");
            let theirs = Position {
                song: Some(item(3)),
                resume_s: 30,
                saved_at: u64::MAX / 2,
                marks: vec![item(2)],
            };
            let mut answer = done(&job, &[1, 3, 2], Some(theirs));
            answer.tracks = vec![(item(3), Some(entry(3).track))];
            rig.tell(Event::Synced {
                list,
                result: Ok(answer),
            });
            assert_eq!(titles(&rig, list), ["Song 1", "Song 3", "Song 2"]);
            let gym = rig.controller.state().library.get(list).unwrap();
            let cursor = gym.get(gym.cursor().unwrap()).unwrap();
            assert_eq!(cursor.track.title, "Song 3");
            assert_eq!(gym.resume_ms, 30_000);
            assert_eq!(gym.mark_number(gym.entries()[2].id), Some(1));
            // Its own change, told back, isn't read again.
            let revision = vec![1, 3, 2];
            rig.tell(Event::PlaylistChanged(Changed {
                uri: URI.into(),
                revision,
            }));
            assert!(jobs(&mut rig).iter().all(|job| job.position.is_some()));
        }

        #[test]
        fn edits_made_offline_wait_and_go_once_connected() {
            let mut rig = Rig::new("sync-offline");
            let list = synced_gym(&mut rig);
            jobs(&mut rig);
            rig.tell(Event::Session(SessionState::Connecting));
            let first = rig.controller.state().library.get(list).unwrap().entries()[0].id;
            rig.controller.apply(Action::Remove(list, vec![first]));
            rig.controller.save();
            assert!(jobs(&mut rig).is_empty());
            assert_eq!(
                rig.controller.state().sync.get(&list),
                Some(&SyncStatus::Waiting)
            );
            ready(&mut rig);
            assert!(rig.sent().iter().any(|c| matches!(c, Command::SyncFolder)));
            rig.tell(Event::SyncFolder(Ok(vec![(URI.into(), "Gym".into())])));
            let job = jobs(&mut rig).pop().expect("sent once connected");
            assert_eq!(job.base, [item(1), item(2)]);
            assert_eq!(job.local.len(), 1);
            rig.tell(Event::Synced {
                list,
                result: Ok(done(&job, &[2], None)),
            });
            assert_eq!(titles(&rig, list), ["Song 2"]);
            assert_eq!(
                rig.controller.state().sync.get(&list),
                Some(&SyncStatus::UpToDate)
            );
        }

        #[test]
        fn the_folder_opens_new_playlists_and_unsyncs_deleted_ones() {
            let mut rig = Rig::new("sync-folder");
            let list = synced_gym(&mut rig);
            // Made synced a while ago, as far as the folder goes.
            rig.controller.syncing.get_mut(&list).unwrap().created = None;
            rig.sent();
            rig.tell(Event::SyncFolder(Ok(vec![(
                "spotify:playlist:road".into(),
                "Road trip".into(),
            )])));
            let state = rig.controller.state();
            assert!(state.library.get(list).unwrap().sync.is_none());
            assert!(
                state
                    .notice
                    .as_deref()
                    .unwrap()
                    .starts_with("Gym was deleted")
            );
            let road = state
                .library
                .synced("spotify:playlist:road")
                .expect("opened");
            assert_eq!(road.name, "Road trip");
            let road = road.id;
            // Read, with no position written before it has read Spotify's.
            let road_jobs: Vec<Job> = jobs(&mut rig)
                .into_iter()
                .filter(|job| job.uri == "spotify:playlist:road")
                .collect();
            assert_eq!(road_jobs.len(), 1);
            assert_eq!(road_jobs[0].position, None);

            // Closed here, it doesn't open again.
            rig.controller.apply(Action::ClosePlaylist(road));
            rig.tell(Event::SyncFolder(Ok(vec![(
                "spotify:playlist:road".into(),
                "Road trip".into(),
            )])));
            assert!(
                rig.controller
                    .state()
                    .library
                    .synced("spotify:playlist:road")
                    .is_none()
            );
        }

        #[test]
        fn renamed_here_or_there_the_name_follows() {
            let mut rig = Rig::new("sync-rename");
            let list = synced_gym(&mut rig);
            jobs(&mut rig);
            rig.controller
                .apply(Action::RenamePlaylist(list, "Gym days".into()));
            rig.controller.save();
            let job = jobs(&mut rig).pop().expect("renamed");
            assert_eq!(job.rename.as_deref(), Some("Gym days"));
            let mut answer = done(&job, &[1, 2], None);
            answer.remote.name = "Gym days".into();
            rig.tell(Event::Synced {
                list,
                result: Ok(answer),
            });
            rig.tell(Event::PlaylistChanged(Changed {
                uri: URI.into(),
                revision: vec![7],
            }));
            let job = jobs(&mut rig).pop().expect("read again");
            assert_eq!(job.rename, None);
            let mut answer = done(&job, &[1, 2], None);
            answer.remote.name = "Leg day".into();
            rig.tell(Event::Synced {
                list,
                result: Ok(answer),
            });
            assert_eq!(
                rig.controller.state().library.get(list).unwrap().name,
                "Leg day"
            );
        }

        #[test]
        fn stopped_or_closed_it_stays_on_spotify_and_deleted_it_closes() {
            let mut rig = Rig::new("sync-stop");
            let list = synced_gym(&mut rig);
            rig.controller.apply(Action::SetSync(list, false));
            let gym = rig.controller.state().library.get(list).unwrap();
            assert!(gym.sync.is_none());
            assert_eq!(gym.len(), 2);
            assert!(gym.entries().iter().all(|e| e.item.is_none()));

            let list = {
                rig.controller.apply(Action::SetSync(list, true));
                rig.tell(Event::SyncCreated {
                    list,
                    result: Ok(URI.into()),
                });
                list
            };
            rig.sent();
            // Closed here, it's only gone from here.
            rig.controller.apply(Action::ClosePlaylist(list));
            assert!(rig.controller.state().library.get(list).is_none());
            assert!(rig.sent().is_empty());
            // Deleted from Browse, its open tab closes too.
            let road = synced_gym_named(&mut rig, "spotify:playlist:road");
            rig.tell(Event::SpotifyPlaylists(Ok(vec![SpotifyPlaylist {
                uri: "spotify:playlist:road".into(),
                name: "Gym".into(),
                tracks: 2,
                owner: None,
                private: true,
            }])));
            rig.controller.apply(Action::UnlistSpotifyPlaylist(
                "spotify:playlist:road".into(),
            ));
            let notice = rig
                .sent()
                .into_iter()
                .find_map(|c| match c {
                    Command::UnlistSpotifyPlaylist { notice, .. } => Some(notice),
                    _ => None,
                })
                .expect("sent to Spotify");
            assert_eq!(notice, "Deleted “Gym” on Spotify and every device.");
            rig.tell(Event::SpotifyUnlisted {
                result: Ok(Unlisted {
                    playlist: "spotify:playlist:road".into(),
                    name: "Gym".into(),
                    at: 1,
                    item: vec![1],
                    notice,
                }),
                undo: false,
            });
            let list = road;
            assert!(rig.controller.state().library.get(list).is_none());
        }
    }
}
