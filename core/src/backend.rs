//! The bridge between the UI thread and asynchronous work.
//!
//! The UI runs on the main thread and must never block. A tokio runtime on
//! its own thread hosts the session, the player and catalogue reads. The UI
//! sends [`Command`]s; the worker answers with [`Event`]s and wakes the UI
//! with the callback it was started with (egui's `request_repaint` on the
//! desktop), so the app stays idle when nothing happens.

use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::Result;
use librespot_core::SpotifyUri;
use librespot_core::session::Session;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::time::MissedTickBehavior;

use crate::browser::Browser;
use crate::images::CoverImage;
use crate::lastfm::{self, LastFmState, Scrobbler, api::Keys};
use crate::model::catalog::{Discography, SearchResults};
use crate::model::{ArtistRef, EntryId, PlaylistId, TrackRef};
use crate::paths::Paths;
use crate::playback::engine::{Engine, EngineEvent, PlayerSnapshot};
use crate::playback::outputs::{self, Output};
use crate::playback::transport::Request;
use crate::spotify::devices::{self, Playing, Watch};
use crate::spotify::queue::SpotifyQueue;
use crate::spotify::remote::Remote;
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
    /// Artists like this one, from Last.fm with this API key.
    SimilarArtists {
        artist: ArtistRef,
        api_key: String,
    },
    /// The Spotify artist with this name, for a similar artist opened.
    FindArtist(String),
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
    /// The system has a network again: a dropped session reconnects now,
    /// not after the wait since the last try.
    NetworkBack,
    /// Empties a cache, then measures again.
    ClearCache(Store),
    /// The outputs to choose from; answered with `Event::Outputs`.
    ListOutputs,
    /// Plays to this output (None: the system's default).
    Output(Option<String>),
    /// Saves this Last.fm API account and approves it in the browser.
    LastFmConnect(Keys),
    LastFmDisconnect,
    /// For the Spotify tab.
    Spotify(SpotifyRequest),
}

/// What the Spotify tab asks of the account's queue.
#[derive(Debug)]
pub enum SpotifyRequest {
    Play,
    Pause,
    TogglePause,
    Next,
    Previous,
    Seek(u32),
    /// Adds songs, or an album, by URI to the end of the queue.
    Add(Vec<String>),
    /// Removes songs to come, by their place in Spotify's list and URI.
    Remove(Vec<(usize, String)>),
    /// Takes playback over from the device playing the queue.
    PlayHere,
}

/// The Spotify tab is on: Spotless joins Spotify Connect as a device and
/// shows the account's queue. Not on Android yet.
pub const SPOTIFY_TAB: bool = !cfg!(target_os = "android");

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
    /// Another of the account's devices started playing, by name, so
    /// Spotless paused: Spotify allows one at a time.
    PlayingElsewhere(String),
    /// Spotless started playing and paused the other device, by name.
    PausedElsewhere(String),
    /// The account's queue, for the Spotify tab; None while Spotless isn't
    /// in Spotify Connect.
    Spotify(Option<SpotifyQueue>),
    /// The loaded song can't be played; `offline`: for want of a
    /// connection.
    Unavailable {
        offline: bool,
    },
    /// The playing song stopped short here for want of a connection.
    Cut {
        position_ms: u32,
    },
    /// The session dropped and a new one took its place.
    Reconnected,
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
    /// Similar artists' names, for an artist's URI.
    SimilarArtists {
        artist: String,
        result: Result<Vec<String>, String>,
    },
    /// The artist a `FindArtist` name is on Spotify (None: none by it).
    ArtistFound {
        name: String,
        result: Result<Option<ArtistRef>, String>,
    },
    Cover {
        url: String,
        result: Result<CoverImage, String>,
    },
    Storage(Storage),
    LastFm(LastFmState),
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
    /// `wake` is called after every event, from the worker's thread.
    /// `browser` opens the approval pages.
    pub fn start(
        paths: Paths,
        volume: u16,
        levelling: bool,
        browser: Browser,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("spotless-backend")
            .build()?;
        let (commands, receiver) = unbounded_channel();
        let (sender, events) = mpsc::channel();
        let emit: Emit = Arc::new(move |event| {
            let _ = sender.send(event);
            wake();
        });
        std::thread::Builder::new()
            .name("spotless-runtime".into())
            .spawn(move || {
                runtime
                    .block_on(Worker::new(paths, volume, levelling, browser, emit).run(receiver));
            })?;
        Ok(Self { commands, events })
    }

    /// A backend with no worker, for tests: the commands it is sent come out
    /// of the receiver, and the events put in the sender come in.
    #[cfg(test)]
    pub fn detached() -> (Self, UnboundedReceiver<Command>, mpsc::Sender<Event>) {
        let (commands, receiver) = unbounded_channel();
        let (sender, events) = mpsc::channel();
        (Self { commands, events }, receiver, sender)
    }

    pub fn send(&self, command: Command) {
        if self.commands.send(command).is_err() {
            log::error!("the backend has stopped");
        }
    }
}

type Emit = Arc<dyn Fn(Event) + Send + Sync>;

/// What the Spotify tab's device tells the worker.
enum RemoteReport {
    Started(Result<Remote>),
    Queue(SpotifyQueue),
}

/// How often the session is checked for having dropped. librespot notices
/// a lost connection itself, within about a minute and a half, but says
/// nothing.
const CHECK_SESSION: Duration = Duration::from_secs(10);
/// How long connecting may take before it is tried again later.
const RECONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// The waits between connections that can't reach Spotify, doubling.
const RETRY_FIRST: Duration = Duration::from_secs(5);
const RETRY_MOST: Duration = Duration::from_secs(120);

/// Search signs in with the personal app, a second approval besides
/// playback's (see `spotify/webapi.rs`). It runs when Spotless signs in, or
/// on the first search if search was never approved. The browser is already
/// signed in to Spotify by then, so it usually passes without a question.
/// One at a time: a second caller waits, then finds it done.
async fn approve_search(
    web: &tokio::sync::Mutex<WebApi>,
    approval: &tokio::sync::Mutex<()>,
    browser: &Browser,
    emit: &Emit,
) -> Result<(), String> {
    let _one = approval.lock().await;
    if web.lock().await.signed_in() {
        return Ok(());
    }
    emit(Event::Search(SearchState::SigningIn));
    // Not holding the client: the approval waits as long as the user takes.
    let client = web
        .lock()
        .await
        .approval(browser)
        .map_err(|e| format!("{e:#}"));
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
    browser: Browser,
    emit: Emit,
    session: Option<Session>,
    engine: Option<Engine>,
    /// Follows the player and scrobbles, once `run` has started it.
    scrobbler: Option<Scrobbler>,
    /// Follows the account's other devices on the session.
    watch: Option<Watch>,
    /// The other device playing, as the watch last told.
    elsewhere: Option<Playing>,
    /// Where the watches tell, and where `run` hears it.
    devices: UnboundedSender<Option<Playing>>,
    device_reports: Option<UnboundedReceiver<Option<Playing>>>,
    /// Spotless's Connect device for the Spotify tab, once started.
    remote: Option<Remote>,
    /// A remote is being started.
    remote_starting: bool,
    /// The queue as the remote last told.
    queue: SpotifyQueue,
    /// Where remotes are handed over once started, and tell the queue.
    remotes: UnboundedSender<RemoteReport>,
    remote_reports: Option<UnboundedReceiver<RemoteReport>>,
    /// Shared with the searches running in the background.
    web: Option<Arc<tokio::sync::Mutex<WebApi>>>,
    /// Held while search's browser approval runs, so it runs once.
    search_approval: Arc<tokio::sync::Mutex<()>>,
    http: reqwest::Client,
    /// Limits cover downloads running at once.
    cover_slots: Arc<tokio::sync::Semaphore>,
    /// Connecting on the saved credential at start-up, until Spotify answers.
    starting: bool,
    /// After a failed connection: no new one before then.
    retry_at: Option<Instant>,
    /// The wait after the next failed connection.
    retry_wait: Duration,
}

impl Worker {
    fn new(paths: Paths, volume: u16, levelling: bool, browser: Browser, emit: Emit) -> Self {
        let (devices, device_reports) = unbounded_channel();
        let (remotes, remote_reports) = unbounded_channel();
        Self {
            paths,
            volume,
            levelling,
            browser,
            emit,
            session: None,
            engine: None,
            scrobbler: None,
            watch: None,
            elsewhere: None,
            devices,
            device_reports: Some(device_reports),
            remote: None,
            remote_starting: false,
            queue: SpotifyQueue::default(),
            remotes,
            remote_reports: Some(remote_reports),
            web: None,
            search_approval: Arc::default(),
            http: reqwest::Client::new(),
            cover_slots: Arc::new(tokio::sync::Semaphore::new(6)),
            starting: false,
            retry_at: None,
            retry_wait: RETRY_FIRST,
        }
    }

    async fn run(mut self, mut commands: UnboundedReceiver<Command>) {
        let settings = Settings::load(&self.paths);
        outputs::restore(settings.audio_output.clone());
        self.set_search_app(settings.web_client_id());
        let emit = Arc::clone(&self.emit);
        self.scrobbler = Some(lastfm::start(
            &self.paths,
            Arc::clone(&self.browser),
            Arc::new(move |report| {
                emit(match report {
                    lastfm::Report::State(state) => Event::LastFm(state),
                    lastfm::Report::Error(message) => Event::Error(message),
                })
            }),
        ));
        (self.emit)(Event::Session(SessionState::Connecting));
        self.starting = true;
        self.reconnect_if_needed().await;
        let mut check = tokio::time::interval(CHECK_SESSION);
        check.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut device_reports = self.device_reports.take().expect("run once");
        let mut remote_reports = self.remote_reports.take().expect("run once");
        loop {
            tokio::select! {
                command = commands.recv() => {
                    let Some(command) = command else { break };
                    if matches!(command, Command::NetworkBack) {
                        self.retry_at = None;
                        self.retry_wait = RETRY_FIRST;
                    }
                    // Signing in or out makes a session dropped or not.
                    if !matches!(command, Command::SignIn | Command::SignOut) {
                        self.reconnect_if_needed().await;
                    }
                    self.handle(command).await;
                }
                _ = check.tick() => {
                    self.reconnect_if_needed().await;
                    self.restart_remote_if_needed();
                }
                Some(playing) = device_reports.recv() => self.elsewhere_changed(playing),
                Some(report) = remote_reports.recv() => self.remote_report(report),
            }
        }
    }

    /// Connects on the saved credential at start-up, or replaces a session
    /// Spotify dropped (the network went, or the computer slept) with a new
    /// one and moves the engine onto it, going on with the song. While
    /// Spotify can't be reached it tries again, waiting longer each time,
    /// and the interface stays connecting or signed in, unless Spotify
    /// refuses the credential.
    async fn reconnect_if_needed(&mut self) {
        let dropped = self.session.as_ref().is_some_and(Session::is_invalid);
        if !(dropped || self.starting) || self.retry_at.is_some_and(|at| Instant::now() < at) {
            return;
        }
        if dropped {
            log::info!("the session dropped; reconnecting");
        }
        let restored = tokio::time::timeout(RECONNECT_TIMEOUT, session::restore(&self.paths))
            .await
            .unwrap_or_else(|_| Err(anyhow::anyhow!("Spotify took too long to answer")));
        match restored {
            Ok(Some(session)) if self.starting => {
                self.starting = false;
                self.ready(session);
            }
            Ok(Some(session)) => {
                log::info!("reconnected");
                self.retry_at = None;
                self.retry_wait = RETRY_FIRST;
                if let Some(engine) = &mut self.engine {
                    engine.set_session(&session);
                }
                self.watch(&session);
                self.session = Some(session);
                (self.emit)(Event::Reconnected);
            }
            // The credential was removed meanwhile.
            Ok(None) => self.drop_session(SessionState::SignedOut),
            Err(error) if session::refused(&error) => {
                log::warn!("connecting refused: {error:#}");
                self.drop_session(SessionState::Failed(format!("{error:#}")));
            }
            Err(error) => {
                log::warn!(
                    "unable to connect, trying again in {}s: {error:#}",
                    self.retry_wait.as_secs()
                );
                self.retry_at = Some(Instant::now() + self.retry_wait);
                self.retry_wait = (self.retry_wait * 2).min(RETRY_MOST);
            }
        }
    }

    /// Stops playing and lets the session go, then tells the interface why.
    fn drop_session(&mut self, why: SessionState) {
        if let Some(engine) = self.engine.take() {
            engine.stop();
        }
        self.unwatch();
        self.stop_remote();
        if let Some(session) = self.session.take() {
            session.shutdown();
        }
        self.starting = false;
        self.retry_at = None;
        self.retry_wait = RETRY_FIRST;
        (self.emit)(Event::Player(PlayerSnapshot::default()));
        (self.emit)(Event::Session(why));
    }

    /// Follows the other devices on `session` from now on.
    fn watch(&mut self, session: &Session) {
        self.unwatch();
        let devices = self.devices.clone();
        match Watch::start(session, move |playing| {
            let _ = devices.send(playing);
        }) {
            Ok(watch) => self.watch = Some(watch),
            Err(error) => log::warn!("unable to watch the other devices: {error:#}"),
        }
    }

    fn unwatch(&mut self) {
        if let Some(watch) = self.watch.take() {
            tokio::spawn(watch.close());
        }
        self.elsewhere = None;
    }

    /// Pauses when another device starts playing.
    fn elsewhere_changed(&mut self, playing: Option<Playing>) {
        let started = playing.as_ref().filter(|now| {
            self.elsewhere
                .as_ref()
                .is_none_or(|before| before.device != now.device)
        });
        if let (Some(other), Some(engine)) = (started, &self.engine)
            && engine.playing()
        {
            engine.pause();
            // The Spotify tab playing needs no word.
            if !self.is_remote(&other.device) {
                (self.emit)(Event::PlayingElsewhere(other.name.clone()));
            }
        }
        self.elsewhere = playing;
    }

    /// Pauses the other device playing, if one is, as Spotless starts. The
    /// Spotify tab lets playback go instead, so the player is local again.
    fn pause_elsewhere(&self) {
        if let Some(remote) = &self.remote
            && (self.queue.playing_here()
                || self
                    .elsewhere
                    .as_ref()
                    .is_some_and(|o| self.is_remote(&o.device)))
        {
            remote.leave();
            return;
        }
        let (Some(other), Some(session)) = (self.elsewhere.clone(), self.session.clone()) else {
            return;
        };
        let emit = Arc::clone(&self.emit);
        tokio::spawn(async move {
            match devices::pause(&session, &other.device).await {
                Ok(()) => emit(Event::PausedElsewhere(other.name)),
                Err(error) => {
                    log::warn!("unable to pause {}: {error:#}", other.name);
                    emit(Event::Error(format!(
                        "Couldn't pause Spotify on {}.",
                        other.name
                    )));
                }
            }
        });
    }

    fn is_remote(&self, device: &str) -> bool {
        self.remote
            .as_ref()
            .is_some_and(|r| r.device_id() == device)
    }

    /// Starts the Spotify tab's device, unless it runs or is starting.
    fn start_remote(&mut self) {
        if !SPOTIFY_TAB || self.remote.is_some() || self.remote_starting || self.session.is_none() {
            return;
        }
        self.remote_starting = true;
        let paths = self.paths.clone();
        let (volume, levelling) = (self.volume, self.levelling);
        let remotes = self.remotes.clone();
        tokio::spawn(async move {
            let queues = remotes.clone();
            let report = move |queue| {
                let _ = queues.send(RemoteReport::Queue(queue));
            };
            let started = Remote::start(&paths, volume, levelling, report).await;
            let _ = remotes.send(RemoteReport::Started(started));
        });
    }

    /// Starts the Spotify tab's device again when Spotify dropped its
    /// session.
    fn restart_remote_if_needed(&mut self) {
        if self.remote.as_ref().is_some_and(Remote::is_invalid) {
            log::info!("the Spotify tab's session dropped; joining again");
            self.stop_remote();
        }
        self.start_remote();
    }

    fn stop_remote(&mut self) {
        if let Some(remote) = self.remote.take() {
            remote.shut_down();
            (self.emit)(Event::Spotify(None));
        }
        self.queue = SpotifyQueue::default();
    }

    fn remote_report(&mut self, report: RemoteReport) {
        match report {
            RemoteReport::Started(Ok(remote)) => {
                self.remote_starting = false;
                // Signed out meanwhile.
                if self.session.is_none() {
                    remote.shut_down();
                    return;
                }
                self.remote = Some(remote);
                (self.emit)(Event::Spotify(Some(self.queue.clone())));
            }
            RemoteReport::Started(Err(error)) => {
                self.remote_starting = false;
                log::warn!("unable to start the Spotify tab: {error:#}");
            }
            RemoteReport::Queue(queue) => {
                if self.remote.is_none() && !self.remote_starting {
                    return;
                }
                self.queue = queue.clone();
                (self.emit)(Event::Spotify(Some(queue)));
            }
        }
    }

    fn spotify(&self, request: SpotifyRequest) {
        let Some(remote) = &self.remote else { return };
        match request {
            SpotifyRequest::Play => remote.play(),
            SpotifyRequest::Pause => remote.pause(),
            SpotifyRequest::TogglePause => remote.toggle_pause(),
            SpotifyRequest::Next => remote.next(),
            SpotifyRequest::Previous => remote.previous(),
            SpotifyRequest::Seek(position) => remote.seek(position),
            SpotifyRequest::Add(uris) => remote.add(&uris),
            SpotifyRequest::Remove(songs) => remote.remove(songs),
            SpotifyRequest::PlayHere => remote.play_here(),
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
        let browser = Arc::clone(&self.browser);
        let emit = Arc::clone(&self.emit);
        tokio::spawn(async move {
            if let Err(error) = approve_search(&web, &approval, &browser, &emit).await {
                emit(Event::Error(error));
            }
        });
    }

    fn ready(&mut self, session: Session) {
        let emit = Arc::clone(&self.emit);
        let scrobbler = self.scrobbler.clone();
        match Engine::new(
            &session,
            self.volume,
            self.levelling,
            Arc::new(move |event| {
                emit(match event {
                    EngineEvent::Snapshot(snapshot) => {
                        if let Some(scrobbler) = &scrobbler {
                            scrobbler.snapshot(snapshot.clone());
                        }
                        Event::Player(snapshot)
                    }
                    EngineEvent::Followed(entry) => Event::Followed(entry),
                    EngineEvent::Ended => Event::Ended,
                    EngineEvent::Unavailable { offline } => Event::Unavailable { offline },
                    EngineEvent::Cut { position_ms } => Event::Cut { position_ms },
                    EngineEvent::OutputError(message) => Event::Error(message),
                })
            }),
        ) {
            Ok(engine) => {
                (self.emit)(Event::Session(SessionState::Ready {
                    user: session.username(),
                }));
                self.engine = Some(engine);
                self.watch(&session);
                self.start_remote();
                self.session = Some(session);
                self.retry_at = None;
                self.retry_wait = RETRY_FIRST;
            }
            Err(error) => (self.emit)(Event::Session(SessionState::Failed(format!("{error:#}")))),
        }
    }

    async fn handle(&mut self, command: Command) {
        match command {
            Command::SignIn => {
                self.starting = false;
                (self.emit)(Event::Session(SessionState::SigningIn));
                match session::sign_in(&self.paths, &self.browser).await {
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
                session::sign_out(&self.paths);
                self.drop_session(SessionState::SignedOut);
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
            // Reconnected already, before any command.
            Command::NetworkBack => {}
            Command::ListOutputs => {
                let emit = Arc::clone(&self.emit);
                tokio::task::spawn_blocking(move || emit(Event::Outputs(outputs::list())));
            }
            Command::Output(output) => outputs::choose(output),
            Command::LastFmConnect(keys) => {
                if let Some(scrobbler) = &self.scrobbler {
                    scrobbler.connect(keys);
                }
            }
            Command::LastFmDisconnect => {
                if let Some(scrobbler) = &self.scrobbler {
                    scrobbler.disconnect();
                }
            }
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
                let browser = Arc::clone(&self.browser);
                let emit = Arc::clone(&self.emit);
                tokio::spawn(async move {
                    // A saved approval Spotify no longer takes is dropped by
                    // the search; approve again and retry once.
                    let mut results = Err(String::new());
                    for _ in 0..2 {
                        if let Err(error) = approve_search(&web, &approval, &browser, &emit).await {
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
            Command::SimilarArtists { artist, api_key } => {
                let http = self.http.clone();
                let emit = Arc::clone(&self.emit);
                tokio::spawn(async move {
                    let result = lastfm::api::similar_artists(&http, &api_key, &artist.name)
                        .await
                        .map_err(|e| e.to_string());
                    emit(Event::SimilarArtists {
                        artist: artist.uri,
                        result,
                    });
                });
            }
            Command::FindArtist(name) => {
                let Some(web) = self.web.clone() else { return };
                let approval = Arc::clone(&self.search_approval);
                let browser = Arc::clone(&self.browser);
                let emit = Arc::clone(&self.emit);
                tokio::spawn(async move {
                    let result = match approve_search(&web, &approval, &browser, &emit).await {
                        Ok(()) => web
                            .lock()
                            .await
                            .find_artist(&name)
                            .await
                            .map_err(|e| format!("{e:#}")),
                        Err(error) => Err(error),
                    };
                    emit(Event::ArtistFound { name, result });
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
                if let Some(remote) = &self.remote {
                    remote.set_volume(volume);
                }
            }
            Command::Spotify(request) => self.spotify(request),
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
                Some(track) => {
                    self.pause_elsewhere();
                    engine.load(track, position_ms);
                }
                None => (self.emit)(Event::Unavailable { offline: false }),
            },
            Request::Follow(next) => {
                engine.follow(next.and_then(|(entry, text)| Some((entry, uri(&text)?))));
            }
            Request::Resume => {
                self.pause_elsewhere();
                engine.play();
            }
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
