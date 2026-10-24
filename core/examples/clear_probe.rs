//! Can the Spotify tab be cleared: nothing playing, no queue, no context?
//!
//! Joins Connect as a visible device, "Spotless probe", and prints what
//! Spotify says plays, as `connect_probe` does. Type a letter and Enter:
//!
//!   c  clear: take playback over if another device has it (paused stays
//!      paused), then stop and drop the song, the queue and the context
//!   r  clear, then release playback: no device is left active
//!   t  only take playback over
//!   a  add a song (from Boards of Canada's first album) to the queue
//!   p  play    s  pause    n  next
//!   q  quit
//!
//!   cargo run -p spotless-core --example clear_probe 2>&1 | tee clear.log
//!
//! Needs the fork's `Spirc::clear` (branch clear-probe). Sign in through
//! the app first: this reuses its saved credential.

use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
use tokio::io::{AsyncBufReadExt, BufReader};

/// Boards of Canada, Music Has the Right to Children: `a` queues its
/// third song.
const ALBUM: &str = "spotify:album:1vWnB0hYmluskQuzxwo25a";
/// Played at about a third of full volume.
const VOLUME: u16 = u16::MAX / 3;
/// How long a clear waits for playback to come here.
const TAKE_OVER_WAIT: Duration = Duration::from_secs(10);
/// Songs shown per part of the queue.
const SHOWN: usize = 5;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .filter_module("librespot_connect", log::LevelFilter::Debug)
        .format_timestamp_millis()
        .init();
    let paths = Paths::new()?;
    let cache = Cache::new(Some(paths.config.as_path()), None, None, None)?;
    let Some(credentials) = cache.credentials() else {
        bail!("not signed in: start Spotless and sign in first");
    };
    let session = Session::new(SessionConfig::default(), Some(cache));

    let mixer = mixer::find(Some("softvol")).ok_or_else(|| anyhow!("no soft volume mixer"))?;
    let mixer = mixer(MixerConfig::default())?;
    mixer.set_volume(VOLUME);
    let player = Player::new(
        PlayerConfig {
            bitrate: Bitrate::Bitrate320,
            gapless: true,
            ..PlayerConfig::default()
        },
        session.clone(),
        mixer.get_soft_volume(),
        || Box::new(RodioSink::new(None, Box::new(|e| eprintln!("output: {e}")))),
    );
    let config = ConnectConfig {
        name: "Spotless probe".into(),
        device_type: DeviceType::Computer,
        initial_volume: VOLUME,
        ..ConnectConfig::default()
    };
    let (spirc, task) = Spirc::new(config, session.clone(), credentials, player, mixer).await?;
    tokio::spawn(task);
    println!(
        "joined as \"Spotless probe\", device {}\n\
         keys: c clear, r clear and release, t take over, a add, p play, s pause, n next, q quit",
        session.device_id()
    );

    let song = catalog::link_tracks(&session, ALBUM)
        .await?
        .into_iter()
        .nth(2)
        .ok_or_else(|| anyhow!("album too short"))?;
    println!("a adds {}", song.title);

    let mut states = spirc.watch_player_state();
    let mut clusters = spirc.watch_cluster_state();
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut names: HashMap<String, String> = HashMap::new();
    let mut shown = String::new();
    let mut clear_by: Option<Instant> = None;
    let mut release = false;
    let began = Instant::now();
    loop {
        let here = clusters.borrow().active_device_id.as_deref() == Some(session.device_id());
        if let Some(by) = clear_by {
            let t = began.elapsed().as_secs_f32();
            if here {
                println!("{t:6.1}s  CLEARING\n");
                spirc.clear()?;
                if release {
                    println!("{t:6.1}s  RELEASING\n");
                    spirc.disconnect(false)?;
                }
                clear_by = None;
            } else if Instant::now() >= by {
                println!("{t:6.1}s  playback didn't come here: not cleared\n");
                clear_by = None;
            }
        }
        tokio::select! {
            line = lines.next_line() => {
                let t = began.elapsed().as_secs_f32();
                match line?.as_deref().map(str::trim) {
                    None | Some("q") => break,
                    Some(key @ ("c" | "r")) if here => {
                        println!("{t:6.1}s  CLEARING\n");
                        spirc.clear()?;
                        if key == "r" {
                            println!("{t:6.1}s  RELEASING\n");
                            spirc.disconnect(false)?;
                        }
                    }
                    Some(key @ ("c" | "r")) => {
                        release = key == "r";
                        println!("{t:6.1}s  taking playback over, to clear it\n");
                        spirc.transfer(None)?;
                        clear_by = Some(Instant::now() + TAKE_OVER_WAIT);
                    }
                    Some("t") => {
                        println!("{t:6.1}s  taking playback over\n");
                        spirc.transfer(None)?;
                    }
                    Some("a") => {
                        println!("{t:6.1}s  adding {} to the queue\n", song.title);
                        spirc.add_to_queue(SpotifyUri::from_uri(&song.uri)?)?;
                    }
                    Some("p") => {
                        println!("{t:6.1}s  play\n");
                        spirc.play()?;
                    }
                    Some("s") => {
                        println!("{t:6.1}s  pause\n");
                        spirc.pause()?;
                    }
                    Some("n") => {
                        println!("{t:6.1}s  next\n");
                        spirc.next()?;
                    }
                    Some("") => {}
                    Some(other) => println!("unknown key {other:?}"),
                }
                continue;
            }
            _ = clusters.changed() => {}
            changed = states.changed() => {
                if changed.is_err() {
                    break;
                }
            }
            () = tokio::time::sleep(Duration::from_millis(500)) => continue,
        }
        let Some(state) = states.borrow_and_update().clone() else {
            continue;
        };
        let (active, here) = {
            let cluster = clusters.borrow_and_update();
            let id = cluster.active_device_id.clone().unwrap_or_default();
            let name = cluster
                .devices
                .get(&id)
                .map_or_else(|| id.clone(), |d| d.device_alias.clone());
            (name, id == session.device_id())
        };
        name_tracks(&session, &state, &mut names).await;
        let text = describe(&state, &active, here, &names);
        if text != shown {
            let t = began.elapsed().as_secs_f32();
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
    let mut text = format!(
        "{device} {status} at {} ms, from {}",
        position_now(state),
        state.context_uri
    );
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

/// Where the song is now, from where it was when the state was put.
fn position_now(state: &PlayerState) -> u32 {
    let mut position = state.position_as_of_timestamp;
    if state.is_playing && !state.is_paused {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64);
        position += (now - state.timestamp).max(0);
    }
    position.max(0) as u32
}
