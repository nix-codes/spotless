//! The worker: the tokio task that holds the session, the player and
//! Spotify Connect, takes the controller's [`Command`]s and answers with
//! [`Event`]s.
//!
//! This file keeps the session: signing in and out, connecting at the
//! start and again when it drops. What each command does is in the file
//! of its part: `player`, `catalog`, `spotify_lists`, `sync`, `storage`
//! and `connect` (the Spotify tab).

use std::sync::Arc;
use std::time::{Duration, Instant};

use librespot_core::session::Session;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::time::MissedTickBehavior;

mod catalog;
mod connect;
mod player;
mod retry;
mod spotify_lists;
mod storage;
mod sync;

use super::{Command, Emit, Event, SessionState};
use crate::browser::Browser;
use crate::lastfm::{self, Scrobbler};
use crate::paths::Paths;
use crate::playback::engine::{Engine, EngineEvent, PlayerSnapshot};
use crate::playback::outputs;
use crate::spotify::devices::{Playing, Watch};
use crate::spotify::queue::SpotifyQueue;
use crate::spotify::remote::Remote;
use crate::spotify::webapi::WebApi;
use crate::spotify::{session, synced};
use crate::store::Settings;
use connect::{Adding, Clearing, PlayHere, RemoteReport, SEEK_GAP, TakingOver};
use retry::Retry;

/// How often the session is checked for having dropped. librespot notices
/// a lost connection itself, within about a minute and a half, but says
/// nothing.
const CHECK_SESSION: Duration = Duration::from_secs(10);
/// How long connecting may take before it is tried again later.
const RECONNECT_TIMEOUT: Duration = Duration::from_secs(20);

pub(super) struct Worker {
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
    /// Tells of changes to the account's playlists, on the session.
    playlist_changes: Option<tokio::task::JoinHandle<()>>,
    /// Tells of changes to the account's list of playlists, on the session.
    list_changes: Option<tokio::task::JoinHandle<()>>,
    /// The other device playing, as the watch last told.
    elsewhere: Option<Playing>,
    /// Where the watches tell, and where `run` hears it.
    devices: UnboundedSender<Option<Playing>>,
    device_reports: Option<UnboundedReceiver<Option<Playing>>>,
    /// The Spotify tab is open: its device runs while signed in.
    spotify_tab: bool,
    /// Spotless's Connect device for the Spotify tab, once started.
    remote: Option<Remote>,
    /// A remote is being started.
    remote_starting: bool,
    /// The queue as the remote last told.
    queue: SpotifyQueue,
    /// The queue asked to play here, done step by step as it reports.
    play_here: Option<PlayHere>,
    /// When the queue was asked to be cleared, while it comes here to be.
    clearing: Option<Clearing>,
    /// Songs to add once the queue, held by no device, comes here.
    adding: Option<Adding>,
    /// The queue asked to be taken over, while it comes here paused.
    taking_over: Option<TakingOver>,
    /// When the queue was last asked to seek, and the seek waiting for
    /// seeking to pause.
    seek_asked: Option<Instant>,
    seek_waiting: Option<u32>,
    /// Where remotes are handed over once started, and tell the queue.
    remotes: UnboundedSender<RemoteReport>,
    remote_reports: Option<UnboundedReceiver<RemoteReport>>,
    /// Shared with the searches running in the background.
    web: Option<Arc<tokio::sync::Mutex<WebApi>>>,
    /// Held while search's browser approval runs, so it runs once.
    search_approval: Arc<tokio::sync::Mutex<()>>,
    /// Loved songs found on Spotify so far.
    found: Arc<tokio::sync::Mutex<lastfm::found::Found>>,
    http: reqwest::Client,
    /// Limits cover downloads running at once.
    cover_slots: Arc<tokio::sync::Semaphore>,
    /// When to connect to Spotify, at the start and after a drop.
    retry: Retry,
}

impl Worker {
    pub(super) fn new(
        paths: Paths,
        volume: u16,
        levelling: bool,
        spotify_tab: bool,
        browser: Browser,
        emit: Emit,
    ) -> Self {
        let (devices, device_reports) = unbounded_channel();
        let (remotes, remote_reports) = unbounded_channel();
        let found = Arc::new(tokio::sync::Mutex::new(lastfm::found::Found::load(&paths)));
        Self {
            paths,
            volume,
            levelling,
            spotify_tab,
            browser,
            emit,
            session: None,
            engine: None,
            scrobbler: None,
            watch: None,
            playlist_changes: None,
            list_changes: None,
            elsewhere: None,
            devices,
            device_reports: Some(device_reports),
            remote: None,
            remote_starting: false,
            play_here: None,
            clearing: None,
            adding: None,
            taking_over: None,
            seek_asked: None,
            seek_waiting: None,
            queue: SpotifyQueue::default(),
            remotes,
            remote_reports: Some(remote_reports),
            web: None,
            search_approval: Arc::default(),
            found,
            http: reqwest::Client::new(),
            cover_slots: Arc::new(tokio::sync::Semaphore::new(6)),
            retry: Retry::new(),
        }
    }

    pub(super) async fn run(mut self, mut commands: UnboundedReceiver<Command>) {
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
                    lastfm::Report::Loved(loved) => Event::Loved(loved),
                    lastfm::Report::Error(message) => Event::Error(message),
                })
            }),
        ));
        (self.emit)(Event::Session(SessionState::Connecting));
        self.retry.start();
        self.reconnect_if_needed().await;
        let mut check = tokio::time::interval(CHECK_SESSION);
        check.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut device_reports = self.device_reports.take().expect("run once");
        let mut remote_reports = self.remote_reports.take().expect("run once");
        loop {
            let seek_due = self
                .seek_waiting
                .and(self.seek_asked)
                .map(|asked| tokio::time::Instant::from_std(asked + SEEK_GAP));
            let clear_due = self.clear_due();
            let add_due = self.add_due();
            let take_over_due = self.take_over_due();
            tokio::select! {
                command = commands.recv() => {
                    let Some(command) = command else { break };
                    if matches!(command, Command::NetworkBack) {
                        self.retry.reset();
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
                () = tokio::time::sleep_until(seek_due.unwrap_or_else(tokio::time::Instant::now)),
                    if seek_due.is_some() => self.send_seek(),
                () = tokio::time::sleep_until(clear_due.unwrap_or_else(tokio::time::Instant::now)),
                    if clear_due.is_some() => self.clear_next(),
                () = tokio::time::sleep_until(add_due.unwrap_or_else(tokio::time::Instant::now)),
                    if add_due.is_some() => self.add_next(),
                () = tokio::time::sleep_until(take_over_due.unwrap_or_else(tokio::time::Instant::now)),
                    if take_over_due.is_some() => self.take_over_next(),
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
        if !self.retry.due(dropped, Instant::now()) {
            return;
        }
        if dropped {
            log::info!("the session dropped; reconnecting");
        }
        let restored = tokio::time::timeout(RECONNECT_TIMEOUT, session::restore(&self.paths))
            .await
            .unwrap_or_else(|_| Err(anyhow::anyhow!("Spotify took too long to answer")));
        match restored {
            Ok(Some(session)) if self.retry.starting() => {
                self.retry.started();
                self.ready(session);
            }
            Ok(Some(session)) => {
                log::info!("reconnected");
                self.retry.reset();
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
                let wait = self.retry.failed(Instant::now());
                log::warn!(
                    "unable to connect, trying again in {}s: {error:#}",
                    wait.as_secs()
                );
            }
        }
    }

    /// Runs `work` on the session in the background and emits the event it
    /// ends with: reads take a moment, and commands go on meanwhile. False,
    /// with nothing run, while there is no session.
    fn on_session<F>(&self, work: impl FnOnce(Session) -> F) -> bool
    where
        F: Future<Output = Event> + Send + 'static,
    {
        let Some(session) = self.session.clone() else {
            return false;
        };
        let emit = Arc::clone(&self.emit);
        let work = work(session);
        tokio::spawn(async move { emit(work.await) });
        true
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
        self.retry.stop();
        (self.emit)(Event::Player(PlayerSnapshot::default()));
        (self.emit)(Event::Session(why));
    }

    /// Follows the other devices, and the playlists' changes, on `session`
    /// from now on.
    fn watch(&mut self, session: &Session) {
        self.unwatch();
        match synced::listen(session) {
            Ok(changes) => {
                let emit = Arc::clone(&self.emit);
                self.playlist_changes = Some(tokio::spawn(async move {
                    use futures_util::StreamExt;
                    let mut changes = std::pin::pin!(changes);
                    while let Some(change) = changes.next().await {
                        emit(Event::PlaylistChanged(change));
                    }
                }));
            }
            Err(error) => log::warn!("unable to follow the playlists' changes: {error:#}"),
        }
        match synced::listen_list(session) {
            Ok(changes) => {
                let emit = Arc::clone(&self.emit);
                self.list_changes = Some(tokio::spawn(async move {
                    use futures_util::StreamExt;
                    let mut changes = std::pin::pin!(changes);
                    while changes.next().await.is_some() {
                        emit(Event::PlaylistsChanged);
                    }
                }));
            }
            Err(error) => log::warn!("unable to follow the list of playlists: {error:#}"),
        }
        let devices = self.devices.clone();
        match Watch::start(session, move |playing| {
            let _ = devices.send(playing);
        }) {
            Ok(watch) => self.watch = Some(watch),
            Err(error) => log::warn!("unable to watch the other devices: {error:#}"),
        }
    }

    fn unwatch(&mut self) {
        if let Some(task) = self.playlist_changes.take() {
            task.abort();
        }
        if let Some(task) = self.list_changes.take() {
            task.abort();
        }
        if let Some(watch) = self.watch.take() {
            tokio::spawn(watch.close());
        }
        self.elsewhere = None;
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
                self.retry.reset();
            }
            Err(error) => (self.emit)(Event::Session(SessionState::Failed(format!("{error:#}")))),
        }
    }

    async fn handle(&mut self, command: Command) {
        match command {
            Command::SignIn => {
                self.retry.started();
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
            // Reconnected already, before any command.
            Command::NetworkBack => {}
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
            Command::LastFmLove {
                artist,
                title,
                loved,
            } => {
                if let Some(scrobbler) = &self.scrobbler {
                    scrobbler.love(artist, title, loved);
                }
            }
            Command::Player(_)
            | Command::Seek(_)
            | Command::Volume(_)
            | Command::Levelling(_)
            | Command::ListOutputs
            | Command::Output(_) => self.handle_player(command),
            Command::AddLink { .. }
            | Command::SearchApp(_)
            | Command::Search(_)
            | Command::MoreSongs { .. }
            | Command::Discography(_)
            | Command::SimilarArtists { .. }
            | Command::FindArtist(_)
            | Command::FindSongs { .. }
            | Command::Cover(_)
            | Command::Album(_) => self.handle_catalog(command),
            Command::SpotifyPlaylists
            | Command::RemoveFromSpotify { .. }
            | Command::PutBackOnSpotify(_)
            | Command::UnlistSpotifyPlaylist { .. }
            | Command::RelistSpotifyPlaylist(_)
            | Command::CopyToSpotify { .. } => self.handle_spotify_lists(command),
            Command::Sync { .. } | Command::CreateSynced { .. } | Command::SyncFolder => {
                self.handle_sync(command)
            }
            Command::MeasureStorage | Command::ClearCache(_) => self.handle_storage(command),
            Command::SpotifyTab(_) | Command::Spotify(_) => self.handle_spotify_tab(command),
        }
    }
}
