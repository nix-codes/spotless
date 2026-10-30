//! A probe: does the artist record the session reads for a discography
//! still carry Spotify's popular songs (`top_tracks`), and for which
//! countries? Prints the account's country, the countries listed, and the
//! songs `for_country` gives for it, by name; then what Spotless's own
//! discography makes of them, as soon as the session is up (its country
//! maybe not known yet).
//!
//!   cargo run -p spotless-core --example top_tracks_probe -- [artist URI]
//!
//! Sign in through the app first: this reuses its saved credential.

use anyhow::{Result, bail};
use librespot_core::SpotifyUri;
use librespot_metadata::{Artist, Metadata, Track};
use spotless_core::paths::Paths;
use spotless_core::spotify::{catalog, session};

/// Boards of Canada.
const DEFAULT_ARTIST: &str = "spotify:artist:2VAvhf61GgLYmC6C8anyX1";

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let artist = std::env::args()
        .nth(1)
        .unwrap_or_else(|| DEFAULT_ARTIST.into());
    let Some(session) = session::restore(&Paths::new()?).await? else {
        bail!("not signed in: start Spotless and sign in first");
    };
    let at_once = session.country();
    let discography = catalog::discography(&session, &catalog::parse_uri(&artist)?).await?;
    println!(
        "Spotless's discography, country {at_once:?} then: {} popular songs",
        discography.popular.len()
    );
    for (n, track) in discography.popular.iter().enumerate() {
        println!("{:>3}. {} ({})", n + 1, track.title, track.album.name);
    }
    println!();
    // Spotify tells the session its country a moment after signing in.
    let mut country = session.country();
    for _ in 0..50 {
        if !country.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        country = session.country();
    }
    println!("Signed in, country {country:?}");

    let artist = Artist::get(&session, &SpotifyUri::from_uri(&artist)?).await?;
    println!("Artist: {}", artist.name);
    let countries: Vec<String> = artist
        .top_tracks
        .iter()
        .map(|top| match top.country.as_str() {
            "" => format!("(global) {}", top.tracks.len()),
            country => format!("{country} {}", top.tracks.len()),
        })
        .collect();
    println!(
        "top_tracks lists {} countries: {}",
        countries.len(),
        countries.join(", ")
    );

    let top = artist.top_tracks.for_country(&country);
    println!("\nfor_country({country:?}): {} songs", top.len());
    for (n, uri) in top.iter().enumerate() {
        match Track::get(&session, uri).await {
            Ok(track) => println!("{:>3}. {} ({})", n + 1, track.name, track.album.name),
            Err(error) => println!("{:>3}. {uri}: {error}", n + 1),
        }
    }
    Ok(())
}
