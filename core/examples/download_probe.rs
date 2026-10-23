//! How far ahead of playback does a song download?
//!
//! Opens a song's file the way the player does and reads it at the speed it
//! plays, saying each second whether the rest of the file is in. Once it is,
//! the song plays to its end without a connection.
//!
//!   cargo run -p spotless-core --example download_probe -- [album link]
//!
//! It reads the album's longest song.
//!
//! Sign in through the app first: this reuses its saved credential.

use std::io::Read;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use librespot_audio::AudioFile;
use librespot_core::SpotifyUri;
use librespot_metadata::audio::{AudioFileFormat, AudioItem};
use spotless_core::paths::Paths;
use spotless_core::spotify::{catalog, session};

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
    let song = tracks
        .iter()
        .max_by_key(|t| t.duration_ms)
        .context("the album has no songs")?;
    let uri = SpotifyUri::from_uri(&song.uri).context("bad URI")?;
    let mut item = AudioItem::get_file(&session, uri).await?;
    // As the player does: a song with no files of its own plays as one of
    // its alternatives.
    if item.files.is_empty() {
        for alternative in item.alternatives.clone().into_iter().flat_map(|a| a.0) {
            let other = AudioItem::get_file(&session, alternative).await?;
            if !other.files.is_empty() {
                item = other;
                break;
            }
        }
    }
    let (format, file_id) = [
        (AudioFileFormat::OGG_VORBIS_160, 20),
        (AudioFileFormat::MP3_160, 20),
        (AudioFileFormat::OGG_VORBIS_320, 40),
        (AudioFileFormat::MP3_320, 40),
    ]
    .into_iter()
    .find_map(|(format, kbytes)| Some(((format, kbytes), *item.files.get(&format)?)))
    .with_context(|| {
        format!(
            "none of the files is one read here: {:?}",
            item.files.keys()
        )
    })?;
    let (format, bytes_per_second) = (format.0, format.1 * 1024);
    println!("{} ({} s), {format:?}", item.name, item.duration_ms / 1000);

    let mut file = AudioFile::open(&session, file_id, bytes_per_second).await?;
    if file.is_cached() {
        bail!("the song is in the audio cache: run with XDG_CACHE_HOME set to an empty folder");
    }
    let controller = file.get_stream_loader_controller()?;
    controller.set_stream_mode();
    let size = controller.len();
    println!("{size} bytes, {} s of audio", size / bytes_per_second);

    // Read a tenth of a second's worth every tenth of a second, as playback does.
    let began = Instant::now();
    let mut buffer = vec![0; bytes_per_second / 10];
    let mut read = 0;
    tokio::task::block_in_place(|| -> Result<()> {
        for tick in 1..=600 {
            read += file.read(&mut buffer)?;
            std::thread::sleep(Duration::from_millis(100));
            if tick % 10 == 0 {
                let whole = controller.range_to_end_available();
                println!(
                    "{:5.1}s  read {:3} s of audio, rest downloaded: {whole}",
                    began.elapsed().as_secs_f32(),
                    read / bytes_per_second
                );
                if whole {
                    break;
                }
            }
        }
        Ok(())
    })
}
