//! Check: does switching volume levelling keep the song going? The engine
//! makes a new player for it, which should pick the song up where it was,
//! playing or paused.
//!
//! Plays at volume 0, so nothing is heard. Plays a song for a few seconds,
//! turns levelling on, then pauses and turns it off again, printing the
//! state reported after each switch.
//!
//!   cargo run --example levelling_probe -- [album link]
//!
//! Sign in through the app first: this reuses its saved credential.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use librespot_core::SpotifyUri;
use spotless::paths::Paths;
use spotless::playback::engine::{Engine, EngineEvent, PlayerSnapshot, Status};
use spotless::spotify::{catalog, session};
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
    let tracks = catalog::link_tracks(&session, &album).await?;
    let Some(track) = tracks.get(1) else {
        bail!("the album needs two songs");
    };
    println!("song: {}", track.title);

    let (sender, mut events) = mpsc::unbounded_channel();
    let mut engine = Engine::new(
        &session,
        0,
        false,
        Arc::new(move |event| {
            let _ = sender.send(event);
        }),
    )?;
    let uri = SpotifyUri::from_uri(&track.uri).context("bad URI")?;
    engine.load(uri, 30_000);

    let state = settle(&mut events, Status::Playing).await?;
    tokio::time::sleep(Duration::from_secs(3)).await;
    let state = latest(&mut events).unwrap_or(state);
    let before = state.position_now();
    println!("\nplaying at {} ms, levelling on", before);
    engine.set_levelling(true);
    let after = settle(&mut events, Status::Playing).await?;
    report(before, &after, Status::Playing);

    tokio::time::sleep(Duration::from_secs(2)).await;
    engine.pause();
    let paused = settle(&mut events, Status::Paused).await?;
    let paused = latest(&mut events).unwrap_or(paused);
    let before = paused.position_now();
    println!("\npaused at {} ms, levelling off", before);
    engine.set_levelling(false);
    let after = settle(&mut events, Status::Paused).await?;
    report(before, &after, Status::Paused);
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

fn report(before: u32, after: &PlayerSnapshot, status: Status) {
    let gap = i64::from(after.position_ms) - i64::from(before);
    let ok = after.status == status && gap.abs() < 1_500;
    println!(
        "  {}: {:?}, position moved {gap} ms",
        if ok { "OK" } else { "WRONG" },
        after.status
    );
}
