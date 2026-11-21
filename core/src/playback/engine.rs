//! Plays songs on Spotless's own player, carrying out what the [`Flow`]
//! decides. There is no Spirc: Spotify Connect never hears of it, so the
//! account's queue and other devices are left alone.
//!
//! The engine holds the song to follow the playing one, preloads it when
//! the player says it's time, and starts it the moment the playing song
//! ends, so songs follow each other without a gap (`super::transport`).
//! The player says so [`PRELOAD_BEFORE_END`] ahead, once the playing song is
//! all downloaded, so the next song is there when the connection goes near
//! the end of one.
//!
//! Each song opens through the source its URI names ([`Sources`]): Spotify's
//! on the session, MEGA's with no account. The engine is there from the start, signed in
//! or not: a new session, on signing in or for one that dropped, opens Spotify's songs from
//! then on, and the playing song plays on with the audio it has.

use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use anyhow::Result;
use librespot_core::session::Session;
use tokio::sync::mpsc;

#[cfg(target_os = "android")]
use super::aaudio::AAudioSink;
use super::audio_sink::AudioSink;
use super::cache::SongCache;
use super::flow::{Do, Flow, REPORTS_DUE};
use super::link::AudioLink;
use super::player::{Event, Player};
use super::source::{OpenError, Opened, Source};
#[cfg(not(target_os = "android"))]
use super::sink::RodioSink;
use crate::model::{EntryId, SongSource, SongUri, TrackRef};
use crate::mega::audio::MegaAudio;
use crate::spotify::audio::SpotifyAudio;

/// How long before the playing song's end the next one is preloaded, once
/// the playing one is all downloaded: room to walk out of signal near the
/// end of a song and still hear the next. One song ahead, never more.
const PRELOAD_BEFORE_END: Duration = Duration::from_secs(120);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Stopped,
    Loading,
    Playing,
    Paused,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrackInfo {
    pub uri: SongUri,
    pub title: String,
    pub artists: String,
    /// The first of them, as Last.fm takes it.
    pub main_artist: String,
    pub album: String,
    pub duration_ms: u32,
    /// The album cover's URL, for the desktop's media controls.
    pub cover_url: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerSnapshot {
    pub status: Status,
    pub track: Option<TrackInfo>,
    /// The position when it was last reported, at `reported_at`.
    pub position_ms: u32,
    pub reported_at: Option<Instant>,
    pub error: Option<String>,
}

impl PlayerSnapshot {
    /// The position now, counting on from the last report while playing,
    /// but not past the next report's due: a song whose audio stops coming
    /// is still playing to the player, and reports no more.
    pub fn position_now(&self) -> u32 {
        self.position_at(Instant::now())
    }

    /// The position at `now`, as [`position_now`](Self::position_now).
    pub fn position_at(&self, now: Instant) -> u32 {
        let mut position = self.position_ms;
        if self.status == Status::Playing
            && let Some(at) = self.reported_at
        {
            let since = now.saturating_duration_since(at).min(REPORTS_DUE);
            position = position.saturating_add(since.as_millis() as u32);
        }
        match &self.track {
            Some(track) => position.min(track.duration_ms),
            None => position,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum EngineEvent {
    Snapshot(PlayerSnapshot),
    /// The playing song ended and the followed one started.
    Followed(EntryId),
    /// The playing song ended with nothing to follow it: stopped.
    Ended,
    /// The loaded song can't be played. `offline`: the session to Spotify
    /// had dropped, so it may well play once there is one again.
    Unavailable {
        offline: bool,
    },
    /// The playing song ran out of audio here, short of its end, with the
    /// session to Spotify dropped: it stopped, to go on from here later.
    Cut {
        position_ms: u32,
    },
    /// The audio output is missing or failed; the player paused.
    OutputError(String),
}

pub type Notify = Arc<dyn Fn(EngineEvent) + Send + Sync>;

pub struct Engine {
    shared: Arc<Shared>,
}

/// The engine's parts, shared with the task that hands the player's events
/// to the flow.
struct Shared {
    /// Taken only when the engine is let go.
    player: Option<Player>,
    sources: Arc<Sources>,
    /// None while no one is signed in to Spotify.
    session: Mutex<Option<Session>>,
    flow: Mutex<Flow>,
    notify: Notify,
}

impl Engine {
    /// Must be called on the tokio runtime: songs open on it, and the
    /// player's events are followed there. Its output applies `link`'s
    /// equaliser and feeds its analyser.
    pub fn new(
        session: Option<&Session>,
        songs: Arc<SongCache>,
        volume: u16,
        levelling: bool,
        link: Arc<AudioLink>,
        notify: Notify,
    ) -> Result<Self> {
        let sources = Arc::new(Sources {
            spotify: SpotifyAudio::new(session, Arc::clone(&songs)),
            mega: MegaAudio::new(songs),
        });
        let (told, events) = mpsc::unbounded_channel();
        let on_error = {
            let notify = Arc::clone(&notify);
            Box::new(move |message| notify(EngineEvent::OutputError(message)))
        };
        // A device by name, for testing without one: any name that doesn't
        // exist acts as a missing output.
        #[cfg(not(target_os = "android"))]
        let device = std::env::var("SPOTLESS_AUDIO_DEVICE").ok();
        let player = Player::new(
            Arc::clone(&sources) as Arc<_>,
            Box::new(move || -> Box<dyn AudioSink> {
                #[cfg(not(target_os = "android"))]
                return Box::new(RodioSink::new(device, on_error, link));
                #[cfg(target_os = "android")]
                return Box::new(AAudioSink::new(on_error, link));
            }),
            volume,
            levelling,
            PRELOAD_BEFORE_END,
            Box::new(move |event| {
                let _ = told.send(event);
            }),
        );
        let shared = Arc::new(Shared {
            player: Some(player),
            sources,
            session: Mutex::new(session.cloned()),
            flow: Mutex::default(),
            notify,
        });
        tokio::spawn(follow(Arc::downgrade(&shared), events));
        Ok(Self { shared })
    }

    /// Volume levelling on or off, from the next piece of audio.
    pub fn set_levelling(&mut self, on: bool) {
        self.shared.player().set_levelling(on);
    }

    /// Opens Spotify's songs on a new session: on signing in, or for one
    /// that dropped. A playing song plays on with the audio it has, until
    /// it ends. With None, signed out, a Spotify song stops and the rest
    /// play on.
    pub fn set_session(&mut self, session: Option<&Session>) {
        self.shared.sources.spotify.set_session(session);
        *lock(&self.shared.session) = session.cloned();
        let done = match session {
            Some(_) => lock(&self.shared.flow).session_changed(Instant::now()),
            None => lock(&self.shared.flow).signed_out(),
        };
        self.shared.carry_out(done);
    }

    /// Plays `track` now, following it with nothing until told otherwise.
    pub fn load(&self, track: &SongUri, position_ms: u32) {
        let done = lock(&self.shared.flow).load(track, position_ms);
        self.shared.carry_out(done);
    }

    /// The song to start when the playing one ends. None: playback stops.
    /// Preloaded at once if the playing song is near its end.
    pub fn follow(&self, next: Option<(EntryId, SongUri)>) {
        let done = lock(&self.shared.flow).follow(next);
        self.shared.carry_out(done);
    }

    pub fn play(&self) {
        self.shared.player().play();
    }

    pub fn pause(&self) {
        self.shared.player().pause();
    }

    /// Whether a song is playing, or loading to play.
    pub fn playing(&self) -> bool {
        lock(&self.shared.flow).playing()
    }

    pub fn stop(&self) {
        let done = lock(&self.shared.flow).stop();
        self.shared.carry_out(done);
    }

    pub fn seek(&self, position_ms: u32) {
        self.shared.player().seek(position_ms);
    }

    pub fn set_volume(&self, volume: u16) {
        self.shared.player().set_volume(volume);
    }
}

/// The sources songs open through, each song through the one its URI
/// names.
struct Sources {
    spotify: SpotifyAudio,
    mega: MegaAudio,
}

impl Source for Sources {
    fn open(&self, uri: &SongUri) -> Result<Opened, OpenError> {
        match uri.source() {
            SongSource::Spotify { .. } => self.spotify.open(uri),
            SongSource::Mega { .. } => self.mega.open(uri),
            SongSource::Unknown => Err(OpenError::Unavailable),
        }
    }
}

impl Shared {
    fn player(&self) -> &Player {
        self.player.as_ref().expect("the player lives as long as the engine")
    }

    /// Carries out what the flow decided. The flow's lock is let go by
    /// then: a hook that answers a told event with `Engine::follow` takes
    /// it too.
    fn carry_out(&self, done: Vec<Do>) {
        for step in done {
            match step {
                Do::Load {
                    request,
                    uri,
                    play,
                    position_ms,
                } => self.player().load(request, &uri, play, position_ms),
                Do::Preload { uri, again } => self.player().preload(&uri, again),
                Do::Stop => self.player().stop(),
                Do::LetSessionGo => {
                    if let Some(session) = lock(&self.session).as_ref()
                        && !session.is_invalid()
                    {
                        session.shutdown();
                    }
                }
                Do::Tell(event) => (self.notify)(event),
            }
        }
    }
}

impl Drop for Shared {
    fn drop(&mut self) {
        // Letting the player go waits for its thread to finish the piece
        // of audio it's on: not on the runtime's threads.
        if let Some(player) = self.player.take() {
            std::thread::spawn(move || drop(player));
        }
    }
}

/// Hands the player's events to the flow and carries out what it decides,
/// until the engine is let go.
async fn follow(shared: Weak<Shared>, mut events: mpsc::UnboundedReceiver<Event>) {
    while let Some(event) = events.recv().await {
        let Some(shared) = shared.upgrade() else {
            break;
        };
        let online = (lock(&shared.session).as_ref()).is_some_and(|session| !session.is_invalid());
        let done = lock(&shared.flow).event(event, online, Instant::now());
        shared.carry_out(done);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}

/// A playlist's song, as the player shows it, for one it doesn't hold.
impl From<&TrackRef> for TrackInfo {
    fn from(track: &TrackRef) -> Self {
        Self {
            uri: track.uri.clone(),
            title: track.title.clone(),
            artists: track
                .artists
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            main_artist: track
                .artists
                .first()
                .map(|a| a.name.clone())
                .unwrap_or_default(),
            album: track.album.name.clone(),
            duration_ms: track.duration_ms,
            cover_url: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_position_stops_counting_when_reports_stop() {
        let snapshot = PlayerSnapshot {
            status: Status::Playing,
            position_ms: 60_000,
            reported_at: Some(Instant::now() - Duration::from_secs(10)),
            ..PlayerSnapshot::default()
        };
        assert_eq!(snapshot.position_now(), 61_500);
    }
}
