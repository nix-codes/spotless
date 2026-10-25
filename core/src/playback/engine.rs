//! A bare librespot `Player` on the session. There is no Spirc: Spotify
//! Connect never hears of it, so the account's queue and other devices are
//! left alone (as spotifast's local-queue player proved).
//!
//! Player events are folded into a [`PlayerSnapshot`] that is handed to the
//! interface whenever it changes.
//!
//! Like Spirc, the engine holds the song to follow the playing one, preloads
//! it when librespot asks, and starts it the moment the playing song ends,
//! so songs follow each other without a gap (`super::transport`).
//!
//! Volume levelling and the session are fixed when a librespot player is
//! made, so changing either makes a new player that picks up the song where
//! the old one was. A new session, for one that dropped, waits for the
//! playing song to end instead: the song plays on with the audio it has.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use librespot_core::SpotifyUri;
use librespot_core::session::Session;
use librespot_metadata::audio::{AudioItem, UniqueFields};
use librespot_playback::config::{Bitrate, NormalisationType, PlayerConfig, VolumeCtrl};
use librespot_playback::mixer::{self, Mixer, MixerConfig};
use librespot_playback::player::{Player, PlayerEvent};

#[cfg(target_os = "android")]
use super::aaudio::AAudioSink;
#[cfg(not(target_os = "android"))]
use super::sink::RodioSink;
use crate::model::{EntryId, TrackRef};

/// Position reports come every second while a song plays (see
/// `position_update_interval`); this one's beyond that.
const REPORTS_DUE: Duration = Duration::from_millis(1_500);
/// A song that ends further than this from its end was cut short.
const CUT_MARGIN_MS: u32 = 5_000;

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
    pub uri: String,
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
    /// is still playing to librespot, and reports no more.
    pub fn position_now(&self) -> u32 {
        let mut position = self.position_ms;
        if self.status == Status::Playing
            && let Some(at) = self.reported_at
        {
            let since = at.elapsed().min(REPORTS_DUE);
            position = position.saturating_add(since.as_millis() as u32);
        }
        match &self.track {
            Some(track) => position.min(track.duration_ms),
            None => position,
        }
    }
}

#[derive(Debug)]
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

/// What the engine's players are made with, shared with the task following
/// the playing one's events, which makes the next player itself when a
/// song ends on a dropped session.
struct Shared {
    mixer: Arc<dyn Mixer>,
    notify: Notify,
    /// The song to start when the playing one ends.
    follow: Mutex<Option<(EntryId, SpotifyUri)>>,
    /// The player's state as last reported, to carry over to a new player.
    state: Mutex<PlayerSnapshot>,
    current: Mutex<Current>,
}

/// The player, and what it was made with.
struct Current {
    session: Session,
    /// A new session, for the player's dropped one, waiting for the playing
    /// song to end: the song goes on with the audio it has, as a new player
    /// would start it over (a gap each time the network changes).
    waiting: Option<Session>,
    /// Volume levelling, as the player was made with.
    levelling: bool,
    player: Arc<Player>,
    /// Cleared when the player is replaced: its events are no longer told.
    live: Arc<AtomicBool>,
}

/// A song for a new player to load: playing or not, from where.
type Load = (SpotifyUri, bool, u32);

impl Engine {
    /// Must be called on the tokio runtime: the event fold runs on it.
    pub fn new(session: &Session, volume: u16, levelling: bool, notify: Notify) -> Result<Self> {
        let mixer =
            mixer::find(Some("softvol")).ok_or_else(|| anyhow!("soft volume mixer missing"))?;
        // librespot's default curve puts half the slider below -30 dB. The
        // cubic one spreads the useful range across it (as in spotifast).
        let mixer = mixer(MixerConfig {
            volume_ctrl: VolumeCtrl::Cubic(VolumeCtrl::DEFAULT_DB_RANGE),
            ..MixerConfig::default()
        })
        .context("unable to create the mixer")?;
        mixer.set_volume(volume);
        let shared = Arc::new_cyclic(|shared: &Weak<Shared>| {
            let live = Arc::new(AtomicBool::new(true));
            Shared {
                current: Mutex::new(Current {
                    session: session.clone(),
                    waiting: None,
                    levelling,
                    player: start_player(shared, session, &mixer, levelling, &live, &notify),
                    live,
                }),
                mixer,
                notify,
                follow: Mutex::default(),
                state: Mutex::default(),
            }
        });
        Ok(Self { shared })
    }

    fn player(&self) -> Arc<Player> {
        Arc::clone(&lock(&self.shared.current).player)
    }

    /// Turns volume levelling on or off with a new player, which goes on
    /// with the song (playing or paused) and the song to follow it.
    pub fn set_levelling(&mut self, on: bool) {
        let mut current = lock(&self.shared.current);
        if on == current.levelling {
            return;
        }
        current.levelling = on;
        let carried = self.shared.carried();
        self.shared.renew(&mut current, carried);
    }

    /// Plays on a new session, for one that dropped, with a new player that
    /// goes on as `set_levelling`'s does. A playing song plays on without
    /// one, with the audio it has, until it ends.
    pub fn set_session(&mut self, session: &Session) {
        let mut current = lock(&self.shared.current);
        current.waiting = Some(session.clone());
        // Reports stop coming when the audio has: a stalled song is picked
        // up at once instead.
        let state = lock(&self.shared.state).clone();
        if state.status == Status::Playing
            && state
                .reported_at
                .is_some_and(|at| at.elapsed() < REPORTS_DUE)
        {
            log::info!("the playing song goes on; a new player is made when it ends");
            return;
        }
        let carried = self.shared.carried();
        self.shared.renew(&mut current, carried);
    }

    /// Plays `track` now, following it with nothing until told otherwise.
    pub fn load(&self, track: SpotifyUri, position_ms: u32) {
        *lock(&self.shared.follow) = None;
        let mut current = lock(&self.shared.current);
        if current.waiting.is_some() {
            self.shared
                .renew(&mut current, Some((track, true, position_ms)));
        } else {
            current.player.load(track, true, position_ms);
        }
    }

    /// The song to start when the playing one ends. None: playback stops.
    pub fn follow(&self, next: Option<(EntryId, SpotifyUri)>) {
        *lock(&self.shared.follow) = next;
    }

    pub fn play(&self) {
        self.player().play();
    }

    pub fn pause(&self) {
        self.player().pause();
    }

    /// Whether a song is playing, or loading to play.
    pub fn playing(&self) -> bool {
        matches!(
            lock(&self.shared.state).status,
            Status::Playing | Status::Loading
        )
    }

    pub fn stop(&self) {
        *lock(&self.shared.follow) = None;
        let mut current = lock(&self.shared.current);
        current.player.stop();
        if current.waiting.is_some() {
            self.shared.renew(&mut current, None);
        }
    }

    pub fn seek(&self, position_ms: u32) {
        self.player().seek(position_ms);
    }

    pub fn set_volume(&self, volume: u16) {
        self.shared.mixer.set_volume(volume);
    }
}

impl Shared {
    /// The song as the player has it, for a new one to pick up where it
    /// was, playing or paused.
    fn carried(&self) -> Option<Load> {
        let state = lock(&self.state).clone();
        log::info!(
            "new player: was {:?} at {} ms (last reported {} ms)",
            state.status,
            state.position_now(),
            state.position_ms,
        );
        let playing = matches!(state.status, Status::Playing | Status::Loading);
        if state.status == Status::Stopped {
            return None;
        }
        let uri = SpotifyUri::from_uri(&state.track.as_ref()?.uri).ok()?;
        Some((uri, playing, state.position_now()))
    }

    /// Replaces the player with one on the waiting session, if there is
    /// one, loading `then`.
    fn renew(self: &Arc<Self>, current: &mut Current, then: Option<Load>) {
        if let Some(session) = current.waiting.take() {
            current.session = session;
        }
        current.live.store(false, Ordering::SeqCst);
        current.player.stop();
        current.live = Arc::new(AtomicBool::new(true));
        *lock(&self.state) = PlayerSnapshot::default();
        current.player = start_player(
            &Arc::downgrade(self),
            &current.session,
            &self.mixer,
            current.levelling,
            &current.live,
            &self.notify,
        );
        if let Some((track, playing, position_ms)) = then {
            current.player.load(track, playing, position_ms);
        }
    }
}

/// Makes a player on the session and starts following its events.
fn start_player(
    shared: &Weak<Shared>,
    session: &Session,
    mixer: &Arc<dyn Mixer>,
    levelling: bool,
    live: &Arc<AtomicBool>,
    notify: &Notify,
) -> Arc<Player> {
    // A device by name, for testing without one: any name that doesn't
    // exist acts as a missing output.
    #[cfg(not(target_os = "android"))]
    let device = std::env::var("SPOTLESS_AUDIO_DEVICE").ok();
    let on_error = {
        let notify = Arc::clone(notify);
        let live = Arc::clone(live);
        Box::new(move |message| {
            if live.load(Ordering::SeqCst) {
                notify(EngineEvent::OutputError(message));
            }
        })
    };
    let config = PlayerConfig {
        bitrate: Bitrate::Bitrate320,
        gapless: true,
        normalisation: levelling,
        // Album gain while an album plays through, track gain otherwise.
        normalisation_type: NormalisationType::Auto,
        position_update_interval: Some(Duration::from_secs(1)),
        ..PlayerConfig::default()
    };
    let player = Player::new(
        config,
        session.clone(),
        mixer.get_soft_volume(),
        #[cfg(not(target_os = "android"))]
        move || Box::new(RodioSink::new(device, on_error)),
        #[cfg(target_os = "android")]
        move || Box::new(AAudioSink::new(on_error)),
    );
    // Subscribed before the first load, so its events are seen.
    let events = player.get_player_event_channel();
    tokio::spawn(follow_songs(
        Weak::clone(shared),
        session.clone(),
        Arc::clone(&player),
        events,
        Arc::clone(live),
        Arc::clone(notify),
    ));
    player
}

/// Preloads the followed song when librespot asks, starts it as the playing
/// one ends, and reports the player's state. Ends, letting the player go,
/// once the player has been replaced, or the engine let go.
async fn follow_songs(
    shared: Weak<Shared>,
    session: Session,
    player: Arc<Player>,
    mut events: librespot_playback::player::PlayerEventChannel,
    live: Arc<AtomicBool>,
    notify: Notify,
) {
    let mut state = PlayerSnapshot::default();
    let mut current = None;
    while let Some(event) = events.recv().await {
        let Some(shared) = shared.upgrade() else {
            break;
        };
        if !live.load(Ordering::SeqCst) {
            break;
        }
        if let PlayerEvent::PlayRequestIdChanged { play_request_id } = event {
            current = Some(play_request_id);
            continue;
        }
        // A late event from a song already replaced.
        if event.get_play_request_id().is_some() && event.get_play_request_id() != current {
            continue;
        }
        // librespot reports a song it failed to load ahead, to follow the
        // playing one, as unavailable too, under the playing song's
        // request. The playing song goes on; the next one is loaded again
        // when it is due, and only a failure then counts.
        if matches!(event, PlayerEvent::Unavailable { .. }) && state.status != Status::Loading {
            log::info!("the next song didn't load ahead; it is tried again when due");
            continue;
        }
        // A song that didn't load, or whose audio stopped coming, for want
        // of a connection, with the session's socket yet to notice (it can
        // take a minute and a half): the session is let go, so a new one is
        // made once Spotify answers, and the song goes on then.
        if matches!(
            event,
            PlayerEvent::Unavailable {
                unreachable: true,
                ..
            } | PlayerEvent::EndOfTrack { stalled: true, .. }
        ) && !session.is_invalid()
        {
            log::info!("Spotify couldn't be reached for the song; the session is let go");
            session.shutdown();
        }
        let offline = matches!(event, PlayerEvent::Unavailable { .. }) && session.is_invalid();
        // librespot ends a song whose audio stops coming, as if it were
        // over; offline, the next would fail too. Near its end, it's over.
        let cut = matches!(event, PlayerEvent::EndOfTrack { .. })
            && session.is_invalid()
            && state.track.as_ref().is_some_and(|track| {
                state.position_ms.saturating_add(CUT_MARGIN_MS) < track.duration_ms
            });
        // How a song ended or failed, and with the session up or not, is
        // what tells an offline stop from a real one.
        let session_state = if session.is_invalid() {
            "dropped"
        } else {
            "up"
        };
        match &event {
            PlayerEvent::EndOfTrack { .. } => log::info!(
                "song ended at {} of {} ms, session {session_state}{}",
                state.position_ms,
                state.track.as_ref().map_or(0, |t| t.duration_ms),
                if cut { ": cut short" } else { "" },
            ),
            PlayerEvent::Unavailable { .. } => {
                log::info!("song unavailable, session {session_state}")
            }
            _ => {}
        }
        // A new session waiting for this song to end: the next player is
        // made on it, and this one's done.
        if matches!(event, PlayerEvent::EndOfTrack { .. }) {
            let mut current = lock(&shared.current);
            if current.waiting.is_some() {
                let next = if cut {
                    let uri = state.track.as_ref().map(|t| SpotifyUri::from_uri(&t.uri));
                    if let Some(Ok(uri)) = uri {
                        log::info!("going on from {} ms on the new session", state.position_ms);
                        shared.renew(&mut current, Some((uri, true, state.position_ms)));
                    } else {
                        shared.renew(&mut current, None);
                    }
                    None
                } else {
                    let next = lock(&shared.follow).take();
                    shared.renew(&mut current, next.clone().map(|(_, uri)| (uri, true, 0)));
                    Some(next)
                };
                drop(current);
                match next {
                    Some(Some((entry, _))) => notify(EngineEvent::Followed(entry)),
                    // The new player has nothing to report.
                    Some(None) => {
                        notify(EngineEvent::Ended);
                        notify(EngineEvent::Snapshot(PlayerSnapshot::default()));
                    }
                    None => {}
                }
                break;
            }
        }
        // The follow lock is let go before anything is told: a hook that
        // answers with `Engine::follow` takes it too.
        match &event {
            PlayerEvent::TimeToPreloadNextTrack { .. } => {
                let next = lock(&shared.follow).clone();
                if let Some((_, next)) = next
                    && lock(&shared.current).waiting.is_none()
                {
                    player.preload(next);
                }
            }
            PlayerEvent::EndOfTrack { .. } if cut => {
                player.stop();
                notify(EngineEvent::Cut {
                    position_ms: state.position_ms,
                });
            }
            PlayerEvent::EndOfTrack { .. } => {
                let next = lock(&shared.follow).take();
                match next {
                    Some((entry, next)) => {
                        player.load(next, true, 0);
                        notify(EngineEvent::Followed(entry));
                    }
                    None => {
                        player.stop();
                        notify(EngineEvent::Ended);
                    }
                }
            }
            PlayerEvent::Unavailable { .. } => notify(EngineEvent::Unavailable { offline }),
            _ => {}
        }
        if apply(&mut state, event) {
            *lock(&shared.state) = state.clone();
            notify(EngineEvent::Snapshot(state.clone()));
        }
    }
    // Dropping a player waits for its thread, which may be finishing audio.
    tokio::task::spawn_blocking(move || drop(player));
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}

/// Folds one player event into the snapshot. Returns whether it changed.
fn apply(state: &mut PlayerSnapshot, event: PlayerEvent) -> bool {
    let before = state.clone();
    let now = Some(Instant::now());
    match event {
        PlayerEvent::Loading { position_ms, .. } => {
            state.status = Status::Loading;
            state.position_ms = position_ms;
            state.reported_at = now;
            state.error = None;
        }
        PlayerEvent::Playing { position_ms, .. } => {
            state.status = Status::Playing;
            state.position_ms = position_ms;
            state.reported_at = now;
        }
        PlayerEvent::Paused { position_ms, .. } => {
            state.status = Status::Paused;
            state.position_ms = position_ms;
            state.reported_at = now;
        }
        PlayerEvent::PositionChanged { position_ms, .. }
        | PlayerEvent::PositionCorrection { position_ms, .. }
        | PlayerEvent::Seeked { position_ms, .. } => {
            state.position_ms = position_ms;
            state.reported_at = now;
        }
        // The end of a song is followed by a load or a stop.
        PlayerEvent::Stopped { .. } => {
            state.status = Status::Stopped;
            state.position_ms = 0;
            state.reported_at = now;
        }
        PlayerEvent::TrackChanged { audio_item } => state.track = Some(track_info(&audio_item)),
        PlayerEvent::Unavailable { .. } => {
            state.status = Status::Stopped;
            state.error = Some("This song can't be played.".into());
        }
        PlayerEvent::AudioKeyUnavailable { .. } => {
            state.status = Status::Stopped;
            state.error = Some("Spotify refused to play this song. Playback needs Premium.".into());
        }
        _ => return false,
    }
    // Position reports alone change only the timestamp; skip those repaints
    // unless the position really moved.
    *state != before
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

fn track_info(item: &AudioItem) -> TrackInfo {
    let (artists, main_artist, album) = match &item.unique_fields {
        UniqueFields::Track { artists, album, .. } => (
            artists
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            artists.first().map(|a| a.name.clone()).unwrap_or_default(),
            album.clone(),
        ),
        UniqueFields::Local { artists, album, .. } => {
            let artists = artists.clone().unwrap_or_default();
            (artists.clone(), artists, album.clone().unwrap_or_default())
        }
        UniqueFields::Episode { show_name, .. } => {
            (show_name.clone(), show_name.clone(), String::new())
        }
    };
    TrackInfo {
        uri: item.uri.clone(),
        title: item.name.clone(),
        artists,
        main_artist,
        album,
        duration_ms: item.duration_ms,
        cover_url: item
            .covers
            .iter()
            .find(|cover| cover.size == librespot_metadata::image::ImageSize::DEFAULT)
            .or_else(|| item.covers.first())
            .map(|cover| cover.url.clone()),
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
