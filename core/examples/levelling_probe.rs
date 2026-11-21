//! Check: does switching volume levelling leave the song be? The player
//! levels from the next piece of audio on: a playing song should go on
//! playing, a paused one stay paused, with nothing loaded again.
//!
//! Plays at volume 0, so nothing is heard. Plays a song for a few seconds,
//! turns levelling on, then pauses and turns it off again, printing the
//! states reported in the two seconds after each switch.
//!
//!   cargo run -p spotless-core --features probes --example levelling_probe -- [album link]
//!
//! Sign in through the app first: this reuses its saved credential.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use librespot_core::SpotifyUri;
use spotless_core::paths::Paths;
use spotless_core::playback::cache::SongCache;
use spotless_core::playback::engine::{Engine, EngineEvent, PlayerSnapshot, Status};
use spotless_core::spotify::{catalog, session};
use tokio::sync::mpsc;

/// Boards of Canada, Music Has the Right to Children.
const DEFAULT_ALBUM: &str = "spotify:album:1vWnB0hYmluskQuzxwo25a";

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let album = std::env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_ALBUM.into());
    let paths = Paths::new()?;
    let Some(session) = session::restore(&paths).await? else {
        bail!("not signed in: start Spotless and sign in first");
    };
    let tracks = catalog::songs(&session, &album).await?.tracks;
    let Some(track) = tracks.get(1) else {
        bail!("the album needs two songs");
    };
    println!("song: {}", track.title);

    let (sender, mut events) = mpsc::unbounded_channel();
    let mut engine = Engine::new(
        &session,
        Arc::new(SongCache::off()),
        0,
        false,
        Arc::default(),
        Arc::new(move |event| {
            let _ = sender.send(event);
        }),
    )?;
    let uri = SpotifyUri::from_uri(&track.uri).context("bad URI")?;
    engine.load(uri, 30_000);

    let state = settle(&mut events, Status::Playing).await?;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let state = latest(&mut events).unwrap_or(state);
    println!("\nplaying at {} ms, levelling on", state.position_now());
    engine.set_levelling(true);
    steady(&mut events, Status::Playing).await;

    tokio::time::sleep(Duration::from_secs(2)).await;
    engine.pause();
    let paused = settle(&mut events, Status::Paused).await?;
    let paused = latest(&mut events).unwrap_or(paused);
    println!("\npaused at {} ms, levelling off", paused.position_now());
    engine.set_levelling(false);
    steady(&mut events, Status::Paused).await;
    engine.stop();
    Ok(())
}

/// The last state reported so far, dropping the older reports.
fn latest(events: &mut mpsc::UnboundedReceiver<EngineEvent>) -> Option<PlayerSnapshot> {
    let mut last = None;
    while let Ok(event) = events.try_recv() {
        if let EngineEvent::Snapshot(snapshot) = event {
            last = Some(snapshot);
        }
    }
    last
}

/// Waits for the player to report `status`, printing what it reports.
async fn settle(
    events: &mut mpsc::UnboundedReceiver<EngineEvent>,
    status: Status,
) -> Result<PlayerSnapshot> {
    let began = Instant::now();
    loop {
        let wait = Duration::from_secs(20).saturating_sub(began.elapsed());
        let Ok(Some(event)) = tokio::time::timeout(wait, events.recv()).await else {
            bail!("no {status:?} within 20 s");
        };
        match event {
            EngineEvent::Snapshot(snapshot) => {
                println!("  {:?} at {} ms", snapshot.status, snapshot.position_ms);
                if snapshot.status == status {
                    return Ok(snapshot);
                }
            }
            other => println!("  {other:?}"),
        }
    }
}

/// Prints the states reported over the next two seconds, and whether they
/// all kept to `status`.
async fn steady(events: &mut mpsc::UnboundedReceiver<EngineEvent>, status: Status) {
    let mut kept = true;
    let until = tokio::time::Instant::now() + Duration::from_secs(2);
    while let Ok(Some(event)) = tokio::time::timeout_at(until, events.recv()).await {
        match event {
            EngineEvent::Snapshot(snapshot) => {
                println!("  {:?} at {} ms", snapshot.status, snapshot.position_ms);
                kept &= snapshot.status == status;
            }
            other => {
                println!("  {other:?}");
                kept = false;
            }
        }
    }
    println!("  {}: stayed {status:?}", if kept { "OK" } else { "WRONG" });
}
