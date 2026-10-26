//! Can Spotless be a full Spotify Connect device, and show the account's
//! queue the way Spotify's own apps do?
//!
//! Joins Connect as a visible device, "Spotless probe", playing at volume 0
//! through Spotless's sink, and prints what Spotify says plays: the active
//! device, the song, then the queue in its parts (Next in queue, Next from
//! the album or playlist, autoplay) and what played before. With `--add`,
//! 20 s after playback comes here, it adds one song to the queue, as
//! sending a song to the Spotify tab would.
//!
//! While it runs: play on a phone, then pick "Spotless probe" in the phone's
//! device list, queue songs on the phone, skip, and take playback back.
//!
//!   cargo run -p spotless-core --example connect_probe -- [--add]
//!
//! Sign in through the app first: this reuses its saved credential.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};
use librespot_connect::{ConnectConfig, Spirc};
use librespot_core::cache::Cache;
use librespot_core::config::DeviceType;
use librespot_core::{Session, SessionConfig, SpotifyUri};
use librespot_playback::config::{Bitrate, PlayerConfig};
use librespot_playback::mixer::{self, MixerConfig};
use librespot_playback::player::Player;
use librespot_protocol::player::{PlayerState, ProvidedTrack};
use spotless_core::paths::Paths;
use spotless_core::playback::sink::RodioSink;
use spotless_core::spotify::catalog;

/// Boards of Canada, Music Has the Right to Children: `--add` queues a song
/// from it.
const ALBUM: &str = "spotify:album:1vWnB0hYmluskQuzxwo25a";

const LISTEN: Duration = Duration::from_secs(420);
const ADD_AFTER: Duration = Duration::from_secs(20);
/// Songs shown per part of the queue.
const SHOWN: usize = 5;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .format_timestamp_millis()
        .init();
    let add = std::env::args().any(|arg| arg == "--add");
    let paths = Paths::new()?;
    let cache = Cache::new(Some(paths.config.as_path()), None, None, None)?;
    let Some(credentials) = cache.credentials() else {
        bail!("not signed in: start Spotless and sign in first");
    };
    let session = Session::new(SessionConfig::default(), Some(cache));

    let mixer = mixer::find(Some("softvol")).ok_or_else(|| anyhow!("no soft volume mixer"))?;
    let mixer = mixer(MixerConfig::default())?;
    mixer.set_volume(0);
    let player = Player::new(
        PlayerConfig {
            bitrate: Bitrate::Bitrate320,
            gapless: true,
            ..PlayerConfig::default()
        },
        session.clone(),
        mixer.get_soft_volume(),
        || {
            Box::new(RodioSink::new(
                None,
                Box::new(|e| eprintln!("output: {e}")),
                Box::new(librespot_playback::mixer::NoOpVolume),
            ))
        },
    );
    let config = ConnectConfig {
        name: "Spotless probe".into(),
        device_type: DeviceType::Computer,
        initial_volume: 0,
        disable_volume: true,
        ..ConnectConfig::default()
    };
    let (spirc, task) = Spirc::new(config, session.clone(), credentials, player, mixer).await?;
    tokio::spawn(task);
    println!(
        "joined as \"Spotless probe\", device {}",
        session.device_id()
    );

    let mut states = spirc.watch_player_state();
    let clusters = spirc.watch_cluster_state();
    let mut names: HashMap<String, String> = HashMap::new();
    let mut shown = String::new();
    let mut add_at: Option<Instant> = None;
    let mut added = false;
    let began = Instant::now();
    while began.elapsed() < LISTEN {
        let changed = tokio::time::timeout(Duration::from_millis(500), states.changed()).await;
        let t = began.elapsed().as_secs_f32();
        if add_at.take_if(|at| Instant::now() >= *at).is_some() {
            added = true;
            let tracks = catalog::link_tracks(&session, ALBUM).await?;
            let song = &tracks[2];
            println!("{t:6.1}s  adding {} to the queue", song.title);
            spirc.add_to_queue(SpotifyUri::from_uri(&song.uri)?)?;
        }
        if !matches!(changed, Ok(Ok(()))) {
            continue;
        }
        let Some(state) = states.borrow_and_update().clone() else {
            continue;
        };
        let (active, here) = {
            let cluster = clusters.borrow();
            let id = cluster.active_device_id.clone().unwrap_or_default();
            let name = cluster
                .devices
                .get(&id)
                .map_or_else(|| id.clone(), |d| d.device_alias.clone());
            (name, id == session.device_id())
        };
        if add && !added && add_at.is_none() && here && state.is_playing {
            add_at = Some(Instant::now() + ADD_AFTER);
        }
        name_tracks(&session, &state, &mut names).await;
        let text = describe(&state, &active, here, &names);
        if text != shown {
            println!("{t:6.1}s  {text}\n");
            shown = text;
        }
    }
    spirc.shutdown()?;
    println!("left Connect");
    Ok(())
}

/// Reads the names of the songs in `state` not named yet.
async fn name_tracks(session: &Session, state: &PlayerState, names: &mut HashMap<String, String>) {
    let unnamed: Vec<SpotifyUri> = all_tracks(state)
        .filter(|t| t.uri.starts_with("spotify:track:") && !names.contains_key(&t.uri))
        .filter_map(|t| SpotifyUri::from_uri(&t.uri).ok())
        .collect();
    if unnamed.is_empty() {
        return;
    }
    match catalog::tracks(session, unnamed).await {
        Ok(tracks) => {
            for track in tracks {
                let artists: Vec<&str> = track.artists.iter().map(|a| a.name.as_str()).collect();
                names.insert(
                    track.uri,
                    format!("{} — {}", track.title, artists.join(", ")),
                );
            }
        }
        Err(error) => eprintln!("names: {error:#}"),
    }
}

fn all_tracks(state: &PlayerState) -> impl Iterator<Item = &ProvidedTrack> {
    state
        .track
        .as_ref()
        .into_iter()
        .chain(&state.prev_tracks)
        .chain(&state.next_tracks)
}

fn describe(
    state: &PlayerState,
    active: &str,
    here: bool,
    names: &HashMap<String, String>,
) -> String {
    let name = |t: &ProvidedTrack| names.get(&t.uri).cloned().unwrap_or_else(|| t.uri.clone());
    let status = match (state.is_playing, state.is_paused) {
        (true, false) => "playing",
        (true, true) => "paused",
        _ => "stopped",
    };
    let device = if here {
        "HERE (Spotless probe)"
    } else if active.is_empty() {
        "no device"
    } else {
        active
    };
    let mut text = format!("{device} {status}, from {}", state.context_uri);
    if let Some(track) = state.track.as_ref() {
        text += &format!("\n          now [{}] {}", track.provider, name(track));
    }
    // The parts in the order they play, by who put the songs there.
    let mut parts: Vec<(&str, Vec<&ProvidedTrack>)> = Vec::new();
    for track in state
        .next_tracks
        .iter()
        .filter(|t| !t.uri.contains("delimiter"))
    {
        match parts.last_mut() {
            Some((provider, tracks)) if *provider == track.provider => tracks.push(track),
            _ => parts.push((&track.provider, vec![track])),
        }
    }
    for (provider, tracks) in parts {
        text += &format!("\n          next [{provider}] {} songs:", tracks.len());
        for track in tracks.iter().take(SHOWN) {
            text += &format!("\n            {}", name(track));
        }
    }
    let before: Vec<_> = state.prev_tracks.iter().rev().take(3).collect();
    text += &format!("\n          before: {} songs", state.prev_tracks.len());
    for track in before {
        text += &format!("\n            {}", name(track));
    }
    text
}
