//! Check: does a song play on, without a gap, when the session drops and a
//! new one replaces it (as when the phone moves between Wi-Fi and mobile
//! data)? The engine should keep the playing song on its player and make a
//! new player, on the new session, only for the next song.
//!
//! Plays at volume 0, so nothing is heard. Plays a song for a while, drops
//! the session and hands the engine a new one, then seeks near the end to
//! see the next song start on the new session.
//!
//!   cargo run -p spotless-core --example reconnect_probe -- [album link]
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
    let Some(old) = session::restore(&paths).await? else {
        bail!("not signed in: start Spotless and sign in first");
    };
    let tracks = catalog::link_tracks(&old, &album).await?;
    let (Some(first), Some(second)) = (tracks.get(1), tracks.get(2)) else {
        bail!("the album needs three songs");
    };
    println!("songs: {} / {}", first.title, second.title);

    let (sender, mut events) = mpsc::unbounded_channel();
    let mut engine = Engine::new(
        &old,
        0,
        false,
        Arc::new(move |event| {
            let _ = sender.send(event);
        }),
    )?;
    engine.load(uri(&first.uri)?, 30_000);
    watch(&mut events, Duration::from_secs(15)).await;

    println!("\nsession dropped, new one in");
    old.shutdown();
    let Some(new) = session::restore(&paths).await? else {
        bail!("signed out meanwhile");
    };
    engine.set_session(&new);
    engine.follow(Some((EntryId(2), uri(&second.uri)?)));
    let restarted = watch(&mut events, Duration::from_secs(6)).await;
    println!(
        "  {}",
        if restarted {
            "WRONG: the song was loaded again"
        } else {
            "OK: played on"
        }
    );

    println!("\nseek near the end, for the next song");
    engine.seek(first.duration_ms.saturating_sub(5_000));
    watch(&mut events, Duration::from_secs(12)).await;
    engine.stop();
    Ok(())
}

fn uri(text: &str) -> Result<SpotifyUri> {
    SpotifyUri::from_uri(text).context("bad URI")
}

/// Prints what the engine reports for `long`. Returns whether a song loaded.
async fn watch(events: &mut mpsc::UnboundedReceiver<EngineEvent>, long: Duration) -> bool {
    let began = Instant::now();
    let mut loaded = false;
    while let Ok(Some(event)) =
        tokio::time::timeout(long.saturating_sub(began.elapsed()), events.recv()).await
    {
        let at = began.elapsed().as_millis();
        match event {
            EngineEvent::Snapshot(snapshot) => {
                loaded |= snapshot.status == Status::Loading;
                println!(
                    "  {at:>5} ms  {:?} at {} ms  {}",
                    snapshot.status,
                    snapshot.position_ms,
                    snapshot.track.map(|t| t.title).unwrap_or_default()
                );
            }
            other => println!("  {at:>5} ms  {other:?}"),
        }
    }
    loaded
}
