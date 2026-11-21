//! Check: does Spotless's own player play Spotify's songs? It opens an
//! album's first song through the Spotify source, plays a few seconds,
//! seeks near its end, preloads the second song when told to, and follows
//! into it, timing each step.
//!
//!   cargo run -p spotless-core --features probes --example player_probe -- [album link] [volume 0-100]
//!
//! Silent by default (volume 0). Sign in through the app first: this
//! reuses its saved credential.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use spotless_core::paths::Paths;
use spotless_core::playback::AudioLink;
use spotless_core::playback::player::{Event, Player};
use spotless_core::playback::sink::RodioSink;
use spotless_core::settings::Settings;
use spotless_core::spotify::audio::SpotifyAudio;
use spotless_core::spotify::session::AudioCache;
use spotless_core::spotify::{catalog, session};
use tokio::sync::mpsc;

/// Boards of Canada, Music Has the Right to Children.
const DEFAULT_ALBUM: &str = "spotify:album:1vWnB0hYmluskQuzxwo25a";

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let mut args = std::env::args().skip(1);
    let album = args.next().unwrap_or_else(|| DEFAULT_ALBUM.into());
    let percent: u32 = args.next().map_or(Ok(0), |v| v.parse())?;
    let volume = (u32::from(u16::MAX) * percent.min(100) / 100) as u16;

    let paths = Paths::new()?;
    let cache = AudioCache::from(&Settings::load(&paths));
    let Some(session) = session::restore(&paths, cache).await? else {
        bail!("not signed in: start Spotless and sign in first");
    };
    let tracks = catalog::songs(&session, &album).await?.tracks;
    let [first, second, ..] = tracks.as_slice() else {
        bail!("the album needs two songs");
    };

    let (told, mut events) = mpsc::unbounded_channel();
    let link = Arc::<AudioLink>::default();
    let player = Player::new(
        Arc::new(SpotifyAudio::new(&session)),
        Box::new(move || {
            Box::new(RodioSink::new(
                None,
                Box::new(|message| println!("output: {message}")),
                link,
            ))
        }),
        volume,
        true,
        Duration::from_secs(120),
        Box::new(move |event| {
            let _ = told.send(event);
        }),
    );
    let began = Instant::now();
    let at = || format!("{:6.2}s", began.elapsed().as_secs_f32());

    println!(
        "{} loading {} ({} s)",
        at(),
        first.title,
        first.duration_ms / 1000
    );
    let request = player.load(&first.uri, true, 0);
    wait(&mut events, |e| matches!(e, Event::Playing { .. }), &at).await?;
    tokio::time::sleep(Duration::from_secs(4)).await;

    let near_end = first.duration_ms.saturating_sub(8_000);
    println!("{} seeking to {} s", at(), near_end / 1000);
    player.seek(near_end);
    wait(&mut events, |e| matches!(e, Event::PreloadDue { .. }), &at).await?;
    println!("{} preloading {}", at(), second.title);
    player.preload(&second.uri);
    wait(
        &mut events,
        |e| matches!(e, Event::Ended { request: r, .. } if *r == request),
        &at,
    )
    .await?;
    let followed = Instant::now();
    let next = player.load(&second.uri, true, 0);
    wait(
        &mut events,
        |e| matches!(e, Event::Playing { request: r, .. } if *r == next),
        &at,
    )
    .await?;
    println!(
        "{} following took {} ms (preloaded: should be a few)",
        at(),
        followed.elapsed().as_millis()
    );
    tokio::time::sleep(Duration::from_secs(3)).await;
    player.stop();
    wait(&mut events, |e| matches!(e, Event::Stopped { .. }), &at).await?;
    println!("{} done", at());
    tokio::task::spawn_blocking(move || drop(player))
        .await
        .context("dropping the player")?;
    Ok(())
}

/// Prints the events up to the first `until` takes.
async fn wait(
    events: &mut mpsc::UnboundedReceiver<Event>,
    until: impl Fn(&Event) -> bool,
    at: &impl Fn() -> String,
) -> Result<()> {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(30), events.recv())
            .await
            .context("no event for 30 s")?
            .context("the player stopped telling")?;
        if !matches!(event, Event::Position { .. }) {
            println!("{} {event:?}", at());
        }
        if until(&event) {
            return Ok(());
        }
    }
}
