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
//! the old one was.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use librespot_core::SpotifyUri;
use librespot_core::session::Session;
use librespot_metadata::audio::{AudioItem, UniqueFields};
use librespot_playback::config::{Bitrate, NormalisationType, PlayerConfig, VolumeCtrl};
use librespot_playback::mixer::{self, Mixer, MixerConfig};
use librespot_playback::player::{Player, PlayerEvent};

use super::sink::RodioSink;
use crate::model::EntryId;

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
    /// The position now, counting on from the last report while playing.
    pub fn position_now(&self) -> u32 {
        let mut position = self.position_ms;
        if self.status == Status::Playing
            && let Some(at) = self.reported_at
        {
            position = position.saturating_add(at.elapsed().as_millis() as u32);
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
    /// The loaded song can't be played.
    Unavailable,
    /// The audio output is missing or failed; the player paused.
    OutputError(String),
}

pub type Notify = Arc<dyn Fn(EngineEvent) + Send + Sync>;

type Follow = Arc<Mutex<Option<(EntryId, SpotifyUri)>>>;

pub struct Engine {
    session: Session,
    player: Arc<Player>,
    mixer: Arc<dyn Mixer>,
    follow: Follow,
    notify: Notify,
    /// Volume levelling, as the player was made with.
    levelling: bool,
    /// The player's state as last reported, to carry over to a new player.
    state: Arc<Mutex<PlayerSnapshot>>,
    /// Cleared when the player is replaced: its events are no longer told.
    live: Arc<AtomicBool>,
}

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
        let follow: Follow = Arc::default();
        let state = Arc::default();
        let live = Arc::new(AtomicBool::new(true));
        let player = start_player(session, &mixer, levelling, &follow, &state, &live, &notify);
        Ok(Self {
            session: session.clone(),
            player,
            mixer,
            follow,
            notify,
            levelling,
            state,
            live,
        })
    }

    /// Turns volume levelling on or off with a new player, which goes on
    /// with the song (playing or paused) and the song to follow it.
    pub fn set_levelling(&mut self, on: bool) {
        if on == self.levelling {
            return;
        }
        self.levelling = on;
        self.restart_player();
    }

    /// Plays on a new session, for one that dropped, with a new player that
    /// goes on as `set_levelling`'s does.
    pub fn set_session(&mut self, session: &Session) {
        self.session = session.clone();
        self.restart_player();
    }

    /// Replaces the player with one that picks up the song where the old one
    /// was, playing or paused, followed by the same song.
    fn restart_player(&mut self) {
        let state = lock(&self.state).clone();
        self.live.store(false, Ordering::SeqCst);
        self.player.stop();
        self.live = Arc::new(AtomicBool::new(true));
        *lock(&self.state) = PlayerSnapshot::default();
        self.player = start_player(
            &self.session,
            &self.mixer,
            self.levelling,
            &self.follow,
            &self.state,
            &self.live,
            &self.notify,
        );
        let playing = matches!(state.status, Status::Playing | Status::Loading);
        if state.status != Status::Stopped
            && let Some(track) = state.track.as_ref()
            && let Ok(uri) = SpotifyUri::from_uri(&track.uri)
        {
            self.player.load(uri, playing, state.position_now());
        }
    }

    /// Plays `track` now, following it with nothing until told otherwise.
    pub fn load(&self, track: SpotifyUri, position_ms: u32) {
        *lock(&self.follow) = None;
        self.player.load(track, true, position_ms);
    }

    /// The song to start when the playing one ends. None: playback stops.
    pub fn follow(&self, next: Option<(EntryId, SpotifyUri)>) {
        *lock(&self.follow) = next;
    }

    pub fn play(&self) {
        self.player.play();
    }

    pub fn pause(&self) {
        self.player.pause();
    }

    pub fn stop(&self) {
        *lock(&self.follow) = None;
        self.player.stop();
    }

    pub fn seek(&self, position_ms: u32) {
        self.player.seek(position_ms);
    }

    pub fn set_volume(&self, volume: u16) {
        self.mixer.set_volume(volume);
    }
}

/// Makes a player on the session and starts following its events.
fn start_player(
    session: &Session,
    mixer: &Arc<dyn Mixer>,
    levelling: bool,
    follow: &Follow,
    state: &Arc<Mutex<PlayerSnapshot>>,
    live: &Arc<AtomicBool>,
    notify: &Notify,
) -> Arc<Player> {
    // A device by name, for testing without one: any name that doesn't
    // exist acts as a missing output.
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
        move || Box::new(RodioSink::new(device, on_error)),
    );
    // Subscribed before the first load, so its events are seen.
    let events = player.get_player_event_channel();
    tokio::spawn(follow_songs(
        Arc::clone(&player),
        events,
        Arc::clone(follow),
        Arc::clone(state),
        Arc::clone(live),
        Arc::clone(notify),
    ));
    player
}

/// Preloads the followed song when librespot asks, starts it as the playing
/// one ends, and reports the player's state. Ends, letting the player go,
/// once the player has been replaced.
async fn follow_songs(
    player: Arc<Player>,
    mut events: librespot_playback::player::PlayerEventChannel,
    follow: Follow,
    shared: Arc<Mutex<PlayerSnapshot>>,
    live: Arc<AtomicBool>,
    notify: Notify,
) {
    let mut state = PlayerSnapshot::default();
    let mut current = None;
    while let Some(event) = events.recv().await {
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
        // The follow lock is let go before anything is told: a hook that
        // answers with `Engine::follow` takes it too.
        match &event {
            PlayerEvent::TimeToPreloadNextTrack { .. } => {
                let next = lock(&follow).clone();
                if let Some((_, next)) = next {
                    player.preload(next);
                }
            }
            PlayerEvent::EndOfTrack { .. } => {
                let next = lock(&follow).take();
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
            PlayerEvent::Unavailable { .. } => notify(EngineEvent::Unavailable),
            _ => {}
        }
        if apply(&mut state, event) {
            *lock(&shared) = state.clone();
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
