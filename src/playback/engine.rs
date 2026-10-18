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

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use librespot_core::SpotifyUri;
use librespot_core::session::Session;
use librespot_metadata::audio::{AudioItem, UniqueFields};
use librespot_playback::config::{Bitrate, PlayerConfig, VolumeCtrl};
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
    pub album: String,
    pub duration_ms: u32,
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
    player: Arc<Player>,
    mixer: Arc<dyn Mixer>,
    follow: Follow,
}

impl Engine {
    /// Must be called on the tokio runtime: the event fold runs on it.
    pub fn new(session: &Session, volume: u16, notify: Notify) -> Result<Self> {
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

        // A device by name, for testing without one: any name that doesn't
        // exist acts as a missing output.
        let device = std::env::var("SPOTLESS_AUDIO_DEVICE").ok();
        let on_error = {
            let notify = Arc::clone(&notify);
            Box::new(move |message| notify(EngineEvent::OutputError(message)))
        };
        let config = PlayerConfig {
            bitrate: Bitrate::Bitrate320,
            gapless: true,
            position_update_interval: Some(Duration::from_secs(1)),
            ..PlayerConfig::default()
        };
        let player = Player::new(
            config,
            session.clone(),
            mixer.get_soft_volume(),
            move || Box::new(RodioSink::new(device, on_error)),
        );

        let follow: Follow = Arc::default();
        // Subscribed before the first load, so its events are seen.
        let events = player.get_player_event_channel();
        tokio::spawn(follow_songs(
            Arc::clone(&player),
            events,
            Arc::clone(&follow),
            notify,
        ));
        Ok(Self {
            player,
            mixer,
            follow,
        })
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

/// Preloads the followed song when librespot asks, starts it as the playing
/// one ends, and reports the player's state.
async fn follow_songs(
    player: Arc<Player>,
    mut events: librespot_playback::player::PlayerEventChannel,
    follow: Follow,
    notify: Notify,
) {
    let mut state = PlayerSnapshot::default();
    let mut current = None;
    while let Some(event) = events.recv().await {
        if let PlayerEvent::PlayRequestIdChanged { play_request_id } = event {
            current = Some(play_request_id);
            continue;
        }
        // A late event from a song already replaced.
        if event.get_play_request_id().is_some() && event.get_play_request_id() != current {
            continue;
        }
        match &event {
            PlayerEvent::TimeToPreloadNextTrack { .. } => {
                if let Some((_, next)) = lock(&follow).clone() {
                    player.preload(next);
                }
            }
            PlayerEvent::EndOfTrack { .. } => match lock(&follow).take() {
                Some((entry, next)) => {
                    player.load(next, true, 0);
                    notify(EngineEvent::Followed(entry));
                }
                None => {
                    player.stop();
                    notify(EngineEvent::Ended);
                }
            },
            PlayerEvent::Unavailable { .. } => notify(EngineEvent::Unavailable),
            _ => {}
        }
        if apply(&mut state, event) {
            notify(EngineEvent::Snapshot(state.clone()));
        }
    }
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
    let (artists, album) = match &item.unique_fields {
        UniqueFields::Track { artists, album, .. } => (
            artists
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            album.clone(),
        ),
        UniqueFields::Local { artists, album, .. } => (
            artists.clone().unwrap_or_default(),
            album.clone().unwrap_or_default(),
        ),
        UniqueFields::Episode { show_name, .. } => (show_name.clone(), String::new()),
    };
    TrackInfo {
        uri: item.uri.clone(),
        title: item.name.clone(),
        artists,
        album,
        duration_ms: item.duration_ms,
    }
}
