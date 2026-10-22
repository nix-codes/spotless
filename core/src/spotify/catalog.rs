//! Catalogue reads over the librespot session (no Web API quota).

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use librespot_core::error::ErrorKind;
use librespot_core::session::Session;
use librespot_core::{Error, SpotifyUri};
use librespot_metadata::image::{ImageSize, Images};
use librespot_metadata::{Album, Artist, Metadata, Track};
use librespot_protocol::extended_metadata::{
    BatchedEntityRequest, BatchedExtensionResponse, EntityRequest, ExtensionQuery,
};
use librespot_protocol::extension_kind::ExtensionKind;
use protobuf::{EnumOrUnknown, Message};

use crate::model::catalog::{Discography, Release, ReleaseGroup, tidy};
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

/// Reads the tracks' metadata, keeping their order. Tracks Spotify doesn't
/// know are left out.
pub async fn tracks(session: &Session, uris: Vec<SpotifyUri>) -> Result<Vec<TrackRef>> {
    let mut read = read_many::<Track>(session, &uris, ExtensionKind::TRACK_V4).await?;
    let found: Vec<TrackRef> = uris
        .iter()
        .filter_map(|uri| {
            let track = read.remove(&uri.to_uri());
            if track.is_none() {
                log::warn!("track {uri} unavailable");
            }
            track.as_ref().map(track_ref)
        })
        .collect();
    if found.is_empty() {
        bail!("no playable songs found");
    }
    Ok(found)
}

/// Entities asked for in one metadata request at most. Spotify answered
/// 663 albums in one in 250 ms; this leaves a margin.
const BATCH: usize = 200;
/// Tries for a request librespot's rate limit holds back, a second apart.
const TRIES: u32 = 3;

/// Reads the metadata of many albums or tracks, a batch per request, by
/// URI. What Spotify sends nothing for is missing.
async fn read_many<M: Metadata>(
    session: &Session,
    uris: &[SpotifyUri],
    kind: ExtensionKind,
) -> Result<HashMap<String, M>> {
    let mut found = HashMap::new();
    for batch in uris.chunks(BATCH) {
        let request = BatchedEntityRequest {
            entity_request: batch
                .iter()
                .map(|uri| EntityRequest {
                    entity_uri: uri.to_uri(),
                    query: vec![ExtensionQuery {
                        extension_kind: EnumOrUnknown::new(kind),
                        ..Default::default()
                    }],
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        let response = request_batch(session, request)
            .await
            .context("Spotify's catalogue can't be read")?;
        let by_uri: HashMap<String, &SpotifyUri> = batch.iter().map(|u| (u.to_uri(), u)).collect();
        for data in response
            .extended_metadata
            .iter()
            .flat_map(|array| &array.extension_data)
        {
            let (Some(uri), Some(any)) =
                (by_uri.get(&data.entity_uri), data.extension_data.as_ref())
            else {
                continue;
            };
            let parsed = M::Message::parse_from_bytes(&any.value)
                .map_err(Error::from)
                .and_then(|message| M::parse(&message, uri));
            match parsed {
                Ok(entity) => {
                    found.insert(data.entity_uri.clone(), entity);
                }
                Err(error) => log::warn!("{} unreadable: {error}", data.entity_uri),
            }
        }
    }
    Ok(found)
}

/// Sends a batch, waiting for librespot's rate limit when it holds it back.
async fn request_batch(
    session: &Session,
    request: BatchedEntityRequest,
) -> Result<BatchedExtensionResponse, Error> {
    let mut tries = 1;
    loop {
        match session
            .spclient()
            .get_extended_metadata(request.clone())
            .await
        {
            Err(error) if error.kind == ErrorKind::ResourceExhausted && tries < TRIES => {
                log::info!("catalogue read held back: {error}; trying again");
                tokio::time::sleep(Duration::from_secs(1)).await;
                tries += 1;
            }
            result => return result,
        }
    }
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
                image: None,
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

/// An artist's releases, oldest first, read a batch at a time.
///
/// Each group lists one entry per release; an entry may hold several
/// editions of the same release, of which the first is used.
pub async fn discography(session: &Session, artist: &SpotifyUri) -> Result<Discography> {
    let artist = Artist::get(session, artist)
        .await
        .context("artist not found")?;
    let groups = [
        (ReleaseGroup::Album, &artist.albums),
        (ReleaseGroup::Single, &artist.singles),
        (ReleaseGroup::Compilation, &artist.compilations),
        (ReleaseGroup::AppearsOn, &artist.appears_on_albums),
    ];
    // In Spotify's order, so tidy keeps it within a year.
    let entries: Vec<(ReleaseGroup, SpotifyUri)> = groups
        .into_iter()
        .flat_map(|(group, entries)| {
            entries
                .iter()
                .filter_map(move |entry| Some((group, entry.first()?.clone())))
        })
        .collect();
    let uris: Vec<SpotifyUri> = entries.iter().map(|(_, uri)| uri.clone()).collect();
    let mut albums = read_many::<Album>(session, &uris, ExtensionKind::ALBUM_V4).await?;
    let mut releases = Vec::new();
    for (group, uri) in entries {
        let Some(album) = albums.remove(&uri.to_uri()) else {
            log::warn!("release {uri} unavailable");
            continue;
        };
        let tracks = album.tracks().count();
        releases.push(Release {
            uri: uri.to_uri(),
            title: album.name.clone(),
            artists: album
                .artists
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            year: album.date.year(),
            group,
            kind: Release::kind_label(&format!("{:?}", album.album_type), tracks),
            tracks,
            cover: cover_url(&album),
        });
    }
    Ok(Discography {
        artist: ArtistRef {
            uri: artist.id.to_uri(),
            name: artist.name.clone(),
            image: image_url(&artist.portrait_group),
        },
        releases: tidy(releases),
    })
}

/// The album's cover at Spotify's default size (300 px), else any.
fn cover_url(album: &Album) -> Option<String> {
    image_url(&album.covers)
}

/// An image at Spotify's default size (about 300 px), else any.
fn image_url(covers: &Images) -> Option<String> {
    covers
        .iter()
        .find(|image| image.size == ImageSize::DEFAULT)
        .or_else(|| covers.first())
        .map(|image| format!("https://i.scdn.co/image/{}", image.id.to_base16()))
}

/// An album's songs, in order.
pub async fn album(session: &Session, album: &str) -> Result<Vec<TrackRef>> {
    let uri = parse_uri(album)?;
    tracks(session, album_tracks(session, &uri).await?).await
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
