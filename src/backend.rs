//! The bridge between the UI thread and asynchronous work.
//!
//! egui runs on the main thread and must never block. A tokio runtime on its
//! own thread hosts the session, the player and catalogue reads. The UI sends
//! [`Command`]s; the worker answers with [`Event`]s and wakes the UI with
//! `request_repaint`, so the app stays idle when nothing happens.

use std::sync::Arc;
use std::sync::mpsc;

use anyhow::Result;
use librespot_core::SpotifyUri;
use librespot_core::session::Session;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::model::catalog::{Discography, SearchResults};
use crate::model::{EntryId, PlaylistId, TrackRef};
use crate::paths::Paths;
use crate::playback::engine::{Engine, EngineEvent, PlayerSnapshot};
use crate::playback::outputs::{self, Output};
use crate::playback::transport::Request;
use crate::spotify::webapi::WebApi;
use crate::spotify::{catalog, session};
use crate::store::Settings;

#[derive(Debug)]
pub enum Command {
    SignIn,
    SignOut,
    /// Reads the songs of a track, album or artist link for a playlist.
    AddLink {
        text: String,
        playlist: PlaylistId,
    },
    Player(Request),
    Seek(u32),
    Volume(u16),
    Search(String),
    /// An artist's discography, by artist URI.
    Discography(String),
    /// An album's songs, by album URI.
    Album(String),
    /// An album cover, by image URL.
    Cover(String),
    /// The personal app search signs in with changed (None: no app).
    SearchApp(Option<String>),
    /// Volume levelling on or off.
    Levelling(bool),
    /// How much the caches hold; answered with `Event::Storage`.
    MeasureStorage,
    /// Empties a cache, then measures again.
    ClearCache(Store),
    /// The outputs to choose from; answered with `Event::Outputs`.
    ListOutputs,
    /// Plays to this output (None: the system's default).
    Output(Option<String>),
}

/// What Spotless keeps on disk that can be thrown away.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Store {
    Audio,
    Covers,
}

/// Bytes on disk per cache.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Storage {
    pub audio: u64,
    pub covers: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SearchState {
    /// No personal app is set up.
    NoApp,
    SignedOut,
    SigningIn,
    Ready,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SessionState {
    Connecting,
    SignedOut,
    /// Waiting for the browser approval.
    SigningIn,
    Ready {
        user: String,
    },
    Failed(String),
}

#[derive(Debug)]
pub enum Event {
    Session(SessionState),
    Player(PlayerSnapshot),
    /// The engine started the followed song by itself.
    Followed(EntryId),
    /// The engine finished a song with nothing to follow, and stopped.
    Ended,
    Unavailable,
    Search(SearchState),
    SearchResults {
        query: String,
        results: Result<SearchResults, String>,
    },
    Discography {
        artist: String,
        result: Result<Discography, String>,
    },
    Album {
        album: String,
        result: Result<Vec<TrackRef>, String>,
    },
    Cover {
        url: String,
        result: Result<egui::ColorImage, String>,
    },
    Storage(Storage),
    /// The outputs to choose from; None when they can't be listed.
    Outputs(Option<Vec<Output>>),
    /// The songs of an `AddLink`.
    Tracks {
        playlist: PlaylistId,
        tracks: Vec<TrackRef>,
    },
    Error(String),
}

pub struct Backend {
    commands: UnboundedSender<Command>,
    pub events: mpsc::Receiver<Event>,
}

impl Backend {
    pub fn start(paths: Paths, volume: u16, levelling: bool, ctx: egui::Context) -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("spotless-backend")
            .build()?;
        let (commands, receiver) = unbounded_channel();
        let (sender, events) = mpsc::channel();
        let emit: Emit = Arc::new(move |event| {
            let _ = sender.send(event);
            ctx.request_repaint();
        });
        std::thread::Builder::new()
            .name("spotless-runtime".into())
            .spawn(move || {
                runtime.block_on(Worker::new(paths, volume, levelling, emit).run(receiver));
            })?;
        Ok(Self { commands, events })
    }

    pub fn send(&self, command: Command) {
        if self.commands.send(command).is_err() {
            log::error!("the backend has stopped");
        }
    }
}

type Emit = Arc<dyn Fn(Event) + Send + Sync>;

/// Search signs in with the personal app, a second approval besides
/// playback's (see `spotify/webapi.rs`). It runs when Spotless signs in, or
/// on the first search if search was never approved. The browser is already
/// signed in to Spotify by then, so it usually passes without a question.
/// One at a time: a second caller waits, then finds it done.
async fn approve_search(
    web: &tokio::sync::Mutex<WebApi>,
    approval: &tokio::sync::Mutex<()>,
    emit: &Emit,
) -> Result<(), String> {
    let _one = approval.lock().await;
    if web.lock().await.signed_in() {
        return Ok(());
    }
    emit(Event::Search(SearchState::SigningIn));
    // Not holding the client: the approval waits as long as the user takes.
    let client = web.lock().await.approval().map_err(|e| format!("{e:#}"));
    let result = match client {
        Ok(client) => match client.get_access_token_async().await {
            Ok(token) => web.lock().await.keep(token).map_err(|e| format!("{e:#}")),
            Err(error) => Err(format!("search sign-in failed: {error}")),
        },
        Err(error) => Err(error),
    };
    emit(Event::Search(if result.is_ok() {
        SearchState::Ready
    } else {
        SearchState::SignedOut
    }));
    result
}

struct Worker {
    paths: Paths,
    volume: u16,
    levelling: bool,
    emit: Emit,
    session: Option<Session>,
    engine: Option<Engine>,
    /// Shared with the searches running in the background.
    web: Option<Arc<tokio::sync::Mutex<WebApi>>>,
    /// Held while search's browser approval runs, so it runs once.
    search_approval: Arc<tokio::sync::Mutex<()>>,
    http: reqwest::Client,
    /// Limits cover downloads running at once.
    cover_slots: Arc<tokio::sync::Semaphore>,
}

impl Worker {
    fn new(paths: Paths, volume: u16, levelling: bool, emit: Emit) -> Self {
        Self {
            paths,
            volume,
            levelling,
            emit,
            session: None,
            engine: None,
            web: None,
            search_approval: Arc::default(),
            http: reqwest::Client::new(),
            cover_slots: Arc::new(tokio::sync::Semaphore::new(6)),
        }
    }

    async fn run(mut self, mut commands: UnboundedReceiver<Command>) {
        let settings = Settings::load(&self.paths);
        outputs::restore(settings.audio_output.clone());
        self.set_search_app(settings.web_client_id());
        (self.emit)(Event::Session(SessionState::Connecting));
        match session::restore(&self.paths).await {
            Ok(Some(session)) => self.ready(session),
            Ok(None) => (self.emit)(Event::Session(SessionState::SignedOut)),
            Err(error) => (self.emit)(Event::Session(SessionState::Failed(format!("{error:#}")))),
        }
        while let Some(command) = commands.recv().await {
            self.handle(command).await;
        }
    }

    /// Search signs in with this personal app from now on.
    fn set_search_app(&mut self, client_id: Option<String>) {
        let web = client_id.map(|id| WebApi::new(id, self.paths.search_credentials()));
        (self.emit)(Event::Search(match &web {
            None => SearchState::NoApp,
            Some(web) if web.signed_in() => SearchState::Ready,
            Some(_) => SearchState::SignedOut,
        }));
        self.web = web.map(|web| Arc::new(tokio::sync::Mutex::new(web)));
    }

    /// Measures the caches in the background.
    fn measure_storage(&self) {
        let emit = Arc::clone(&self.emit);
        let paths = self.paths.clone();
        tokio::task::spawn_blocking(move || emit(Event::Storage(measure(&paths))));
    }

    /// Runs search's browser approval now, unless it was given already.
    fn approve_search_now(&self) {
        let Some(web) = self.web.clone() else { return };
        let approval = Arc::clone(&self.search_approval);
        let emit = Arc::clone(&self.emit);
        tokio::spawn(async move {
            if let Err(error) = approve_search(&web, &approval, &emit).await {
                emit(Event::Error(error));
            }
        });
    }

    fn ready(&mut self, session: Session) {
        let emit = Arc::clone(&self.emit);
        match Engine::new(
            &session,
            self.volume,
            self.levelling,
            Arc::new(move |event| {
                emit(match event {
                    EngineEvent::Snapshot(snapshot) => Event::Player(snapshot),
                    EngineEvent::Followed(entry) => Event::Followed(entry),
                    EngineEvent::Ended => Event::Ended,
                    EngineEvent::Unavailable => Event::Unavailable,
                    EngineEvent::OutputError(message) => Event::Error(message),
                })
            }),
        ) {
            Ok(engine) => {
                (self.emit)(Event::Session(SessionState::Ready {
                    user: session.username(),
                }));
                self.engine = Some(engine);
                self.session = Some(session);
            }
            Err(error) => (self.emit)(Event::Session(SessionState::Failed(format!("{error:#}")))),
        }
    }

    async fn handle(&mut self, command: Command) {
        match command {
            Command::SignIn => {
                (self.emit)(Event::Session(SessionState::SigningIn));
                match session::sign_in(&self.paths).await {
                    Ok(session) => {
                        self.ready(session);
                        // Search's approval follows at once, as one sign-in.
                        self.approve_search_now();
                    }
                    Err(error) => {
                        (self.emit)(Event::Session(SessionState::Failed(format!("{error:#}"))))
                    }
                }
            }
            Command::SignOut => {
                if let Some(engine) = self.engine.take() {
                    engine.stop();
                }
                if let Some(session) = self.session.take() {
                    session.shutdown();
                }
                session::sign_out(&self.paths);
                (self.emit)(Event::Player(PlayerSnapshot::default()));
                (self.emit)(Event::Session(SessionState::SignedOut));
            }
            Command::AddLink { text, playlist } => {
                let Some(session) = self.session.clone() else {
                    return;
                };
                let emit = Arc::clone(&self.emit);
                // Album reads take a moment; commands go on meanwhile.
                tokio::spawn(async move {
                    match catalog::link_tracks(&session, &text).await {
                        Ok(tracks) => emit(Event::Tracks { playlist, tracks }),
                        Err(error) => emit(Event::Error(format!("{error:#}"))),
                    }
                });
            }
            Command::Player(request) => self.player(request),
            Command::MeasureStorage => self.measure_storage(),
            Command::ListOutputs => {
                let emit = Arc::clone(&self.emit);
                tokio::task::spawn_blocking(move || emit(Event::Outputs(outputs::list())));
            }
            Command::Output(output) => outputs::choose(output),
            Command::ClearCache(store) => {
                let dir = match store {
                    Store::Audio => self.paths.audio_cache(),
                    Store::Covers => self.paths.covers(),
                };
                let emit = Arc::clone(&self.emit);
                let paths = self.paths.clone();
                tokio::task::spawn_blocking(move || {
                    if let Err(error) = clear_dir(&dir) {
                        log::error!("unable to clear {}: {error}", dir.display());
                        emit(Event::Error("Couldn't clear all of the cache.".into()));
                    }
                    emit(Event::Storage(measure(&paths)));
                });
            }
            Command::Levelling(on) => {
                self.levelling = on;
                if let Some(engine) = &mut self.engine {
                    engine.set_levelling(on);
                }
            }
            Command::SearchApp(client_id) => {
                // The saved approval belongs to the old app.
                let _ = std::fs::remove_file(self.paths.search_credentials());
                self.set_search_app(client_id);
                // Signed in already: approve now, as part of setting it up.
                if self.session.is_some() {
                    self.approve_search_now();
                }
            }
            Command::Search(query) => {
                let Some(web) = self.web.clone() else { return };
                let approval = Arc::clone(&self.search_approval);
                let emit = Arc::clone(&self.emit);
                tokio::spawn(async move {
                    // A saved approval Spotify no longer takes is dropped by
                    // the search; approve again and retry once.
                    let mut results = Err(String::new());
                    for _ in 0..2 {
                        if let Err(error) = approve_search(&web, &approval, &emit).await {
                            results = Err(error);
                            break;
                        }
                        let mut web = web.lock().await;
                        results = web.search(&query).await.map_err(|e| format!("{e:#}"));
                        if results.is_ok() || web.signed_in() {
                            break;
                        }
                        emit(Event::Search(SearchState::SignedOut));
                    }
                    emit(Event::SearchResults { query, results });
                });
            }
            Command::Discography(artist) => {
                let Some(session) = self.session.clone() else {
                    return;
                };
                let emit = Arc::clone(&self.emit);
                tokio::spawn(async move {
                    let result = match catalog::parse_uri(&artist) {
                        Ok(uri) => catalog::discography(&session, &uri).await,
                        Err(error) => Err(error),
                    };
                    let result = result.map_err(|e| format!("{e:#}"));
                    emit(Event::Discography { artist, result });
                });
            }
            Command::Cover(url) => {
                let http = self.http.clone();
                let dir = self.paths.covers();
                let slots = Arc::clone(&self.cover_slots);
                let emit = Arc::clone(&self.emit);
                tokio::spawn(async move {
                    let _slot = slots.acquire().await;
                    let result = crate::images::cover(&http, &dir, &url)
                        .await
                        .map_err(|e| format!("{e:#}"));
                    if let Err(error) = &result {
                        log::warn!("cover {url}: {error}");
                    }
                    emit(Event::Cover { url, result });
                });
            }
            Command::Album(album) => {
                let Some(session) = self.session.clone() else {
                    return;
                };
                let emit = Arc::clone(&self.emit);
                tokio::spawn(async move {
                    let result = catalog::album(&session, &album)
                        .await
                        .map_err(|e| format!("{e:#}"));
                    emit(Event::Album { album, result });
                });
            }
            Command::Seek(position) => self.with_engine(|e| e.seek(position)),
            Command::Volume(volume) => {
                self.volume = volume;
                self.with_engine(|e| e.set_volume(volume));
            }
        }
    }

    fn player(&self, request: Request) {
        let Some(engine) = &self.engine else { return };
        let uri = |text: &str| {
            SpotifyUri::from_uri(text)
                .map_err(|error| log::error!("not a Spotify URI {text:?}: {error}"))
                .ok()
        };
        match request {
            Request::Load {
                uri: text,
                position_ms,
                ..
            } => match uri(&text) {
                Some(track) => engine.load(track, position_ms),
                None => (self.emit)(Event::Unavailable),
            },
            Request::Follow(next) => {
                engine.follow(next.and_then(|(entry, text)| Some((entry, uri(&text)?))));
            }
            Request::Resume => engine.play(),
            Request::Pause => engine.pause(),
            Request::Restart => engine.seek(0),
            Request::Stop => engine.stop(),
        }
    }

    fn with_engine(&self, action: impl FnOnce(&Engine)) {
        if let Some(engine) = &self.engine {
            action(engine);
        }
    }
}

fn measure(paths: &Paths) -> Storage {
    Storage {
        audio: dir_size(&paths.audio_cache()),
        covers: dir_size(&paths.covers()),
    }
}

/// The bytes in a folder and its subfolders; a missing folder holds none.
fn dir_size(dir: &std::path::Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => dir_size(&entry.path()),
            Ok(kind) if kind.is_file() => entry.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

/// Removes what a folder holds, keeping the folder.
fn clear_dir(dir: &std::path::Path) -> std::io::Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            std::fs::remove_dir_all(entry.path())?;
        } else {
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{clear_dir, dir_size};

    #[test]
    fn a_cleared_folder_is_empty_and_kept() {
        let dir = std::env::temp_dir().join(format!("spotless-clear-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("ab")).unwrap();
        std::fs::write(dir.join("ab/cd"), [0; 300]).unwrap();
        std::fs::write(dir.join("ef"), [0; 200]).unwrap();
        assert_eq!(dir_size(&dir), 500);
        clear_dir(&dir).unwrap();
        assert_eq!(dir_size(&dir), 0);
        assert!(dir.is_dir());
        assert!(clear_dir(&dir.join("missing")).is_ok());
    }
}
