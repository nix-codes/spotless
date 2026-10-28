//! Catalogue reads over the librespot session (no Web API quota).

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use futures_util::future::join_all;
use futures_util::stream::{self, StreamExt};
use librespot_core::error::ErrorKind;
use librespot_core::session::Session;
use librespot_core::{Error, SpotifyUri};
use librespot_metadata::image::{ImageSize, Images};
use librespot_metadata::{Album, Artist, Metadata, Playlist, Track};
use librespot_protocol::extended_metadata::{
    BatchedEntityRequest, BatchedExtensionResponse, EntityRequest, ExtensionQuery,
};
use librespot_protocol::extension_kind::ExtensionKind;
use librespot_protocol::playlist_permission::{Permission, PermissionLevel};
use librespot_protocol::playlist4_external::SelectedListContent;
use protobuf::{EnumOrUnknown, Message};
use reqwest::Method;

use crate::model::catalog::{
    Discography, Place, Release, ReleaseGroup, Songs, SpotifyPlaylist, tidy,
};
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

/// Liked Songs, read like a playlist (Spotify's own URI for it).
pub const LIKED_SONGS: &str = "spotify:collection:tracks";

/// The songs a link stands for, in order: a track, an album's or a
/// playlist's tracks, or the tracks of an artist's first album.
pub async fn link_tracks(session: &Session, text: &str) -> Result<Vec<TrackRef>> {
    let uris = match parse_uri(text)? {
        uri @ SpotifyUri::Track { .. } => vec![uri],
        uri @ SpotifyUri::Album { .. } => album_tracks(session, &uri).await?,
        uri @ SpotifyUri::Artist { .. } => {
            let album = first_release(session, &uri).await?;
            album_tracks(session, &album).await?
        }
        uri @ SpotifyUri::Playlist { .. } => {
            let (tracks, _) = playlist_tracks(session, &uri).await?;
            tracks.into_iter().map(|(uri, _)| uri).collect()
        }
        _ => bail!("only tracks, albums, playlists and artists can be added"),
    };
    tracks(session, uris).await
}

async fn album_tracks(session: &Session, album: &SpotifyUri) -> Result<Vec<SpotifyUri>> {
    let album = Album::get(session, album)
        .await
        .context("album not found")?;
    Ok(album.tracks().cloned().collect())
}

/// A playlist's tracks with their places, leaving out its podcast
/// episodes and local files, and the revision read.
async fn playlist_tracks(
    session: &Session,
    playlist: &SpotifyUri,
) -> Result<(Vec<(SpotifyUri, Place)>, Vec<u8>)> {
    let playlist = Playlist::get(session, playlist)
        .await
        .context("playlist not found")?;
    let tracks = playlist
        .tracks()
        .enumerate()
        .filter(|(_, uri)| matches!(uri, SpotifyUri::Track { .. }))
        .map(|(at, uri)| (uri.clone(), Place::At(at)))
        .collect();
    Ok((tracks, playlist.revision))
}

/// Liked Songs' tracks, the latest liked first, as Spotify lists them.
async fn liked_tracks(session: &Session) -> Result<Vec<(SpotifyUri, Place)>> {
    let mut items = Vec::new();
    let mut token = String::new();
    loop {
        let page = session
            .spclient()
            .get_collection_page("collection", &token, LIKED_PAGE)
            .await
            .context("Liked Songs can't be read")?;
        items.extend(
            page.items
                .into_iter()
                .filter(|item| !item.is_removed && item.uri.starts_with("spotify:track:")),
        );
        if page.next_page_token.is_empty() {
            break;
        }
        token = page.next_page_token;
    }
    items.sort_by_key(|item| std::cmp::Reverse(item.added_at));
    Ok(items
        .iter()
        .filter_map(|item| {
            let uri = SpotifyUri::from_uri(&item.uri).ok()?;
            Some((uri, Place::Liked(item.added_at)))
        })
        .collect())
}

/// Liked songs asked for in one page.
const LIKED_PAGE: i32 = 500;
/// Playlists asked for in one read of the account's list.
const ROOTLIST_PAGE: usize = 200;

/// The account's playlists, in its own order, after Liked Songs. Folders
/// are left out and their playlists listed in their place.
pub async fn playlists(session: &Session) -> Result<Vec<SpotifyPlaylist>> {
    let liked = liked_tracks(session).await?;
    let mut found = vec![SpotifyPlaylist {
        uri: LIKED_SONGS.into(),
        name: "Liked Songs".into(),
        tracks: liked.len(),
        owner: None,
        private: true,
    }];
    let mut owners = Vec::new();
    let mut from = 0;
    loop {
        let bytes = session
            .spclient()
            .get_rootlist(from, Some(ROOTLIST_PAGE))
            .await
            .context("your playlists can't be read")?;
        let list = SelectedListContent::parse_from_bytes(&bytes)?;
        let contents = &list.contents;
        for (item, meta) in contents.items.iter().zip(contents.meta_items.iter()) {
            if !item.uri().starts_with("spotify:playlist:") {
                continue;
            }
            found.push(SpotifyPlaylist {
                uri: item.uri().into(),
                name: meta.attributes.name().into(),
                tracks: meta.length().max(0) as usize,
                owner: None,
                private: false,
            });
            owners.push(meta.owner_username().to_string());
        }
        from += contents.items.len();
        if contents.items.is_empty() || from >= list.length().max(0) as usize {
            break;
        }
    }
    let me = session.username();
    let others: HashSet<&String> = owners.iter().filter(|owner| **owner != me).collect();
    let names = join_all(
        others
            .into_iter()
            .map(|owner| async move { (owner, display_name(session, owner).await) }),
    );
    // The account's own, a few at a time: the list doesn't say.
    let mine: Vec<usize> = (0..owners.len()).filter(|&n| owners[n] == me).collect();
    let private = stream::iter(mine)
        .map(|n| {
            let uri = &found[n + 1].uri;
            async move { (n, is_private(session, uri).await) }
        })
        .buffer_unordered(PERMISSION_READS)
        .collect::<Vec<_>>();
    let (names, private) = futures_util::join!(names, private);
    let names: HashMap<&String, String> = names.into_iter().collect();
    let private: HashSet<usize> = private
        .into_iter()
        .filter_map(|(n, private)| private.then_some(n))
        .collect();
    for (n, (playlist, owner)) in found[1..].iter_mut().zip(&owners).enumerate() {
        playlist.owner = names.get(owner).cloned();
        playlist.private = private.contains(&n);
    }
    Ok(found)
}

/// Playlists whose privacy is read at once.
const PERMISSION_READS: usize = 8;

/// Whether only those a playlist is shared with can see it. Spotify keeps
/// it apart from the playlist, as who may see it by default; the `public`
/// flag in the account's list only says whether it's on the profile. A
/// playlist whose privacy can't be read counts as public.
async fn is_private(session: &Session, uri: &str) -> bool {
    let id = uri.trim_start_matches("spotify:playlist:");
    let endpoint = format!("/playlist-permission/v1/playlist/{id}/permission/base");
    let read = session
        .spclient()
        .request(&Method::GET, &endpoint, None, None)
        .await;
    read.ok()
        .and_then(|bytes| Permission::parse_from_bytes(&bytes).ok())
        .is_some_and(|base| base.permission_level() == PermissionLevel::BLOCKED)
}

/// A Spotify user's name as Spotify shows it, else their username.
async fn display_name(session: &Session, user: &str) -> String {
    let profile = session
        .spclient()
        .get_user_profile(user, Some(0), Some(0))
        .await
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    profile
        .as_ref()
        .and_then(|p| p.get("name")?.as_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(user)
        .to_string()
}

/// Reads the tracks' metadata, keeping their order and repeats (a playlist
/// may have a song twice). Tracks Spotify doesn't know are left out.
pub async fn tracks(session: &Session, uris: Vec<SpotifyUri>) -> Result<Vec<TrackRef>> {
    let tagged = uris.into_iter().map(|uri| (uri, ())).collect();
    let found = tracks_with(session, tagged).await?;
    Ok(found.into_iter().map(|(track, ())| track).collect())
}

/// Like [`tracks`], each track keeping what it came with.
async fn tracks_with<T>(
    session: &Session,
    uris: Vec<(SpotifyUri, T)>,
) -> Result<Vec<(TrackRef, T)>> {
    let mut seen = HashSet::new();
    let unique: Vec<SpotifyUri> = uris
        .iter()
        .map(|(uri, _)| uri)
        .filter(|uri| seen.insert(uri.to_uri()))
        .cloned()
        .collect();
    let read = read_many::<Track>(session, &unique, ExtensionKind::TRACK_V4).await?;
    let found: Vec<(TrackRef, T)> = uris
        .into_iter()
        .filter_map(|(uri, tag)| {
            let track = read.get(&uri.to_uri());
            if track.is_none() {
                log::warn!("track {uri} unavailable");
            }
            Some((track_ref(track?), tag))
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

/// The songs of an album, a playlist or Liked Songs, by URI, in order;
/// a playlist's and Liked Songs' with their places.
pub async fn songs(session: &Session, uri: &str) -> Result<Songs> {
    let (uris, revision, placed) = if uri == LIKED_SONGS {
        (liked_tracks(session).await?, Vec::new(), true)
    } else {
        match parse_uri(uri)? {
            uri @ SpotifyUri::Playlist { .. } => {
                let (uris, revision) = playlist_tracks(session, &uri).await?;
                (uris, revision, true)
            }
            uri => {
                let uris = album_tracks(session, &uri).await?;
                let uris = uris.into_iter().map(|uri| (uri, Place::At(0))).collect();
                (uris, Vec::new(), false)
            }
        }
    };
    if uris.is_empty() {
        bail!("no songs in it");
    }
    let (tracks, places) = tracks_with(session, uris).await?.into_iter().unzip();
    Ok(Songs {
        tracks,
        places: if placed { places } else { Vec::new() },
        revision,
    })
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
