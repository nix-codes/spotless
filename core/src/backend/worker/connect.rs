//! The worker's part in the Spotify tab: Spotless's Connect device, and
//! the account's queue played, cleared, added to and taken over through
//! it, step by step as Spotify reports.
//!
//! Built with the `spotify-tab` feature only; without it `connect_off.rs`
//! takes this file's place, and Spotless never joins Spotify Connect.

use super::Worker;
use crate::backend::SpotifyRequest;
use crate::backend::{Command, Event};
use crate::playback::engine::Status;
use crate::spotify::devices;
use crate::spotify::queue::SpotifyQueue;
use crate::spotify::remote::Remote;
use anyhow::Result;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

/// The queue asked to play here: since `asked`, from `song` if given;
/// `skipped`: the skip to it is sent.
struct PlayHere {
    asked: Instant,
    song: Option<String>,
    skipped: bool,
}

/// The queue asked to be cleared: since `asked`; `took_over`: it was
/// taken over for it, and came here at `here_at`.
struct Clearing {
    asked: Instant,
    took_over: bool,
    here_at: Option<Instant>,
}

/// Songs asked to be added or removed while no device held the queue:
/// since `asked`, while the queue comes here to take the changes; it came
/// at `here_at`.
struct Adding {
    asked: Instant,
    uris: Vec<String>,
    /// The songs to come to remove, by place and URI as shown when asked.
    remove: Vec<(usize, String)>,
    here_at: Option<Instant>,
}

/// The queue asked to be taken over: since `asked`; `handed`: Spotify was
/// asked to hand it over, once the device holding it paused.
struct TakingOver {
    asked: Instant,
    handed: bool,
}

/// What the Spotify tab's device tells the worker.
pub(super) enum RemoteReport {
    Started(Result<Remote>),
    Queue(Box<SpotifyQueue>),
}

/// How long the queue asked to play here is waited for.
const PLAY_HERE_WAIT: Duration = Duration::from_secs(10);
/// How long a take-over waits for the device holding the queue to pause
/// before asking Spotify to hand it over anyway.
const TAKE_OVER_PAUSE: Duration = Duration::from_millis(1_500);
/// A seek on the queue after one this recent waits, and goes to Spotify
/// only once seeking has paused this long: each seek is a few requests to
/// Spotify, which refuses too many.
const SEEK_GAP: Duration = Duration::from_millis(400);
/// A queue taken over to be cleared, or added to, is changed once its songs
/// to come are in, or this long after it came, if none come: changed
/// sooner, the handover still finishing would undo it.
const CLEAR_SETTLE: Duration = Duration::from_secs(2);

/// The Spotify tab's device and what is asked of the queue through it.
pub(super) struct Connect {
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
}

impl Connect {
    pub(super) fn new(spotify_tab: bool) -> Self {
        let (remotes, remote_reports) = unbounded_channel();
        Self {
            spotify_tab,
            remote: None,
            remote_starting: false,
            queue: SpotifyQueue::default(),
            play_here: None,
            clearing: None,
            adding: None,
            taking_over: None,
            seek_asked: None,
            seek_waiting: None,
            remotes,
            remote_reports: Some(remote_reports),
        }
    }

    /// What the remotes tell, for the worker's loop to hear: once.
    pub(super) fn take_reports(&mut self) -> UnboundedReceiver<RemoteReport> {
        self.remote_reports.take().expect("taken once")
    }
}

impl Worker {
    /// Joins or leaves Connect, and does what the Spotify tab asks.
    pub(super) fn handle_spotify_tab(&mut self, command: Command) {
        match command {
            Command::SpotifyTab(on) => {
                self.connect.spotify_tab = on;
                if on {
                    self.start_remote();
                } else {
                    self.connect.play_here = None;
                    self.connect.clearing = None;
                    self.connect.adding = None;
                    self.connect.taking_over = None;
                    self.stop_remote();
                }
            }
            Command::Spotify(request) => self.spotify(request),
            _ => {}
        }
    }

    /// When the next of the queue's waiting steps is due: a seek, a clear,
    /// songs to add, a take-over.
    pub(super) fn connect_due(&self) -> Option<tokio::time::Instant> {
        [
            self.seek_due(),
            self.clear_due(),
            self.add_due(),
            self.take_over_due(),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// Takes the queue's steps that are due now.
    pub(super) fn connect_step(&mut self) {
        let now = tokio::time::Instant::now();
        let due = |at: Option<tokio::time::Instant>| at.is_some_and(|at| at <= now);
        if due(self.seek_due()) {
            self.send_seek();
        }
        if due(self.clear_due()) {
            self.clear_next();
        }
        if due(self.add_due()) {
            self.add_next();
        }
        if due(self.take_over_due()) {
            self.take_over_next();
        }
    }

    /// When the seek waiting goes to Spotify: once seeking has paused.
    fn seek_due(&self) -> Option<tokio::time::Instant> {
        let asked = self.connect.seek_waiting.and(self.connect.seek_asked)?;
        Some(tokio::time::Instant::from_std(asked + SEEK_GAP))
    }

    /// Sets the Spotify tab's device's volume, as the player's.
    pub(super) fn set_remote_volume(&self, volume: u16) {
        if let Some(remote) = &self.connect.remote {
            remote.set_volume(volume);
        }
    }

    /// Pauses the Spotify tab's device instead of another, as Spotless
    /// starts, if it's the one playing; true if so. It keeps the queue:
    /// let go, Spotify would still show it here, paused, and playing it
    /// here again would find no one to play it.
    pub(super) fn pause_remote_instead(&self) -> bool {
        let Some(remote) = &self.connect.remote else {
            return false;
        };
        let playing = self.connect.queue.playing_here()
            || self
                .elsewhere
                .as_ref()
                .is_some_and(|o| self.is_remote(&o.device));
        if playing {
            remote.pause();
        }
        playing
    }

    pub(super) fn is_remote(&self, device: &str) -> bool {
        self.connect
            .remote
            .as_ref()
            .is_some_and(|r| r.device_id() == device)
    }

    /// Starts the Spotify tab's device, unless it runs or is starting.
    pub(super) fn start_remote(&mut self) {
        if !self.connect.spotify_tab
            || self.connect.remote.is_some()
            || self.connect.remote_starting
            || self.session.is_none()
        {
            return;
        }
        self.connect.remote_starting = true;
        let paths = self.paths.clone();
        let audio_cache = self.audio_cache;
        let (volume, levelling) = (self.volume, self.levelling);
        let link = Arc::clone(&self.link);
        let remotes = self.connect.remotes.clone();
        tokio::spawn(async move {
            let queues = remotes.clone();
            let report = move |queue| {
                let _ = queues.send(RemoteReport::Queue(Box::new(queue)));
            };
            let started = Remote::start(&paths, audio_cache, volume, levelling, link, report).await;
            let _ = remotes.send(RemoteReport::Started(started));
        });
    }

    /// Starts the Spotify tab's device again when Spotify dropped its
    /// session.
    pub(super) fn restart_remote_if_needed(&mut self) {
        if self.connect.remote.as_ref().is_some_and(Remote::is_invalid) {
            log::info!("the Spotify tab's session dropped; joining again");
            self.stop_remote();
        }
        self.start_remote();
    }

    pub(super) fn stop_remote(&mut self) {
        if let Some(remote) = self.connect.remote.take() {
            remote.shut_down();
            (self.emit)(Event::Spotify(None));
        }
        self.connect.queue = SpotifyQueue::default();
    }

    pub(super) fn remote_report(&mut self, report: RemoteReport) {
        match report {
            RemoteReport::Started(Ok(remote)) => {
                self.connect.remote_starting = false;
                // Signed out, or the tab closed, meanwhile.
                if self.session.is_none() || !self.connect.spotify_tab {
                    remote.shut_down();
                    return;
                }
                self.connect.remote = Some(remote);
                (self.emit)(Event::Spotify(Some(self.connect.queue.clone())));
            }
            RemoteReport::Started(Err(error)) => {
                self.connect.remote_starting = false;
                log::warn!("unable to start the Spotify tab: {error:#}");
            }
            RemoteReport::Queue(queue) => {
                let queue = *queue;
                if self.connect.remote.is_none() && !self.connect.remote_starting {
                    return;
                }
                self.connect.queue = queue.clone();
                self.play_here_next();
                self.clear_next();
                self.add_next();
                self.take_over_next();
                (self.emit)(Event::Spotify(Some(queue)));
            }
        }
    }

    fn spotify(&mut self, request: SpotifyRequest) {
        let Some(remote) = &self.connect.remote else {
            return;
        };
        // A clear leaves Empty Track paused, shown as nothing: Play goes to
        // the first song queued since, if one is, not to its silence.
        let request = match request {
            SpotifyRequest::Play | SpotifyRequest::TogglePause
                if self.connect.queue.here
                    && self.connect.queue.now.is_none()
                    && self.connect.queue.status != Status::Playing =>
            {
                let mut to_come = self.connect.queue.parts.iter().flat_map(|part| &part.songs);
                match to_come.next() {
                    Some(first) => SpotifyRequest::PlayHere(Some(first.track.uri.clone())),
                    None => return,
                }
            }
            other => other,
        };
        if let SpotifyRequest::PlayHere(song) = request {
            // The queue's to play instead, not alongside.
            if let Some(engine) = &self.engine
                && engine.playing()
            {
                engine.pause();
            }
            if !self.connect.queue.here {
                log::info!("taking the queue over");
                remote.play_here();
            }
            self.connect.play_here = Some(PlayHere {
                asked: Instant::now(),
                song,
                skipped: false,
            });
            self.play_here_next();
            return;
        }
        if let SpotifyRequest::Clear = request {
            self.connect.play_here = None;
            self.connect.seek_waiting = None;
            let took_over = !self.connect.queue.here;
            if took_over {
                log::info!("taking the queue over, to clear it");
                remote.play_here();
            }
            self.connect.clearing = Some(Clearing {
                asked: Instant::now(),
                took_over,
                here_at: None,
            });
            self.clear_next();
            return;
        }
        // No device holds the queue, so none takes changes to it: Spotless
        // takes it over, without playing it, and adds or removes the songs
        // once it has it, as CLEAR does.
        if let SpotifyRequest::Add(_) | SpotifyRequest::Remove(_) = &request
            && !self.connect.queue.here
            && self.connect.queue.device.is_none()
        {
            let adding = self.connect.adding.get_or_insert_with(|| {
                log::info!("taking the queue over, to change it");
                remote.play_here();
                Adding {
                    asked: Instant::now(),
                    uris: Vec::new(),
                    remove: Vec::new(),
                    here_at: None,
                }
            });
            match request {
                SpotifyRequest::Add(uris) => adding.uris.extend(uris),
                SpotifyRequest::Remove(songs) => adding.remove.extend(songs),
                _ => {}
            }
            self.add_next();
            return;
        }
        if let SpotifyRequest::TakeOver = request {
            if self.connect.queue.here || self.connect.taking_over.is_some() {
                return;
            }
            log::info!("taking the queue over, paused");
            self.connect.play_here = None;
            if self.connect.queue.status == Status::Playing {
                self.send_spotify(SpotifyRequest::Pause);
            }
            self.connect.taking_over = Some(TakingOver {
                asked: Instant::now(),
                handed: false,
            });
            self.take_over_next();
            return;
        }
        if let SpotifyRequest::Seek(position) = request {
            let quiet = self
                .connect
                .seek_asked
                .is_none_or(|at| at.elapsed() >= SEEK_GAP);
            self.connect.seek_asked = Some(Instant::now());
            self.connect.seek_waiting = Some(position);
            if quiet {
                self.send_seek();
            }
            return;
        }
        // A seek still waiting goes first, as it was asked first.
        if self.connect.seek_waiting.is_some() {
            self.send_seek();
        }
        self.send_spotify(request);
    }

    /// Takes the next step to play the queue here, as asked, once it can:
    /// once the queue is here, skips to the song asked for once Spotless
    /// has it to come, then plays, as Spotify hands a paused queue over
    /// paused.
    fn play_here_next(&mut self) {
        let Some(asked) = &mut self.connect.play_here else {
            return;
        };
        let Some(remote) = &self.connect.remote else {
            return;
        };
        if asked.asked.elapsed() >= PLAY_HERE_WAIT {
            log::warn!("Spotify didn't hand the queue over");
            self.connect.play_here = None;
            return;
        }
        if !self.connect.queue.here || self.connect.queue.loading {
            return;
        }
        if let Some(song) = &asked.song
            && self
                .connect
                .queue
                .now
                .as_ref()
                .is_none_or(|now| now.uri != *song)
        {
            let to_come = self.connect.queue.parts.iter().flat_map(|part| &part.songs);
            if !asked.skipped && to_come.clone().any(|queued| queued.track.uri == *song) {
                asked.skipped = true;
                let (session, device, song) = (
                    remote.session(),
                    remote.device_id().to_string(),
                    song.clone(),
                );
                tokio::spawn(async move {
                    if let Err(error) = devices::skip_to(&session, &device, &song).await {
                        log::warn!("{error:#}");
                    }
                });
            }
            return;
        }
        if self.connect.queue.status != Status::Playing {
            remote.play();
        }
        self.connect.play_here = None;
    }

    /// Clears the queue as asked, once it is here.
    fn clear_next(&mut self) {
        let (Some(clearing), Some(remote)) = (&mut self.connect.clearing, &self.connect.remote)
        else {
            return;
        };
        if clearing.asked.elapsed() >= PLAY_HERE_WAIT {
            log::warn!("Spotify didn't hand the queue over to clear it");
            self.connect.clearing = None;
            return;
        }
        if !self.connect.queue.here {
            return;
        }
        let here_at = *clearing.here_at.get_or_insert_with(Instant::now);
        let settled = !self.connect.queue.loading && !self.connect.queue.parts.is_empty();
        if clearing.took_over && !settled && here_at.elapsed() < CLEAR_SETTLE {
            return;
        }
        remote.clear();
        self.connect.clearing = None;
    }

    /// When a clear waiting on a handover is due anyway.
    fn clear_due(&self) -> Option<tokio::time::Instant> {
        let here_at = self.connect.clearing.as_ref()?.here_at?;
        Some(tokio::time::Instant::from_std(here_at + CLEAR_SETTLE))
    }

    /// Removes and adds the songs waiting, once the queue taken over for
    /// them is here and settled: changed sooner, the handover still
    /// finishing would undo the changes. The songs to remove are found again
    /// by URI if the handover moved them.
    fn add_next(&mut self) {
        let (Some(adding), Some(remote)) = (&mut self.connect.adding, &self.connect.remote) else {
            return;
        };
        if adding.asked.elapsed() >= PLAY_HERE_WAIT {
            log::warn!("Spotify didn't hand the queue over to change it");
            self.connect.adding = None;
            (self.emit)(Event::Error(
                "Spotify didn't hand the queue over to Spotless, so the queue wasn't changed."
                    .into(),
            ));
            return;
        }
        if !self.connect.queue.here {
            return;
        }
        let here_at = *adding.here_at.get_or_insert_with(Instant::now);
        let settled = !self.connect.queue.loading && !self.connect.queue.parts.is_empty();
        if !settled && here_at.elapsed() < CLEAR_SETTLE {
            return;
        }
        if !adding.remove.is_empty() {
            remote.remove(self.connect.queue.find_again(&adding.remove));
        }
        if !adding.uris.is_empty() {
            remote.add(&adding.uris);
        }
        self.connect.adding = None;
    }

    /// When songs waiting on a handover are due to be added anyway.
    fn add_due(&self) -> Option<tokio::time::Instant> {
        let adding = self.connect.adding.as_ref()?;
        let due = match adding.here_at {
            Some(here_at) => here_at + CLEAR_SETTLE,
            // Checked for having waited too long.
            None => adding.asked + PLAY_HERE_WAIT,
        };
        Some(tokio::time::Instant::from_std(due))
    }

    /// Takes the next step to take the queue over: asks Spotify to hand it
    /// over once the device holding it paused, or soon anyway; once here,
    /// pauses it if it came playing.
    fn take_over_next(&mut self) {
        let (Some(taking), Some(remote)) = (&mut self.connect.taking_over, &self.connect.remote)
        else {
            return;
        };
        if taking.asked.elapsed() >= PLAY_HERE_WAIT {
            log::warn!("Spotify didn't hand the queue over");
            self.connect.taking_over = None;
            (self.emit)(Event::Error(
                "Spotify didn't hand the queue over to Spotless.".into(),
            ));
            return;
        }
        if !taking.handed {
            if self.connect.queue.status == Status::Playing
                && taking.asked.elapsed() < TAKE_OVER_PAUSE
            {
                return;
            }
            taking.handed = true;
            remote.play_here();
            return;
        }
        if !self.connect.queue.here || self.connect.queue.loading {
            return;
        }
        if self.connect.queue.status == Status::Playing {
            remote.pause();
        }
        self.connect.taking_over = None;
    }

    /// When the next step of a take-over is due anyway.
    fn take_over_due(&self) -> Option<tokio::time::Instant> {
        let taking = self.connect.taking_over.as_ref()?;
        let due = if taking.handed {
            taking.asked + PLAY_HERE_WAIT
        } else {
            taking.asked + TAKE_OVER_PAUSE
        };
        Some(tokio::time::Instant::from_std(due))
    }

    /// Sends the seek waiting, if one is.
    fn send_seek(&mut self) {
        if let Some(position) = self.connect.seek_waiting.take() {
            self.send_spotify(SpotifyRequest::Seek(position));
        }
    }

    /// Sends `request` to the device playing the queue.
    fn send_spotify(&self, request: SpotifyRequest) {
        let Some(remote) = &self.connect.remote else {
            return;
        };
        // Another device plays the queue: the player's buttons act on it.
        if let Some(device) = self
            .connect
            .queue
            .device_id
            .clone()
            .filter(|_| !self.connect.queue.here)
        {
            let commands = match request {
                SpotifyRequest::Play => vec![("resume", None)],
                SpotifyRequest::Pause => vec![("pause", None)],
                SpotifyRequest::Stop => vec![("pause", None), ("seek_to", Some(0))],
                SpotifyRequest::TogglePause if self.connect.queue.status == Status::Playing => {
                    vec![("pause", None)]
                }
                SpotifyRequest::TogglePause => vec![("resume", None)],
                SpotifyRequest::Next => vec![("skip_next", None)],
                SpotifyRequest::Previous => vec![("skip_prev", None)],
                SpotifyRequest::Seek(position) => vec![("seek_to", Some(position))],
                // Spotify takes no queue changes from a device that watches.
                SpotifyRequest::Add(_)
                | SpotifyRequest::Remove(_)
                | SpotifyRequest::PlayHere(_)
                | SpotifyRequest::Clear
                | SpotifyRequest::TakeOver => return,
            };
            let session = remote.session();
            tokio::spawn(async move {
                for (endpoint, value) in commands {
                    if let Err(error) = devices::command(&session, &device, endpoint, value).await {
                        log::warn!("{error:#}");
                        return;
                    }
                }
            });
            return;
        }
        match request {
            SpotifyRequest::Play => remote.play(),
            SpotifyRequest::Pause => remote.pause(),
            SpotifyRequest::Stop => {
                remote.pause();
                remote.seek(0);
            }
            SpotifyRequest::TogglePause => remote.toggle_pause(),
            SpotifyRequest::Next => remote.next(),
            SpotifyRequest::Previous => remote.previous(),
            SpotifyRequest::Seek(position) => remote.seek(position),
            SpotifyRequest::Add(uris) => remote.add(&uris),
            SpotifyRequest::Remove(songs) => remote.remove(songs),
            SpotifyRequest::PlayHere(_) | SpotifyRequest::Clear | SpotifyRequest::TakeOver => {}
        }
    }
}
