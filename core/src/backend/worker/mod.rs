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
#[cfg(feature = "spotify-tab")]
mod connect;
#[cfg(not(feature = "spotify-tab"))]
#[path = "connect_off.rs"]
mod connect;
mod player;
mod retry;
mod spotify_lists;
mod storage;
mod sync;

use super::{Command, Emit, Event, SessionState, Start};
use crate::browser::Browser;
use crate::lastfm::{self, Scrobbler};
use crate::paths::Paths;
use crate::playback::engine::{Engine, EngineEvent, PlayerSnapshot};
use crate::playback::link::AudioLink;
use crate::spotify::devices::{Playing, Watch};
use crate::spotify::session::AudioCache;
use crate::spotify::webapi::WebApi;
use crate::spotify::{session, synced};
use connect::Connect;
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
    /// The personal app search signs in with, as at the start; `run` sets
    /// it up.
    search_app: Option<String>,
    /// Every session's audio cache, as at the start.
    audio_cache: AudioCache,
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
    /// The sign-in waiting for the browser's approval, by its number: it
    /// runs beside the commands, and a new one or signing out drops it.
    signing_in: Option<(u64, tokio::task::JoinHandle<()>)>,
    /// The number the last sign-in was given.
    sign_ins: u64,
    /// Where a sign-in tells how it went, by its number, and where `run`
    /// hears it.
    signed_in: UnboundedSender<(u64, anyhow::Result<Session>)>,
    sign_in_reports: Option<UnboundedReceiver<(u64, anyhow::Result<Session>)>>,
    /// The Spotify tab's Connect device (`connect.rs`), with the
    /// `spotify-tab` feature.
    connect: Connect,
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
    /// The equaliser and analyser the audio outputs use.
    link: Arc<AudioLink>,
}

impl Worker {
    pub(super) fn new(
        paths: Paths,
        start: Start,
        browser: Browser,
        link: Arc<AudioLink>,
        emit: Emit,
    ) -> Self {
        let (devices, device_reports) = unbounded_channel();
        let (signed_in, sign_in_reports) = unbounded_channel();
        let found = Arc::new(tokio::sync::Mutex::new(lastfm::found::Found::load(&paths)));
        Self {
            paths,
            volume: start.volume,
            levelling: start.levelling,
            search_app: start.search_app,
            audio_cache: start.audio_cache,
            connect: Connect::new(start.spotify_tab),
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
            signing_in: None,
            sign_ins: 0,
            signed_in,
            sign_in_reports: Some(sign_in_reports),
            web: None,
            search_approval: Arc::default(),
            found,
            http: reqwest::Client::new(),
            cover_slots: Arc::new(tokio::sync::Semaphore::new(6)),
            retry: Retry::new(),
            link,
        }
    }

    pub(super) async fn run(mut self, mut commands: UnboundedReceiver<Command>) {
        let search_app = self.search_app.take();
        self.set_search_app(search_app);
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
        let mut sign_in_reports = self.sign_in_reports.take().expect("run once");
        let mut remote_reports = self.connect.take_reports();
        loop {
            let connect_due = self.connect_due();
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
                Some((number, result)) = sign_in_reports.recv() => self.signed_in(number, result),
                Some(report) = remote_reports.recv() => self.remote_report(report),
                () = tokio::time::sleep_until(connect_due.unwrap_or_else(tokio::time::Instant::now)),
                    if connect_due.is_some() => self.connect_step(),
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
        let restored = tokio::time::timeout(RECONNECT_TIMEOUT, session::restore(&self.paths, self.audio_cache))
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
            Arc::clone(&self.link),
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

    /// Opens the approval page and waits for it beside the commands. A
    /// sign-in still waiting is given up first: its page may have been
    /// closed, and this one opens a new page.
    async fn sign_in(&mut self) {
        self.give_up_sign_in().await;
        self.retry.started();
        (self.emit)(Event::Session(SessionState::SigningIn));
        self.sign_ins += 1;
        let number = self.sign_ins;
        let paths = self.paths.clone();
        let audio_cache = self.audio_cache;
        let browser = Arc::clone(&self.browser);
        let report = self.signed_in.clone();
        let task = tokio::spawn(async move {
            let _ = report.send((number, session::sign_in(&paths, audio_cache, &browser).await));
        });
        self.signing_in = Some((number, task));
    }

    /// Stops waiting for the sign-in's approval, if one is waited for. Its
    /// port is free once this returns, for a new sign-in to listen on.
    async fn give_up_sign_in(&mut self) {
        if let Some((_, task)) = self.signing_in.take() {
            task.abort();
            let _ = task.await;
        }
    }

    /// A sign-in finished; one given up since is let go.
    fn signed_in(&mut self, number: u64, result: anyhow::Result<Session>) {
        if self.signing_in.as_ref().is_none_or(|(n, _)| *n != number) {
            if let Ok(session) = result {
                session.shutdown();
            }
            return;
        }
        self.signing_in = None;
        match result {
            Ok(session) => {
                self.ready(session);
                // Search's approval follows at once, as one sign-in.
                self.approve_search_now();
            }
            Err(error) => (self.emit)(Event::Session(SessionState::Failed(format!("{error:#}")))),
        }
    }

    async fn handle(&mut self, command: Command) {
        match command {
            Command::SignIn => self.sign_in().await,
            Command::SignOut => {
                self.give_up_sign_in().await;
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
            | Command::MoreArtists { .. }
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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use super::super::{Backend, Command, Event, SessionState};
    use crate::paths::Paths;

    /// The next event that `wanted` takes, within a few seconds.
    fn next(backend: &Backend, wanted: impl Fn(&Event) -> bool) -> Option<Event> {
        let until = std::time::Instant::now() + Duration::from_secs(5);
        while let Some(wait) = until.checked_duration_since(std::time::Instant::now()) {
            match backend.events.recv_timeout(wait) {
                Ok(event) if wanted(&event) => return Some(event),
                Ok(_) => {}
                Err(_) => return None,
            }
        }
        None
    }

    // One test: a sign-in listens on playback's port, which only one can.
    #[test]
    fn a_sign_in_waits_beside_the_commands_and_can_start_over_or_be_given_up() {
        let root = std::env::temp_dir().join(format!("spotless-sign-in-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = Paths::from_dirs(root.join("config"), root.join("cache")).unwrap();
        let pages = std::sync::Arc::new(Mutex::new(0));
        let opened = std::sync::Arc::clone(&pages);
        // The browser opens nothing: as if its tab were closed.
        let browser: crate::browser::Browser = std::sync::Arc::new(move |_: &str| {
            *opened.lock().unwrap() += 1;
        });
        let backend = Backend::start(
            paths,
            super::super::Start::default(),
            browser,
            std::sync::Arc::default(),
            || {},
        )
        .unwrap();
        let session = |event: &Event| matches!(event, Event::Session(_));
        let signed_out = |e: &Event| matches!(e, Event::Session(SessionState::SignedOut));
        assert!(next(&backend, signed_out).is_some());

        backend.send(Command::SignIn);
        assert!(matches!(
            next(&backend, session),
            Some(Event::Session(SessionState::SigningIn))
        ));
        backend.send(Command::MeasureStorage);
        assert!(
            next(&backend, |e| matches!(e, Event::Storage(_))).is_some(),
            "the commands go on while the sign-in waits"
        );

        // Signing in again opens a new page: the first gave its port up.
        backend.send(Command::SignIn);
        assert!(matches!(
            next(&backend, session),
            Some(Event::Session(SessionState::SigningIn))
        ));
        // Its page opens once it has the port, which a busy computer can
        // take a while to give.
        let until = std::time::Instant::now() + Duration::from_secs(5);
        while *pages.lock().unwrap() < 2 && std::time::Instant::now() < until {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(*pages.lock().unwrap(), 2);
        let failed = |e: &Event| matches!(e, Event::Session(SessionState::Failed(_)));
        assert!(
            next(&backend, failed).is_none(),
            "the second sign-in listens"
        );

        backend.send(Command::SignOut);
        assert!(matches!(
            next(&backend, session),
            Some(Event::Session(SessionState::SignedOut))
        ));
    }
}
