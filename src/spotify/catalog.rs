//! Catalogue reads over the librespot session (no Web API quota).

use anyhow::{Context, Result, anyhow, bail};
use librespot_core::SpotifyUri;
use librespot_core::session::Session;
use librespot_metadata::{Album, Artist, Metadata, Track};

use crate::model::{AlbumRef, ArtistRef, TrackRef};

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

/// The songs a link stands for, in order: a track, an album's tracks, or
/// the tracks of an artist's first album (until the artist page, M2).
pub async fn link_tracks(session: &Session, text: &str) -> Result<Vec<TrackRef>> {
    let uris = match parse_uri(text)? {
        uri @ SpotifyUri::Track { .. } => vec![uri],
        uri @ SpotifyUri::Album { .. } => album_tracks(session, &uri).await?,
        uri @ SpotifyUri::Artist { .. } => {
            let album = first_release(session, &uri).await?;
            album_tracks(session, &album).await?
        }
        _ => bail!("only tracks, albums and artists can be added"),
    };
    tracks(session, uris).await
}

async fn album_tracks(session: &Session, album: &SpotifyUri) -> Result<Vec<SpotifyUri>> {
    let album = Album::get(session, album)
        .await
        .context("album not found")?;
    Ok(album.tracks().cloned().collect())
}

/// Reads the tracks' metadata concurrently, keeping their order. Tracks
/// Spotify doesn't know are left out.
pub async fn tracks(session: &Session, uris: Vec<SpotifyUri>) -> Result<Vec<TrackRef>> {
    let mut reads = tokio::task::JoinSet::new();
    for (i, uri) in uris.into_iter().enumerate() {
        let session = session.clone();
        reads.spawn(async move { (i, Track::get(&session, &uri).await.map_err(|e| (uri, e))) });
    }
    let mut found = Vec::new();
    while let Some(read) = reads.join_next().await {
        match read? {
            (i, Ok(track)) => found.push((i, track_ref(&track))),
            (_, Err((uri, error))) => log::warn!("track {uri} unavailable: {error}"),
        }
    }
    if found.is_empty() {
        bail!("no playable songs found");
    }
    found.sort_by_key(|(i, _)| *i);
    Ok(found.into_iter().map(|(_, track)| track).collect())
}

fn track_ref(track: &Track) -> TrackRef {
    TrackRef {
        uri: track.id.to_uri(),
        title: track.name.clone(),
        artists: track
            .artists
            .iter()
            .map(|artist| ArtistRef {
                uri: artist.id.to_uri(),
                name: artist.name.clone(),
            })
            .collect(),
        album: AlbumRef {
            uri: track.album.id.to_uri(),
            name: track.album.name.clone(),
        },
        duration_ms: track.duration.max(0) as u32,
    }
}

/// An artist's first album, or first single if it has no albums.
async fn first_release(session: &Session, uri: &SpotifyUri) -> Result<SpotifyUri> {
    let artist = Artist::get(session, uri)
        .await
        .context("artist not found")?;
    artist
        .albums
        .iter()
        .chain(artist.singles.iter())
        .flat_map(|group| group.iter())
        .next()
        .cloned()
        .context("the artist has no releases")
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
