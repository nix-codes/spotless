//! Reads the account's Spotify playlists as Browse lists them (Liked Songs
//! first, owners by name), then the songs of Liked Songs and of the
//! biggest playlist, with timings. Reads only; changes nothing.
//!
//!   cargo run -p spotless-core --example playlists_probe
//!
//! Sign in through the app first: this reuses its saved credential.

use std::time::Instant;

use anyhow::{Result, bail};
use librespot_core::cache::Cache;
use librespot_core::{Session, SessionConfig};
use spotless_core::paths::Paths;
use spotless_core::spotify::catalog;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let paths = Paths::new()?;
    let cache = Cache::new(Some(paths.config.as_path()), None, None, None)?;
    let Some(credentials) = cache.credentials() else {
        bail!("not signed in: start Spotless and sign in first");
    };
    let session = Session::new(SessionConfig::default(), Some(cache));
    session.connect(credentials, true).await?;

    let began = Instant::now();
    let playlists = catalog::playlists(&session).await?;
    println!("{} playlists in {:?}", playlists.len(), began.elapsed());
    for playlist in &playlists {
        let owner = playlist.owner.as_deref().unwrap_or("-");
        println!("{:>5}  {}  by {owner}", playlist.tracks, playlist.name);
    }
    let biggest = playlists[1..].iter().max_by_key(|p| p.tracks);
    for playlist in playlists.first().into_iter().chain(biggest) {
        let began = Instant::now();
        let songs = catalog::songs(&session, &playlist.uri).await?;
        println!(
            "{}: {} of {} songs read in {:?}, first: {:?}",
            playlist.name,
            songs.len(),
            playlist.tracks,
            began.elapsed(),
            songs.first().map(|s| &s.title)
        );
    }
    Ok(())
}
