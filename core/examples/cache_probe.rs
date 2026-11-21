//! Check: does a song kept in the cache open without its details from
//! Spotify, and with no connection once its key was given?
//!
//! Opens a song with a cache of its own (in the system's temporary folder),
//! reads it until it's whole and kept, then opens it again on a cache read
//! afresh, as after a restart; then once more with the session shut down,
//! as with the network gone.
//!
//!   RUST_LOG=librespot=debug,cache_probe=info cargo run -p spotless-core --features probes --example cache_probe -- [song link]
//!
//! By default An Eagle In Your Mind (Boards of Canada), which isn't
//! playable here and plays as its alternative. With the log above, a
//! request for details shows as `extended-metadata`. Sign in through the
//! app first: this reuses its saved credential.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};
use spotless_core::model::SongUri;
use spotless_core::paths::Paths;
use spotless_core::playback::cache::SongCache;
use spotless_core::playback::source::Source;
use spotless_core::spotify::audio::SpotifyAudio;
use spotless_core::spotify::session;

const DEFAULT_SONG: &str = "spotify:track:03zv0Znm7bA7XzWMZ1lEKO";

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let song = std::env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_SONG.into());
    let paths = Paths::new()?;
    let Some(session) = session::restore(&paths).await? else {
        bail!("not signed in: start Spotless and sign in first");
    };
    let dir = std::env::temp_dir().join(format!("spotless-cache-probe-{}", std::process::id()));
    let cache = || Arc::new(SongCache::new(dir.clone(), 1024 * 1024 * 1024));

    let first = SpotifyAudio::new(Some(&session), cache());
    let uri = SongUri::from(song.clone());
    let check = dir.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        log::info!("1. not kept: opening");
        let began = Instant::now();
        let mut opened = first
            .open(&uri)
            .map_err(|e| anyhow!("didn't open: {e:?}"))?;
        println!(
            "1. {} opened in {} ms",
            opened.details.title,
            began.elapsed().as_millis()
        );
        opened.download.read_ahead();
        while !opened.download.whole() {
            if opened.decoder.next_piece()?.is_none() {
                break;
            }
        }
        drop(opened);
        // Kept once whole, on the download's side.
        let waited = Instant::now();
        while !has_record(&check) && waited.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(50));
        }
        println!("1. kept with its record: {}", has_record(&check));
        Ok(())
    })
    .await??;

    for (step, shut) in [(2, false), (3, true)] {
        if shut {
            session.shutdown();
        }
        let source = SpotifyAudio::new(Some(&session), cache());
        let uri = SongUri::from(song.clone());
        tokio::task::spawn_blocking(move || -> Result<()> {
            log::info!(
                "{step}. kept{}: opening",
                if shut { ", session shut down" } else { "" }
            );
            let began = Instant::now();
            match source.open(&uri) {
                Ok(mut opened) => {
                    let played = opened.decoder.next_piece()?.is_some();
                    println!(
                        "{step}. {} opened in {} ms, decodes: {played}",
                        opened.details.title,
                        began.elapsed().as_millis()
                    );
                }
                Err(e) => println!("{step}. didn't open: {e:?}"),
            }
            Ok(())
        })
        .await??;
    }
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

fn has_record(dir: &std::path::Path) -> bool {
    std::fs::read_dir(dir.join("spotify"))
        .map(|entries| {
            entries
                .flatten()
                .any(|e| e.path().extension().is_some_and(|x| x == "json"))
        })
        .unwrap_or(false)
}
