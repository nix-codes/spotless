//! A probe: does Spotify's private lookup turn a Spotify Code's media
//! reference into a URI for our session? Reads the code as the apps do
//! (`spotify::code`), then asks once.
//!
//!   cargo run -p spotless-core --features probes --example spotify_code_probe -- <what>
//!
//! where <what> is one of:
//!
//!   --heights 0,6,0,2,4,5,1,4,5,2,3,7,3,7,1,5,6,2,5,7,4,3,0
//!   path/to/code.png        a screenshot or saved Spotify Code image
//!   --uri spotify:track:…   fetches that URI's code from Spotify's public
//!                           image server (one request), then reads it
//!   --link https://open.spotify.com/…   no code: opens the link as Browse
//!                           does when it's searched for
//!
//! and `--no-lookup` stops after decoding, without signing in.
//!
//! Every request to Spotify is sent once: nothing is retried, whatever the
//! answer, so the probe never looks like a client hammering the service.
//! Sign in through the app first: this reuses its saved credential.

use anyhow::{Context, Result, bail};
use librespot_core::session::Session;
use spotless_core::paths::Paths;
use spotless_core::settings::Settings;
use spotless_core::spotify::CodeImage;
use spotless_core::spotify::session::{self, AudioCache};
use spotless_core::spotify::{catalog, code};

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let lookup = !args.iter().any(|a| a == "--no-lookup");
    let args: Vec<&str> = (args.iter().map(String::as_str))
        .filter(|a| *a != "--no-lookup")
        .collect();

    if let ["--link", link] = args.as_slice() {
        let session = signed_in().await?;
        return opens(&session, link).await;
    }
    let mut expected = None;
    let reference = match args.as_slice() {
        ["--heights", list] => {
            let heights = parse_heights(list)?;
            code::bars::decode(&heights).context("these bars aren't a Spotify Code")?
        }
        ["--uri", uri] => {
            expected = Some(uri.to_string());
            code::read(&CodeImage::File(fetch_code_image(uri).await?))?
        }
        [path] => code::read(&CodeImage::File(
            std::fs::read(path).with_context(|| format!("reading {path}"))?,
        ))?,
        _ => bail!("give --heights a,b,…, an image file, or --uri spotify:…"),
    };
    println!("Media reference: {reference}");
    println!("Bars: {:?}", code::encode(reference));
    if !lookup {
        return Ok(());
    }

    let session = signed_in().await?;
    let found = code::look_up(&session, &reqwest::Client::new(), reference).await?;
    println!("Spotify: {found}");
    opens(&session, &found).await?;
    if let Some(expected) = expected {
        println!("Asked for {expected}");
    }
    Ok(())
}

/// The session the app saved.
async fn signed_in() -> Result<Session> {
    let paths = Paths::new()?;
    match session::restore(&paths, AudioCache::from(&Settings::load(&paths))).await? {
        Some(session) => Ok(session),
        None => bail!("not signed in: start Spotless and sign in first"),
    }
}

/// What Browse opens for `found`, and a playlist's songs as its page reads
/// them.
async fn opens(session: &Session, found: &str) -> Result<()> {
    let target = code::target(session, found).await?;
    println!("Opens: {target:?}");
    // What Browse reads next: a playlist's songs, as its page shows them.
    if let code::CodeTarget::Playlist(playlist) = &target {
        let started = std::time::Instant::now();
        let songs = catalog::songs(session, &playlist.uri).await;
        let took = started.elapsed();
        match songs {
            Ok(songs) => println!("Songs: {} in {took:.1?}", songs.tracks.len()),
            Err(error) => println!("Songs: {error:#} after {took:.1?}"),
        }
    }
    Ok(())
}

fn parse_heights(list: &str) -> Result<[u8; 23]> {
    let heights: Vec<u8> = list
        .split(',')
        .map(|h| h.trim().parse::<u8>())
        .collect::<Result<_, _>>()
        .context("heights are numbers 0 to 7")?;
    match <[u8; 23]>::try_from(heights) {
        Ok(heights) if heights.iter().all(|h| *h <= 7) => Ok(heights),
        _ => bail!("a Spotify Code has 23 heights of 0 to 7"),
    }
}

/// The code Spotify's public image server draws for `uri`: one request.
async fn fetch_code_image(uri: &str) -> Result<Vec<u8>> {
    let url = format!("https://scannables.scdn.co/uri/plain/png/FFFFFF/black/640/{uri}");
    let response = reqwest::get(&url).await?;
    let status = response.status();
    if !status.is_success() {
        bail!("the image server answered {status} (not retried)");
    }
    Ok(response.bytes().await?.to_vec())
}
