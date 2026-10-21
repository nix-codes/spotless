//! M1 check: does the engine start the followed song by itself when the
//! playing one ends, and stop when nothing follows?
//!
//! Plays at volume 0, so nothing is heard. Loads the first song of an album
//! 8 s before its end, hands over the second, and waits for the switch,
//! then lets the second end with nothing to follow.
//!
//!   cargo run -p spotless-core --example follow_probe -- [album link]
//!
//! Sign in through the app first: this reuses its saved credential.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use librespot_core::SpotifyUri;
use spotless_core::model::EntryId;
use spotless_core::paths::Paths;
use spotless_core::playback::engine::{Engine, EngineEvent, Status};
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
    let tracks = catalog::link_tracks(&session, &album).await?;
    let [first, second, ..] = tracks.as_slice() else {
        bail!("the album needs two songs");
    };
    println!("1: {} ({} ms)", first.title, first.duration_ms);
    println!("2: {} ({} ms)", second.title, second.duration_ms);

    let (sender, mut events) = mpsc::unbounded_channel();
    let engine = Engine::new(
        &session,
        0,
        false,
        Arc::new(move |event| {
            let _ = sender.send((Instant::now(), event));
        }),
    )?;
    let uri = |text: &str| SpotifyUri::from_uri(text).context("bad URI");

    let start = first.duration_ms.saturating_sub(8_000);
    engine.load(uri(&first.uri)?, start);
    engine.follow(Some((EntryId(2), uri(&second.uri)?)));
    println!("\nplaying 1 from {} s, 2 handed over", start / 1000);

    let began = Instant::now();
    let mut followed = false;
    let mut seeked_second = false;
    while let Ok(Some((at, event))) =
        tokio::time::timeout(Duration::from_secs(40), events.recv()).await
    {
        let t = at.duration_since(began).as_secs_f32();
        match event {
            EngineEvent::Followed(entry) => {
                println!("{t:6.1}s  followed {entry:?}");
                followed = true;
            }
            EngineEvent::Ended => {
                println!("{t:6.1}s  ended, nothing followed");
                break;
            }
            EngineEvent::Unavailable => println!("{t:6.1}s  unavailable"),
            EngineEvent::OutputError(message) => println!("{t:6.1}s  output error: {message}"),
            EngineEvent::Snapshot(snapshot) => {
                let title = snapshot.track.as_ref().map_or("-", |t| t.title.as_str());
                println!(
                    "{t:6.1}s  {:?} {title} at {} ms",
                    snapshot.status, snapshot.position_ms
                );
                // Skip most of the second song too.
                if followed && !seeked_second && snapshot.status == Status::Playing {
                    seeked_second = true;
                    engine.seek(second.duration_ms.saturating_sub(5_000));
                }
            }
        }
    }
    if !followed {
        bail!("the second song never started by itself");
    }
    println!("\nOK");
    Ok(())
}
