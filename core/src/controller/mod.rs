//! The application's state and what changes it, whatever draws it. The
//! interface reads [`State`], hands its [`Action`]s to
//! [`Controller::apply`], and calls [`Controller::receive`] and
//! [`Controller::tick`] whenever the backend wakes it.
//!
//! This file hands each action and each of the backend's events to the
//! part of the controller it belongs to, a file each (an action's kind
//! names its part): `playback`,
//! `playlists`, `browse`, `spotify_lists`, `lastfm`, `spotify_tab`,
//! `sync`, `view` and `settings`; `persist` saves.
//!
//! What only a desktop has (the window, the tray, media keys, skins and
//! textures) stays with the interface, in `app.rs`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

mod browse;
mod lastfm;
mod persist;
mod playback;
mod playlists;
mod settings;
mod spotify_lists;
mod spotify_tab;
mod state;
mod sync;
mod view;

pub use state::State;
pub use sync::SyncStatus;

use crate::action::Action;
use crate::backend::{Backend, Command, Event, SearchState, SessionState, Start};
use crate::browse::{Browse, Loadable};
use crate::browser::{self, Browser};
use crate::images::Cover;
use crate::lastfm::{LastFmState, Loved, LovedSong};
use crate::model::catalog::Place;
use crate::model::{Playback, Playlist, PlaylistId, TrackRef};
use crate::paths::Paths;
use crate::playback::engine::{PlayerSnapshot, Status, TrackInfo};
use crate::playback::link::AudioLink;
use crate::playback::transport::Transport;
use crate::spotify::session::AudioCache;
use crate::store::{self, Settings};
use lastfm::FoundFor;

pub struct Controller {
    paths: Paths,
    backend: Option<Backend>,
    /// The equaliser the audio outputs apply, and the analyser they feed.
    audio: Arc<AudioLink>,
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
    /// Spotify playlists of playlists no longer synced, being deleted:
    /// UNDO doesn't offer to put those back.
    desynced: HashSet<String>,
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
        let audio = audio_link(&settings);
        let start = Start {
            volume: volume_level(settings.volume),
            levelling: settings.levelling,
            spotify_tab: settings.spotify_tab_shown(),
            search_app: settings.web_client_id(),
            audio_cache: AudioCache::from(&settings),
        };
        let started = Backend::start(
            paths.clone(),
            start,
            browser,
            Arc::clone(&audio),
            wake,
        );
        Self::with_backend(paths, settings, audio, started)
    }

    /// Spotless as its screenshots show it ([`crate::demo`]): signed in,
    /// the made-up library with a song playing and the made-up artist
    /// ready to open in Browse, and nothing connected to Spotify. Its
    /// settings are those in `paths`; anything saved goes there too.
    pub fn demo(paths: Paths) -> Self {
        let settings = Settings::load(&paths);
        let audio = audio_link(&settings);
        let mut controller =
            Self::with_backend(paths, settings, audio, Err(anyhow::anyhow!("a demo")));
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

    fn with_backend(
        paths: Paths,
        settings: Settings,
        audio: Arc<AudioLink>,
        started: anyhow::Result<Backend>,
    ) -> Self {
        let (mut library, tabs) = store::load_library(&paths);
        let history = store::load_history(&paths);
        let viewed = tabs.viewed.or(tabs.playing).unwrap_or(library.first());
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
        let mut browse = Browse::new(viewed, settings.covers);
        browse.recent_searches = history.searches;
        browse.recent = history.artists;
        browse.shown = history.tab;
        browse.spotify_open = history.spotify_playlist;
        Self {
            paths,
            backend,
            audio,
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
                sync_name_taken: None,
            },
            unsaved: None,
            pending_adds: Vec::new(),
            pending_batches: Vec::new(),
            finding: Vec::new(),
            desynced: HashSet::new(),
            last_find: 0,
            resuming: false,
            spotify_seeked: None,
            syncing: HashMap::new(),
        }
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    /// The audio playing, for the deck to measure ([`AudioLink::analyser`]).
    pub fn audio(&self) -> &AudioLink {
        &self.audio
    }

    /// Shows this in the notice line, for what only the interface does
    /// (copying a link).
    pub fn tell(&mut self, notice: impl Into<String>) {
        self.state.notice = Some(notice.into());
    }

    /// The settings only the app has (the desktop's tray, skin and
    /// zoom), kept in the core's file ([`Settings::extra`]). They are
    /// saved soon after.
    pub fn app_settings_mut(&mut self) -> &mut serde_json::Map<String, serde_json::Value> {
        self.mark_unsaved();
        self.state.settings.extra_mut()
    }

    /// Takes in what the backend has sent since, and hands back what is
    /// the interface's to deal with: the covers read.
    pub fn receive(&mut self) -> Vec<Cover> {
        let Some(backend) = &self.backend else {
            return Vec::new();
        };
        let events: Vec<Event> = backend.events.try_iter().collect();
        let mut theirs = Vec::new();
        for event in events {
            match event {
                Event::Session(session) => self.session_changed(session),
                Event::Player(snapshot) => self.player_changed(snapshot),
                Event::Followed(entry) => self.song_followed(entry),
                Event::Ended => self.song_ended(),
                Event::Spotify(queue) => self.spotify_changed(queue),
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
                Event::Unavailable { offline: false } => self.song_unavailable(),
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
                Event::LastFm(lastfm) => self.lastfm_changed(lastfm),
                Event::Loved(loved) => self.state.loved = loved,
                Event::Storage(storage) => self.state.storage = Some(storage),
                Event::Outputs(outputs) => self.state.outputs = Loadable::Ready(outputs),
                Event::SearchResults { query, results } => self.search_results(query, results),
                Event::MoreSongs { query, result } => self.more_songs_read(query, result),
                Event::Discography { artist, result } => self.discography_read(artist, result),
                Event::SimilarArtists { artist, result } => {
                    self.similar_artists_read(artist, result);
                }
                Event::ArtistFound { name, result } => self.artist_found(name, result),
                Event::SongsFound { id, result } => self.songs_found(id, result),
                Event::Album { album, result } => self.album_read(album, result),
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
                Event::SyncNameTaken { list } => self.sync_name_taken(list),
                Event::SyncFolder(result) => self.sync_folder_read(result),
                Event::PlaylistChanged(changed) => self.playlist_changed(changed),
                Event::PlaylistsChanged => self.sync_on_connect(),
                Event::Error(message) => self.state.error = Some(message),
                Event::Cover { url, result } => theirs.push(Cover { url, image: result }),
            }
        }
        theirs
    }

    fn session_changed(&mut self, session: SessionState) {
        self.state.error = None;
        // The engine is gone with the session. Before a song is loaded (as
        // at the start) there is nothing to stop, and where Play goes on
        // from is kept.
        if !matches!(session, SessionState::Ready { .. }) && self.state.transport.active() {
            self.state.transport.stop(&mut self.state.library);
            self.state.player = PlayerSnapshot::default();
        }
        let ready = matches!(session, SessionState::Ready { .. });
        let signed_out = matches!(session, SessionState::SignedOut)
            && !matches!(self.state.session, SessionState::SignedOut);
        self.state.session = session;
        if signed_out {
            self.forget_account();
        }
        if ready && self.state.browse.open {
            self.read_spotify_playlists();
        }
        if ready {
            self.sync_on_connect();
        }
    }

    /// Does what the interface asked for, in the part of the controller
    /// the action is for.
    pub fn apply(&mut self, action: impl Into<Action>) {
        match action.into() {
            Action::Settings(action) => self.apply_settings(action),
            Action::LastFm(action) => self.apply_lastfm(action),
            Action::Playback(action) => self.apply_playback(action),
            Action::Playlists(action) => self.apply_playlists(action),
            Action::Sync(action) => self.apply_sync(action),
            Action::View(action) => self.apply_view(action),
            Action::Browse(action) => self.apply_browse(action),
            Action::SpotifyLists(action) => self.apply_spotify_lists(action),
            Action::SpotifyTab(action) => self.apply_spotify_tab(action),
        }
    }

    /// Saves once changes have rested for a moment, writes synced
    /// playlists' positions once their song has played a few seconds, and
    /// gives up waiting for the queue to come here. Returns how long until the next of
    /// those is due, while one is.
    pub fn tick(&mut self) -> Option<Duration> {
        [self.starting_tick(), self.save_tick(), self.settle_tick()]
            .into_iter()
            .flatten()
            .min()
    }

    /// Reads a cover, from the disk cache or downloaded into it: it comes
    /// back from [`Self::receive`] once read. Ask once per URL.
    pub fn load_cover(&self, url: String) {
        self.send(Command::Cover(url));
    }

    fn send(&self, command: Command) {
        if let Some(backend) = &self.backend {
            backend.send(command);
        }
    }
}

/// What the audio outputs use, as the settings have it: they may open
/// as soon as the backend starts.
fn audio_link(settings: &Settings) -> Arc<AudioLink> {
    let audio = AudioLink::default();
    audio.set_equaliser(settings.equaliser);
    audio.set_output(settings.audio_output.clone());
    Arc::new(audio)
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
mod tests;
