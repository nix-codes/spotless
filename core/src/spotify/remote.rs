//! Spotless as a Spotify Connect device, for the Spotify tab.
//!
//! Local playlists never go through here (see `devices.rs`). This is a
//! second, visible device on its own session: playback can be handed to it
//! from any of the account's devices, it plays the account's queue with a
//! player of its own, and it tells the tab what the queue holds, whichever
//! device plays it. While another device plays, it only watches.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use librespot_connect::{ClusterState, ConnectConfig, Spirc};
use librespot_core::config::DeviceType;
use librespot_core::{Session, SessionConfig, SpotifyUri};
use librespot_playback::config::{Bitrate, NormalisationType, PlayerConfig, VolumeCtrl};
use librespot_playback::mixer::{self, Mixer, MixerConfig};
use librespot_playback::player::Player;
use librespot_protocol::player::{PlayerState, ProvidedTrack};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use super::catalog;
use super::queue::{Part, Queued, SpotifyQueue};
use crate::model::TrackRef;
use crate::paths::Paths;
use crate::playback::engine::Status;
use crate::playback::sink::RodioSink;

/// Changes are told once they have rested this long: a handover sends a
/// short queue before the whole one.
const SETTLE: Duration = Duration::from_millis(300);
/// The songs from the album or playlist shown, at most.
const FROM_CONTEXT: usize = 50;
/// The songs played before shown, at most.
const BEFORE: usize = 10;

pub struct Remote {
    spirc: Spirc,
    session: Session,
    mixer: Arc<dyn Mixer>,
    follower: JoinHandle<()>,
}

impl Remote {
    /// Joins Connect as "Spotless" on a session of its own. `report` is
    /// told the queue whenever it changes.
    pub async fn start(
        paths: &Paths,
        volume: u16,
        levelling: bool,
        report: impl Fn(SpotifyQueue) + Send + Sync + 'static,
    ) -> Result<Self> {
        let config = SessionConfig {
            device_id: device_id(paths),
            ..SessionConfig::default()
        };
        let Some((session, credentials)) = super::session::unconnected(paths, config)? else {
            bail!("not signed in");
        };
        let mixer = mixer::find(Some("softvol")).context("soft volume mixer missing")?;
        // As the engine's: see `engine.rs`.
        let mixer = mixer(MixerConfig {
            volume_ctrl: VolumeCtrl::Cubic(VolumeCtrl::DEFAULT_DB_RANGE),
            ..MixerConfig::default()
        })
        .context("unable to create the mixer")?;
        mixer.set_volume(volume);
        let device = std::env::var("SPOTLESS_AUDIO_DEVICE").ok();
        let player = Player::new(
            PlayerConfig {
                bitrate: Bitrate::Bitrate320,
                gapless: true,
                normalisation: levelling,
                normalisation_type: NormalisationType::Auto,
                position_update_interval: Some(Duration::from_secs(1)),
                ..PlayerConfig::default()
            },
            session.clone(),
            mixer.get_soft_volume(),
            move || {
                Box::new(RodioSink::new(
                    device,
                    Box::new(|message| log::warn!("Spotify tab output: {message}")),
                ))
            },
        );
        let connect = ConnectConfig {
            name: "Spotless".into(),
            device_type: DeviceType::Computer,
            initial_volume: volume,
            // Spotless's own slider sets it.
            disable_volume: true,
            // Queue changes are told at once, not at the next song.
            emit_set_queue_events: true,
            ..ConnectConfig::default()
        };
        let (spirc, task) = Spirc::new(
            connect,
            session.clone(),
            credentials,
            player,
            Arc::clone(&mixer),
        )
        .await
        .context("unable to join Spotify Connect")?;
        tokio::spawn(task);
        let follower = tokio::spawn(follow(
            session.clone(),
            spirc.watch_player_state(),
            spirc.watch_cluster_state(),
            report,
        ));
        Ok(Self {
            spirc,
            session,
            mixer,
            follower,
        })
    }

    /// Its device ID, as other devices see it.
    pub fn device_id(&self) -> &str {
        self.session.device_id()
    }

    /// Spotify dropped its session.
    pub fn is_invalid(&self) -> bool {
        self.session.is_invalid()
    }

    pub fn play(&self) {
        log_error(self.spirc.play());
    }

    pub fn pause(&self) {
        log_error(self.spirc.pause());
    }

    pub fn toggle_pause(&self) {
        log_error(self.spirc.play_pause());
    }

    pub fn next(&self) {
        log_error(self.spirc.next());
    }

    pub fn previous(&self) {
        log_error(self.spirc.prev());
    }

    pub fn seek(&self, position_ms: u32) {
        log_error(self.spirc.set_position_ms(position_ms));
    }

    pub fn set_volume(&self, volume: u16) {
        self.mixer.set_volume(volume);
    }

    /// Adds songs, or an album's, to the end of the queue. Only while
    /// Spotless plays it: Spotify takes no changes from a device that only
    /// watches.
    pub fn add(&self, uris: &[String]) {
        for uri in uris {
            match SpotifyUri::from_uri(uri) {
                Ok(uri) => log_error(self.spirc.add_to_queue(uri)),
                Err(error) => log::warn!("not a Spotify URI {uri:?}: {error}"),
            }
        }
    }

    /// Removes the songs to come at these places in Spotify's list.
    pub fn remove(&self, mut songs: Vec<(usize, String)>) {
        // From the last, so the earlier places stay where they are.
        songs.sort_by_key(|song| std::cmp::Reverse(song.0));
        for (index, uri) in songs {
            match SpotifyUri::from_uri(&uri) {
                Ok(uri) => log_error(self.spirc.remove_next_track(index, uri)),
                Err(error) => log::warn!("not a Spotify URI {uri:?}: {error}"),
            }
        }
    }

    /// Takes playback over from the device playing it.
    pub fn play_here(&self) {
        log_error(self.spirc.transfer(None));
    }

    /// Pauses and lets playback go, for local playback to start: no device
    /// is the active one after.
    pub fn leave(&self) {
        log_error(self.spirc.disconnect(true));
    }

    /// Leaves Connect.
    pub fn shut_down(self) {
        self.follower.abort();
        log_error(self.spirc.shutdown());
    }
}

fn log_error(result: Result<(), librespot_core::Error>) {
    if let Err(error) = result {
        log::warn!("Spotify Connect: {error}");
    }
}

/// The same device ID every start, so other devices list one Spotless
/// rather than a new one each time. Made once and saved.
fn device_id(paths: &Paths) -> String {
    let path = paths.connect_device();
    if let Ok(id) = std::fs::read_to_string(&path) {
        let id = id.trim();
        if id.len() == 40 && id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return id.to_string();
        }
    }
    let id: String = (0..40)
        .map(|_| char::from_digit(fastrand::u32(..16), 16).unwrap_or('0'))
        .collect();
    if let Err(error) = std::fs::write(&path, &id) {
        log::warn!("unable to save {}: {error}", path.display());
    }
    id
}

/// Tells the queue, read into names, whenever it or the active device
/// changes and has settled.
async fn follow(
    session: Session,
    mut states: watch::Receiver<Option<PlayerState>>,
    mut clusters: watch::Receiver<ClusterState>,
    report: impl Fn(SpotifyQueue),
) {
    let mut names: HashMap<String, TrackRef> = HashMap::new();
    let mut last = None;
    loop {
        tokio::select! {
            changed = states.changed() => if changed.is_err() { break },
            changed = clusters.changed() => if changed.is_err() { break },
        }
        tokio::time::sleep(SETTLE).await;
        let state = states.borrow_and_update().clone();
        let cluster = clusters.borrow_and_update().clone();
        let queue = match state {
            Some(state) => {
                name(&session, &state, &mut names).await;
                read(&session, &state, &cluster, &names)
            }
            None => SpotifyQueue::default(),
        };
        if last.as_ref() != Some(&queue) {
            last = Some(queue.clone());
            report(queue);
        }
    }
}

/// The songs shown in `state`, as Spotify plays them.
fn shown(state: &PlayerState) -> impl Iterator<Item = &ProvidedTrack> {
    let before = state.prev_tracks.len().saturating_sub(BEFORE);
    let mut from_context = 0;
    state.prev_tracks[before..]
        .iter()
        .chain(state.track.as_ref())
        .chain(state.next_tracks.iter().filter(move |track| {
            if track.provider == "queue" {
                return true;
            }
            from_context += 1;
            from_context <= FROM_CONTEXT
        }))
        .filter(|track| track.uri.starts_with("spotify:track:"))
}

/// Reads the names of the songs shown that aren't known yet.
async fn name(session: &Session, state: &PlayerState, names: &mut HashMap<String, TrackRef>) {
    let unknown: Vec<SpotifyUri> = shown(state)
        .filter(|track| !names.contains_key(&track.uri))
        .filter_map(|track| SpotifyUri::from_uri(&track.uri).ok())
        .collect();
    if unknown.is_empty() {
        return;
    }
    match catalog::tracks(session, unknown).await {
        Ok(tracks) => names.extend(tracks.into_iter().map(|t| (t.uri.clone(), t))),
        Err(error) => log::warn!("unable to name the queue's songs: {error:#}"),
    }
}

fn read(
    session: &Session,
    state: &PlayerState,
    cluster: &ClusterState,
    names: &HashMap<String, TrackRef>,
) -> SpotifyQueue {
    let active = cluster.active_device_id.as_deref();
    let device = active.map(|id| {
        cluster
            .devices
            .get(id)
            .map_or_else(|| "another device".to_string(), |d| d.device_alias.clone())
    });
    let here = active == Some(session.device_id());
    let status = match (device.is_some(), state.is_playing, state.is_paused) {
        (true, true, false) => Status::Playing,
        (true, true, true) => Status::Paused,
        _ => Status::Stopped,
    };
    let named = |track: &ProvidedTrack| names.get(&track.uri).cloned();
    let before = state.prev_tracks.len().saturating_sub(BEFORE);
    let mut parts: Vec<Part> = Vec::new();
    for (index, track) in state.next_tracks.iter().enumerate() {
        let Some(named) = named(track) else { continue };
        let queued = track.provider == "queue";
        if !queued
            && parts
                .iter()
                .filter(|p| !p.queued)
                .map(|p| p.songs.len())
                .sum::<usize>()
                >= FROM_CONTEXT
        {
            continue;
        }
        let title = if queued {
            "Next in queue".to_string()
        } else if track.provider == "autoplay" {
            "Next from: Autoplay".to_string()
        } else {
            format!("Next from: {}", context_name(state, &named))
        };
        if parts.last().is_none_or(|part| part.title != title) {
            parts.push(Part {
                title,
                queued,
                songs: Vec::new(),
            });
        }
        if let Some(part) = parts.last_mut() {
            part.songs.push(Queued {
                track: named,
                index,
            });
        }
    }
    SpotifyQueue {
        device,
        here,
        status,
        now: state.track.as_ref().and_then(named),
        position_ms: position(state, status),
        reported_at: Some(Instant::now()),
        parts,
        before: state.prev_tracks[before..]
            .iter()
            .filter_map(named)
            .collect(),
    }
}

/// The album or playlist the songs come from, as well as Spotify says.
fn context_name(state: &PlayerState, track: &TrackRef) -> String {
    if let Some(name) = state
        .context_metadata
        .get("context_description")
        .filter(|name| !name.is_empty())
    {
        return name.clone();
    }
    let uri = state.context_uri.as_str();
    if uri.starts_with("spotify:album:") {
        track.album.name.clone()
    } else if uri.starts_with("spotify:playlist:") {
        "a playlist".into()
    } else if uri.starts_with("spotify:artist:") {
        "an artist".into()
    } else if uri.contains(":collection") {
        "Liked Songs".into()
    } else {
        "what was playing".into()
    }
}

/// The position now, from the one Spotify gave and when.
fn position(state: &PlayerState, status: Status) -> u32 {
    let mut position = state.position_as_of_timestamp;
    if status == Status::Playing && state.timestamp > 0 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64);
        position += (now - state.timestamp).max(0);
    }
    position.clamp(0, state.duration.max(0)) as u32
}
