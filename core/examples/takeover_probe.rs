//! What does Spotify tell Spotless when another device starts playing on the
//! same account?
//!
//! Plays a song at volume 0 for a few minutes and prints every packet the
//! session ignores, every change of the engine's status and whether the
//! session is still valid. Start playing on another device (a phone) while it
//! runs, and see what arrives.
//!
//!   RUST_LOG=warn,librespot_core::session=trace \
//!     cargo run -p spotless-core --example takeover_probe -- [album link]
//!
//! Sign in through the app first: this reuses its saved credential.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use librespot_core::SpotifyUri;
use spotless_core::paths::Paths;
use spotless_core::playback::engine::{Engine, EngineEvent};
use spotless_core::spotify::{catalog, session};
use tokio::sync::mpsc;

/// Boards of Canada, Music Has the Right to Children.
const DEFAULT_ALBUM: &str = "spotify:album:1vWnB0hYmluskQuzxwo25a";

const LISTEN: Duration = Duration::from_secs(240);

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,librespot_core::session=trace"),
    )
    .format_timestamp_millis()
    .init();
    let album = std::env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_ALBUM.into());
    let paths = Paths::new()?;
    let Some(session) = session::restore(&paths).await? else {
        bail!("not signed in: start Spotless and sign in first");
    };
    let tracks = catalog::link_tracks(&session, &album).await?;
    let Some(song) = tracks.iter().max_by_key(|t| t.duration_ms) else {
        bail!("the album has no songs");
    };
    println!(
        "playing {} ({} s) at volume 0",
        song.title,
        song.duration_ms / 1000
    );

    let (sender, mut events) = mpsc::unbounded_channel();
    let engine = Engine::new(
        &session,
        0,
        false,
        Arc::new(move |event| {
            let _ = sender.send((Instant::now(), event));
        }),
    )?;
    engine.load(SpotifyUri::from_uri(&song.uri).context("bad URI")?, 0);
    println!(
        "now start playing on another device; listening for {} s\n",
        LISTEN.as_secs()
    );

    let began = Instant::now();
    let mut status = None;
    let mut valid = true;
    while began.elapsed() < LISTEN {
        let event = tokio::time::timeout(Duration::from_secs(1), events.recv()).await;
        let t = began.elapsed().as_secs_f32();
        if valid && session.is_invalid() {
            valid = false;
            println!("{t:6.1}s  session is no longer valid");
        }
        let Ok(Some((_, event))) = event else {
            continue;
        };
        match event {
            EngineEvent::Snapshot(snapshot) if status != Some(snapshot.status) => {
                status = Some(snapshot.status);
                println!(
                    "{t:6.1}s  {:?} at {} ms",
                    snapshot.status, snapshot.position_ms
                );
            }
            EngineEvent::Snapshot(_) => {}
            other => println!("{t:6.1}s  {other:?}"),
        }
    }
    Ok(())
}
