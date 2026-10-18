//! A bare librespot `Player` on the session. There is no Spirc: Spotify
//! Connect never hears of it, so the account's queue and other devices are
//! left alone (as spotifast's local-queue player proved).
//!
//! Player events are folded into a [`PlayerSnapshot`] that is handed to the
//! interface whenever it changes.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use librespot_core::SpotifyUri;
use librespot_core::session::Session;
use librespot_metadata::audio::{AudioItem, UniqueFields};
use librespot_playback::audio_backend;
use librespot_playback::config::{AudioFormat, Bitrate, PlayerConfig, VolumeCtrl};
use librespot_playback::mixer::{self, Mixer, MixerConfig};
use librespot_playback::player::{Player, PlayerEvent};

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

pub type Notify = Arc<dyn Fn(PlayerSnapshot) + Send + Sync>;

pub struct Engine {
    player: Arc<Player>,
    mixer: Arc<dyn Mixer>,
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

        let backend = audio_backend::find(None).ok_or_else(|| anyhow!("no audio backend"))?;
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
            move || backend(None, AudioFormat::default()),
        );

        let mut events = player.get_player_event_channel();
        let state = Arc::new(Mutex::new(PlayerSnapshot::default()));
        tokio::spawn(async move {
            while let Some(event) = events.recv().await {
                let snapshot = {
                    let mut current = state.lock().unwrap_or_else(|p| p.into_inner());
                    if !apply(&mut current, event) {
                        continue;
                    }
                    current.clone()
                };
                notify(snapshot);
            }
        });
        Ok(Self { player, mixer })
    }

    pub fn load(&self, track: SpotifyUri) {
        self.player.load(track, true, 0);
    }

    pub fn play(&self) {
        self.player.play();
    }

    pub fn pause(&self) {
        self.player.pause();
    }

    pub fn stop(&self) {
        self.player.stop();
    }

    pub fn seek(&self, position_ms: u32) {
        self.player.seek(position_ms);
    }

    pub fn set_volume(&self, volume: u16) {
        self.mixer.set_volume(volume);
    }
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
        // M0 has nothing to follow a song with, so its end stops playback.
        PlayerEvent::Stopped { .. } | PlayerEvent::EndOfTrack { .. } => {
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
