//! The scrobbler: a task of its own that follows the player, keeps the
//! sign-in, and sends listens, keeping them on disk until Last.fm takes
//! them.

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use super::api::{self, Failure, Keys};
use super::listen::{Heard, Listens, Scrobble};
use super::{LastFmState, LastFmStatus, Loved};
use crate::browser::Browser;
use crate::paths::Paths;
use crate::playback::engine::PlayerSnapshot;
use crate::store::save_private;

/// Listens kept for Last.fm at most; the oldest go first.
const QUEUE_LIMIT: usize = 2000;
const FIRST_RETRY: Duration = Duration::from_secs(60);
const LAST_RETRY: Duration = Duration::from_secs(30 * 60);
/// How often the newest loved songs are read, catching those loved
/// elsewhere.
const LOVED_CHECK: Duration = Duration::from_secs(5 * 60);
/// How often all of them are, catching those no longer loved.
const LOVED_ALL: Duration = Duration::from_secs(60 * 60);
/// Loved songs read at once: all of them a page at a time, or the newest.
const LOVED_PAGE: u32 = 1000;
const LOVED_NEWEST: u32 = 50;

/// What the scrobbler tells the interface.
pub enum Report {
    State(LastFmState),
    /// The songs loved, whenever they change.
    Loved(Loved),
    Error(String),
}

pub type Notify = Arc<dyn Fn(Report) + Send + Sync>;

enum Message {
    /// The player's state, when it was seen (and the wall clock then).
    Snapshot(PlayerSnapshot, Instant, u64),
    Connect(Keys),
    Disconnect,
    /// The browser approval ended, for these keys.
    SignedIn(Keys, Result<(String, String), String>),
    Love {
        artist: String,
        title: String,
        loved: bool,
    },
}

/// The handle the backend feeds.
#[derive(Clone)]
pub struct Scrobbler {
    sender: UnboundedSender<Message>,
}

impl Scrobbler {
    pub fn snapshot(&self, snapshot: PlayerSnapshot) {
        let _ = self
            .sender
            .send(Message::Snapshot(snapshot, Instant::now(), unix_now()));
    }

    /// Saves the API account and opens the browser to approve it.
    pub fn connect(&self, keys: Keys) {
        let _ = self.sender.send(Message::Connect(keys));
    }

    pub fn disconnect(&self) {
        let _ = self.sender.send(Message::Disconnect);
    }

    /// Loves a song on Last.fm, or doesn't any more.
    pub fn love(&self, artist: String, title: String, loved: bool) {
        let _ = self.sender.send(Message::Love {
            artist,
            title,
            loved,
        });
    }
}

/// Must be called on the tokio runtime.
pub fn start(paths: &Paths, browser: Browser, notify: Notify) -> Scrobbler {
    let (sender, receiver) = unbounded_channel();
    let saved = Saved::load(paths);
    let task = Task {
        loved: SavedLoved::load(paths, &saved.user),
        loved_read: None,
        saved,
        queue: load_queue(paths),
        paths: paths.clone(),
        signing_in: false,
        listens: Listens::default(),
        retry: None,
        delay: FIRST_RETRY,
        http: reqwest::Client::new(),
        browser,
        notify,
        sender: sender.clone(),
    };
    tokio::spawn(task.run(receiver));
    Scrobbler { sender }
}

/// `lastfm.json`: the API account and, once approved, the session.
#[derive(Clone, Default, Serialize, Deserialize)]
struct Saved {
    api_key: String,
    secret: String,
    #[serde(default)]
    session_key: String,
    #[serde(default)]
    user: String,
}

impl Saved {
    fn load(paths: &Paths) -> Self {
        std::fs::read(paths.lastfm())
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    fn save(&self, paths: &Paths) {
        let result = serde_json::to_vec_pretty(self)
            .map_err(anyhow::Error::from)
            .and_then(|bytes| save_private(&paths.lastfm(), &bytes));
        if let Err(error) = result {
            log::error!("unable to save the Last.fm sign-in: {error:#}");
        }
    }

    fn keys(&self) -> Keys {
        Keys {
            api_key: self.api_key.clone(),
            secret: self.secret.clone(),
        }
    }

    fn connected(&self) -> bool {
        !self.session_key.is_empty()
    }
}

/// `lastfm-loved.json`: the songs `user` loved, as last read.
#[derive(Default, Serialize, Deserialize)]
struct SavedLoved {
    user: String,
    songs: Loved,
}

impl SavedLoved {
    /// The songs `user` loved; none if they were someone else's.
    fn load(paths: &Paths, user: &str) -> Loved {
        std::fs::read(paths.lastfm_loved())
            .ok()
            .and_then(|bytes| serde_json::from_slice::<SavedLoved>(&bytes).ok())
            .filter(|saved| !user.is_empty() && saved.user == user)
            .map(|saved| saved.songs)
            .unwrap_or_default()
    }
}

struct Task {
    paths: Paths,
    saved: Saved,
    queue: Vec<Scrobble>,
    loved: Loved,
    /// When all the loved songs were last read.
    loved_read: Option<Instant>,
    signing_in: bool,
    listens: Listens,
    /// When to try the queue again after Last.fm couldn't take it.
    retry: Option<Instant>,
    delay: Duration,
    http: reqwest::Client,
    /// Opens the approval page.
    browser: Browser,
    notify: Notify,
    sender: UnboundedSender<Message>,
}

impl Task {
    async fn run(mut self, mut receiver: UnboundedReceiver<Message>) {
        self.report_state();
        (self.notify)(Report::Loved(self.loved.clone()));
        self.flush().await;
        let mut ticks = tokio::time::interval(Duration::from_secs(5));
        // Its first tick, at once, reads them all.
        let mut loved_checks = tokio::time::interval(LOVED_CHECK);
        loved_checks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            let retry = self.retry;
            let message = tokio::select! {
                message = receiver.recv() => match message {
                    Some(message) => message,
                    None => return,
                },
                _ = ticks.tick() => {
                    if let Some(heard) = self.listens.tick(Instant::now()) {
                        self.heard(heard).await;
                    }
                    continue;
                }
                _ = loved_checks.tick() => {
                    self.check_loved().await;
                    continue;
                }
                _ = async {
                    match retry {
                        Some(at) => tokio::time::sleep_until(at.into()).await,
                        None => std::future::pending().await,
                    }
                } => {
                    self.retry = None;
                    self.flush().await;
                    continue;
                }
            };
            self.handle(message).await;
        }
    }

    async fn handle(&mut self, message: Message) {
        match message {
            Message::Snapshot(snapshot, at, unix_s) => {
                for heard in self.listens.observe(&snapshot, at, unix_s) {
                    self.heard(heard).await;
                }
            }
            Message::Connect(keys) => {
                if self.signing_in {
                    return;
                }
                if let Err(error) = api::check_keys(&keys) {
                    return (self.notify)(Report::Error(error.to_string()));
                }
                self.saved = Saved {
                    api_key: keys.api_key.clone(),
                    secret: keys.secret.clone(),
                    ..Saved::default()
                };
                self.saved.save(&self.paths);
                self.signing_in = true;
                self.report_state();
                let http = self.http.clone();
                let browser = Arc::clone(&self.browser);
                let sender = self.sender.clone();
                tokio::spawn(async move {
                    let result = api::sign_in(&http, &keys, &browser)
                        .await
                        .map_err(|e| format!("{e:#}"));
                    let _ = sender.send(Message::SignedIn(keys, result));
                });
            }
            Message::SignedIn(keys, result) => {
                self.signing_in = false;
                if keys != self.saved.keys() {
                    return self.report_state();
                }
                match result {
                    Ok((session_key, user)) => {
                        self.saved.session_key = session_key;
                        self.saved.user = user;
                        self.saved.save(&self.paths);
                        self.report_state();
                        self.flush().await;
                        self.loved_read = None;
                        self.check_loved().await;
                    }
                    Err(error) => {
                        self.report_state();
                        (self.notify)(Report::Error(error));
                    }
                }
            }
            Message::Disconnect => self.sign_out(),
            Message::Love {
                artist,
                title,
                loved,
            } => self.love(&artist, &title, loved).await,
        }
    }

    async fn love(&mut self, artist: &str, title: &str, loved: bool) {
        if !self.saved.connected() {
            return;
        }
        let result = api::love(
            &self.http,
            &self.saved.keys(),
            &self.saved.session_key,
            artist,
            title,
            loved,
        )
        .await;
        match result {
            Ok(()) => {
                if self.loved.set(artist, title, loved) {
                    self.save_loved();
                }
            }
            Err(Failure::SignedOut(message)) => {
                self.sign_out();
                (self.notify)(Report::Error(format!(
                    "{message} Connect again in Settings, Last.fm."
                )));
            }
            Err(failure) => {
                let doing = if loved { "loved" } else { "unloved" };
                (self.notify)(Report::Error(format!(
                    "{title} couldn't be {doing} on Last.fm: {failure}"
                )));
            }
        }
        // The interface showed it changed already: this sets it right if
        // it didn't.
        (self.notify)(Report::Loved(self.loved.clone()));
    }

    /// Reads the newest loved songs, or all of them once in a while.
    async fn check_loved(&mut self) {
        if !self.saved.connected() {
            return;
        }
        let all = self
            .loved_read
            .is_none_or(|read| read.elapsed() >= LOVED_ALL);
        let result = if all {
            self.read_loved().await
        } else {
            api::loved_page(
                &self.http,
                &self.saved.api_key,
                &self.saved.user,
                1,
                LOVED_NEWEST,
            )
            .await
            .map(|(songs, _)| {
                let mut loved = self.loved.clone();
                loved.extend(songs);
                loved
            })
        };
        match result {
            Ok(loved) => {
                if all {
                    self.loved_read = Some(Instant::now());
                }
                if loved != self.loved {
                    self.loved = loved;
                    self.save_loved();
                    (self.notify)(Report::Loved(self.loved.clone()));
                }
            }
            Err(failure) => log::info!("Last.fm loved songs: {failure}"),
        }
    }

    /// All the songs loved, a page at a time.
    async fn read_loved(&self) -> Result<Loved, Failure> {
        let mut loved = Loved::default();
        let mut page = 1;
        loop {
            let (songs, pages) = api::loved_page(
                &self.http,
                &self.saved.api_key,
                &self.saved.user,
                page,
                LOVED_PAGE,
            )
            .await?;
            loved.extend(songs);
            if page >= pages {
                return Ok(loved);
            }
            page += 1;
        }
    }

    fn save_loved(&self) {
        let saved = SavedLoved {
            user: self.saved.user.clone(),
            songs: self.loved.clone(),
        };
        let result = serde_json::to_vec(&saved)
            .map_err(anyhow::Error::from)
            .and_then(|bytes| save_private(&self.paths.lastfm_loved(), &bytes));
        if let Err(error) = result {
            log::error!("unable to save the Last.fm loved songs: {error:#}");
        }
    }

    async fn heard(&mut self, heard: Heard) {
        if !self.saved.connected() {
            return;
        }
        match heard {
            Heard::NowPlaying(song) => {
                // Only a courtesy: not kept, not retried.
                let (http, keys, session) = (
                    self.http.clone(),
                    self.saved.keys(),
                    self.saved.session_key.clone(),
                );
                tokio::spawn(async move {
                    if let Err(failure) = api::now_playing(&http, &keys, &session, &song).await {
                        log::info!("Last.fm now playing: {failure}");
                    }
                });
            }
            Heard::Scrobble(scrobble) => {
                self.queue.push(scrobble);
                if self.queue.len() > QUEUE_LIMIT {
                    let extra = self.queue.len() - QUEUE_LIMIT;
                    self.queue.drain(..extra);
                }
                self.save_queue();
                // While waiting to retry, the listen waits with the rest.
                if self.retry.is_none() {
                    self.flush().await;
                }
            }
        }
    }

    /// Sends what is queued, a batch at a time, until done or refused.
    async fn flush(&mut self) {
        while self.saved.connected() && !self.queue.is_empty() {
            let count = self.queue.len().min(api::BATCH);
            let result = api::scrobble(
                &self.http,
                &self.saved.keys(),
                &self.saved.session_key,
                &self.queue[..count],
            )
            .await;
            match result {
                Ok(()) => {
                    self.queue.drain(..count);
                    self.save_queue();
                    self.delay = FIRST_RETRY;
                }
                Err(Failure::Later(message)) => {
                    log::info!("{message}; trying again in {} s", self.delay.as_secs());
                    self.retry = Some(Instant::now() + self.delay);
                    self.delay = (self.delay * 2).min(LAST_RETRY);
                    return;
                }
                Err(Failure::Refused(message)) => {
                    // Dropped: kept, they would block every later listen.
                    log::warn!("Last.fm refused {count} listens: {message}");
                    self.queue.drain(..count);
                    self.save_queue();
                }
                Err(Failure::SignedOut(message)) => {
                    self.sign_out();
                    (self.notify)(Report::Error(format!(
                        "{message} Connect again in Settings, Last.fm."
                    )));
                    return;
                }
            }
        }
    }

    /// Forgets the session, keeping the API account and the queue.
    fn sign_out(&mut self) {
        self.saved.session_key.clear();
        self.saved.user.clear();
        self.saved.save(&self.paths);
        self.retry = None;
        self.report_state();
        self.loved = Loved::default();
        self.loved_read = None;
        if let Err(error) = std::fs::remove_file(self.paths.lastfm_loved())
            && error.kind() != std::io::ErrorKind::NotFound
        {
            log::error!("unable to forget the Last.fm loved songs: {error:#}");
        }
        (self.notify)(Report::Loved(Loved::default()));
    }

    fn report_state(&self) {
        let status = if self.signing_in {
            LastFmStatus::SigningIn
        } else if self.saved.connected() {
            LastFmStatus::Connected {
                user: self.saved.user.clone(),
            }
        } else {
            LastFmStatus::Off
        };
        (self.notify)(Report::State(LastFmState {
            status,
            api_key: self.saved.api_key.clone(),
            secret: self.saved.secret.clone(),
        }));
    }

    fn save_queue(&self) {
        let path = self.paths.lastfm_queue();
        let result = if self.queue.is_empty() {
            match std::fs::remove_file(&path) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
                _ => Ok(()),
            }
        } else {
            serde_json::to_vec(&self.queue)
                .map_err(anyhow::Error::from)
                .and_then(|bytes| save_private(&path, &bytes))
        };
        if let Err(error) = result {
            log::error!("unable to save the Last.fm queue: {error:#}");
        }
    }
}

fn load_queue(paths: &Paths) -> Vec<Scrobble> {
    std::fs::read(paths.lastfm_queue())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
