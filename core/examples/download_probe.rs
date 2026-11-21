//! How far ahead of playback does a song download?
//!
//! Opens a song the way the player does, with no cache, and decodes it at
//! the speed it plays, saying each second whether the rest of the file is
//! in. Once it is, the song plays to its end without a connection.
//!
//!   cargo run -p spotless-core --features probes --example download_probe -- [album link] [seconds before the end]
//!
//! It reads the album's longest song, from its start or from that many
//! seconds before its end.
//!
//! Sign in through the app first: this reuses its saved credential.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use spotless_core::paths::Paths;
use spotless_core::playback::cache::SongCache;
use spotless_core::playback::source::Source;
use spotless_core::spotify::audio::SpotifyAudio;
use spotless_core::spotify::{catalog, session};

/// Boards of Canada, Music Has the Right to Children.
const DEFAULT_ALBUM: &str = "spotify:album:1vWnB0hYmluskQuzxwo25a";

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let mut args = std::env::args().skip(1);
    let album = args.next().unwrap_or_else(|| DEFAULT_ALBUM.into());
    let before_end: Option<u32> = args.next().map(|s| s.parse()).transpose()?;
    let paths = Paths::new()?;
    let Some(session) = session::restore(&paths).await? else {
        bail!("not signed in: start Spotless and sign in first");
    };
    let tracks = catalog::songs(&session, &album).await?.tracks;
    let song = tracks
        .iter()
        .max_by_key(|t| t.duration_ms)
        .context("the album has no songs")?;
    println!("{} ({} s)", song.title, song.duration_ms / 1000);

    let source = SpotifyAudio::new(Some(&session), Arc::new(SongCache::off()));
    let uri = song.uri.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let began = Instant::now();
        let mut opened = source
            .open(&uri)
            .map_err(|e| anyhow!("the song didn't open: {e:?}"))?;
        println!("opened in {} ms", began.elapsed().as_millis());
        let from = before_end.map_or(0, |s| opened.details.duration_ms.saturating_sub(s * 1000));
        if from > 0 {
            let at = opened.decoder.seek(from)?;
            opened.download.read_ahead();
            println!(
                "seeked to {} s in {} ms",
                at / 1000,
                began.elapsed().as_millis()
            );
        }
        // Decode at the speed it plays.
        let began = Instant::now()
            .checked_sub(Duration::from_millis(from.into()))
            .context("a clock")?;
        let start = Instant::now();
        let mut said = 0;
        while let Some(piece) = opened.decoder.next_piece()? {
            let at = Duration::from_millis(piece.position_ms.into());
            if let Some(early) = at.checked_sub(began.elapsed()) {
                std::thread::sleep(early);
            }
            let seconds = start.elapsed().as_secs();
            if seconds > said {
                said = seconds;
                let whole = opened.download.whole();
                println!(
                    "{:5.1}s  played {:3} s, rest downloaded: {whole}",
                    start.elapsed().as_secs_f32(),
                    piece.position_ms / 1000,
                );
                if whole {
                    break;
                }
            }
        }
        Ok(())
    })
    .await?
}
