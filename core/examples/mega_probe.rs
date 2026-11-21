//! Check: does a song of a public MEGA folder play through Spotless's own
//! player parts, with no account, and open again from the cache?
//!
//! Lists the folder (its files' names decrypted), opens the first audio
//! file (or the one named) as `mega:<folder>:<file>#<key>`, reads its
//! start, seeks to its middle, then reads it whole so it's kept, and
//! opens it again on a cache read afresh, as after a restart. The cache
//! is its own, in the system's temporary folder. Nothing is played aloud.
//!
//!   RUST_LOG=spotless_core=info cargo run -p spotless-core --features probes --example mega_probe -- <folder link> [file name]

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use spotless_core::mega::api::Api;
use spotless_core::mega::audio::MegaAudio;
use spotless_core::mega::link::{FileKey, FolderLink};
use spotless_core::model::SongUri;
use spotless_core::playback::cache::SongCache;
use spotless_core::playback::source::Source;

const AUDIO: [&str; 4] = ["ogg", "mp3", "flac", "wav"];

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let mut args = std::env::args().skip(1);
    let link = args.next().context("give a MEGA folder link")?;
    let wanted = args.next();
    let link = FolderLink::parse(&link).context("not a MEGA folder link")?;
    let folder_key = link.folder_key().context("the link's key doesn't read")?;

    let began = Instant::now();
    let nodes = Api::new().list(&link.handle).await?;
    println!(
        "listed {} files and folders in {} ms",
        nodes.len(),
        began.elapsed().as_millis()
    );
    let mut chosen = None;
    for node in nodes.iter().filter(|node| !node.folder) {
        let name = FileKey::open(&node.key, &folder_key)
            .and_then(|key| key.name(&node.attributes))
            .unwrap_or_else(|| "(name doesn't open)".into());
        println!("  {} {:>9} B  {name}", node.handle, node.size);
        let audio = name
            .rsplit_once('.')
            .is_some_and(|(_, ext)| AUDIO.contains(&ext.to_lowercase().as_str()));
        let fits = wanted.as_ref().map_or(audio, |wanted| &name == wanted);
        if fits && chosen.is_none() {
            chosen = Some(node.handle.clone());
        }
    }
    let file = chosen.context("no audio file to open")?;
    let uri = SongUri::mega(&link.handle, &file, &link.key);

    let dir = std::env::temp_dir().join(format!("spotless-mega-probe-{}", std::process::id()));
    let cache = || Arc::new(SongCache::new(dir.clone(), 1024 * 1024 * 1024));
    let first = MegaAudio::new(cache());
    let song = uri.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let began = Instant::now();
        let mut opened = first
            .open(&song)
            .map_err(|e| anyhow!("didn't open: {e:?}"))?;
        let d = &opened.details;
        println!(
            "1. opened in {} ms: {:?} by {:?}, album {:?}, {} ms long, gains {:?}",
            began.elapsed().as_millis(),
            d.title,
            d.artists,
            d.album,
            d.duration_ms,
            opened.gains
        );
        let piece = opened.decoder.next_piece()?.context("no audio")?;
        println!(
            "1. decodes: {} samples at {} ms",
            piece.samples.len(),
            piece.position_ms
        );
        let middle = opened.details.duration_ms / 2;
        let began = Instant::now();
        opened.download.read_ahead();
        let at = opened.decoder.seek(middle)?;
        let piece = opened
            .decoder
            .next_piece()?
            .context("no audio after the seek")?;
        println!(
            "1. seeked to {at} ms (asked {middle}) in {} ms, decodes at {} ms",
            began.elapsed().as_millis(),
            piece.position_ms
        );
        let began = Instant::now();
        opened.decoder.seek(0)?;
        opened.download.read_ahead();
        while !opened.download.whole() {
            if opened.decoder.next_piece()?.is_none() {
                break;
            }
        }
        println!("1. read whole in {} ms", began.elapsed().as_millis());
        Ok(())
    })
    .await??;
    // Kept once whole, on the download's side.
    let waited = Instant::now();
    while !kept(&dir) && waited.elapsed() < Duration::from_secs(10) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    println!("1. kept with its record: {}", kept(&dir));

    let again = MegaAudio::new(cache());
    tokio::task::spawn_blocking(move || -> Result<()> {
        let began = Instant::now();
        match again.open(&uri) {
            Ok(mut opened) => {
                let decodes = opened.decoder.next_piece()?.is_some();
                println!(
                    "2. from the cache: {:?} opened in {} ms, decodes: {decodes}",
                    opened.details.title,
                    began.elapsed().as_millis()
                );
            }
            Err(e) => println!("2. didn't open from the cache: {e:?}"),
        }
        Ok(())
    })
    .await??;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

fn kept(dir: &std::path::Path) -> bool {
    std::fs::read_dir(dir.join("mega")).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|e| e.path().extension().is_some_and(|x| x == "json"))
    })
}
