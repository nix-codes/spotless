//! Catalogue reads over the librespot session (no Web API quota).

use anyhow::{Context, Result, anyhow, bail};
use librespot_core::SpotifyUri;
use librespot_core::session::Session;
use librespot_metadata::{Album, Artist, Metadata};

/// Accepts `spotify:` URIs and `open.spotify.com` links.
pub fn parse_uri(text: &str) -> Result<SpotifyUri> {
    let text = text.trim();
    let uri = match text.split_once("open.spotify.com/") {
        Some((_, path)) => {
            let path = path.split(['?', '#']).next().unwrap_or_default();
            let mut parts = path.split('/').filter(|part| !part.is_empty());
            let mut kind = parts.next().unwrap_or_default();
            // Localised links: open.spotify.com/intl-de/track/…
            if kind.starts_with("intl-") {
                kind = parts.next().unwrap_or_default();
            }
            let id = parts.next().unwrap_or_default();
            format!("spotify:{kind}:{id}")
        }
        None => text.to_string(),
    };
    SpotifyUri::from_uri(&uri).map_err(|e| anyhow!("not a Spotify link: {e}"))
}

/// The track a link stands for: the track itself, an album's first track,
/// or the first track of an artist's first album.
pub async fn first_track(session: &Session, uri: SpotifyUri) -> Result<SpotifyUri> {
    match uri {
        SpotifyUri::Track { .. } => Ok(uri),
        SpotifyUri::Album { .. } => {
            let album = Album::get(session, &uri).await.context("album not found")?;
            album
                .tracks()
                .next()
                .cloned()
                .context("the album has no tracks")
        }
        SpotifyUri::Artist { .. } => {
            let artist = Artist::get(session, &uri)
                .await
                .context("artist not found")?;
            let album = artist
                .albums
                .iter()
                .chain(artist.singles.iter())
                .flat_map(|group| group.iter())
                .next()
                .cloned()
                .context("the artist has no releases")?;
            Box::pin(first_track(session, album)).await
        }
        _ => bail!("only tracks, albums and artists can be played"),
    }
}

/// Which tab of an artist's discography a release belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseGroup {
    Album,
    Single,
    Compilation,
    AppearsOn,
}

#[derive(Clone, Debug)]
pub struct Release {
    pub uri: SpotifyUri,
    pub title: String,
    pub year: i32,
    pub group: ReleaseGroup,
    /// Spotify's own type for the release (album, single, EP, compilation).
    pub kind: String,
    pub tracks: usize,
}

/// An artist's releases, grouped as the session reports them, oldest first.
///
/// Each group lists one entry per release; an entry may hold several
/// editions of the same release, of which the first is used.
pub async fn discography(session: &Session, artist: &SpotifyUri) -> Result<(String, Vec<Release>)> {
    let artist = Artist::get(session, artist)
        .await
        .context("artist not found")?;
    let groups = [
        (ReleaseGroup::Album, &artist.albums),
        (ReleaseGroup::Single, &artist.singles),
        (ReleaseGroup::Compilation, &artist.compilations),
        (ReleaseGroup::AppearsOn, &artist.appears_on_albums),
    ];
    let mut releases = Vec::new();
    for (group, entries) in groups {
        for entry in entries.iter() {
            let Some(uri) = entry.first() else { continue };
            match Album::get(session, uri).await {
                Ok(album) => releases.push(Release {
                    uri: uri.clone(),
                    title: album.name.clone(),
                    year: album.date.year(),
                    group,
                    kind: format!("{:?}", album.album_type),
                    tracks: album.tracks().count(),
                }),
                Err(error) => log::warn!("release {uri} unavailable: {error}"),
            }
        }
    }
    releases.sort_by_key(|release| release.year);
    Ok((artist.name.clone(), releases))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_and_uris_parse_alike() {
        let uri = "spotify:track:4uLU6hMCjMI75M1A2tKUQC";
        let expected = parse_uri(uri).unwrap();
        for link in [
            "https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC",
            "https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC?si=abc",
            "https://open.spotify.com/intl-de/track/4uLU6hMCjMI75M1A2tKUQC",
            "  spotify:track:4uLU6hMCjMI75M1A2tKUQC \n",
        ] {
            assert_eq!(parse_uri(link).unwrap(), expected, "{link}");
        }
    }

    #[test]
    fn other_text_is_refused() {
        assert!(parse_uri("boards of canada").is_err());
    }
}
