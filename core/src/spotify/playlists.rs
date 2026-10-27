//! Writes to the account's Spotify playlists over the librespot session,
//! as Spotify's own clients do: a new playlist, songs added to the end of
//! one, songs removed and put back; and songs liked and un-liked.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use librespot_core::session::Session;
use librespot_core::{SpotifyId, SpotifyUri};
use librespot_protocol::collection2v2::CollectionItem;
use librespot_protocol::playlist4_external::{
    Add, ChangeInfo, CreateListReply, Delta, Item, ItemAttributes, ListAttributes,
    ListAttributesPartialState, ListChanges, ListUpdateRequest, Op, Rem, SelectedListContent,
    UpdateListAttributes, op,
};
use protobuf::{Message, MessageField};
use reqwest::Method;

/// Songs added in one change. Spotify took 432 at once; this leaves a
/// margin.
const BATCH: usize = 400;

/// Makes a playlist by this name, first in the account's list,
/// with these songs (track URIs). Answers its URI.
pub async fn create(session: &Session, name: &str, songs: &[String]) -> Result<String> {
    let spclient = session.spclient();
    // Spotify makes it empty whatever the request says: the name and the
    // songs follow as changes.
    let request = ListUpdateRequest {
        info: info(session),
        ..Default::default()
    };
    let reply = spclient
        .request_with_protobuf(&Method::POST, "/playlist/v2/playlist", None, &request)
        .await
        .context("Spotify didn't make the playlist")?;
    let created = CreateListReply::parse_from_bytes(&reply)?;
    let uri = created.uri().to_string();

    // In the account's list first, so that it shows whatever comes after.
    let bytes = spclient.get_rootlist(0, Some(0)).await?;
    let root = SelectedListContent::parse_from_bytes(&bytes)?;
    let item = Item {
        uri: Some(uri.clone()),
        attributes: MessageField::some(ItemAttributes {
            timestamp: Some(now_ms()),
            // Kept off the public profile, yet Spotify's apps still
            // show the playlist as public.
            public: Some(false),
            ..Default::default()
        }),
        ..Default::default()
    };
    let changes = changes(
        session,
        root.revision.clone(),
        vec![Op {
            kind: Some(op::Kind::ADD.into()),
            add: MessageField::some(Add {
                items: vec![item],
                add_first: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        }],
    );
    let endpoint = format!("/playlist/v2/user/{}/rootlist/changes", session.username());
    spclient
        .request_with_protobuf(&Method::POST, &endpoint, None, &changes)
        .await
        .context("Spotify made the playlist but didn't list it")?;

    let rename = Op {
        kind: Some(op::Kind::UPDATE_LIST_ATTRIBUTES.into()),
        update_list_attributes: MessageField::some(UpdateListAttributes {
            new_attributes: MessageField::some(ListAttributesPartialState {
                values: MessageField::some(ListAttributes {
                    name: Some(name.to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    write(session, &uri, created.revision, Some(rename), songs).await?;
    Ok(uri)
}

/// Adds songs (track URIs) to the end of a playlist.
pub async fn add(session: &Session, playlist: &str, songs: &[String]) -> Result<()> {
    let revision = revision(session, playlist).await?;
    write(session, playlist, revision, None, songs).await
}

/// Songs of a playlist, by their place in it and URI.
pub type Rows = [(usize, String)];

/// Removes these songs from a playlist, each by its place, so that only
/// that copy of a song there twice goes. Refused, with an `Aborted` error,
/// if the playlist changed since `revision`, rather than removing other
/// songs. Answers the new revision.
pub async fn remove_at(
    session: &Session,
    playlist: &str,
    revision: Vec<u8>,
    rows: &Rows,
) -> Result<Vec<u8>> {
    let mut rows = rows.to_vec();
    // From the last, so the places of those before stay the same.
    rows.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    let ops = rows
        .into_iter()
        .map(|(at, uri)| Op {
            kind: Some(op::Kind::REM.into()),
            rem: MessageField::some(Rem {
                from_index: Some(at as i32),
                length: Some(1),
                items: vec![item(uri)],
                ..Default::default()
            }),
            ..Default::default()
        })
        .collect();
    post(session, playlist, Some(revision), ops).await
}

/// Like [`remove_at`], on the playlist as it is now.
pub async fn remove(session: &Session, playlist: &str, rows: &Rows) -> Result<Vec<u8>> {
    let revision = revision(session, playlist).await?;
    remove_at(session, playlist, revision.unwrap_or_default(), rows).await
}

/// Puts removed songs back at the places they had.
pub async fn put_back(session: &Session, playlist: &str, rows: &Rows) -> Result<()> {
    let mut rows = rows.to_vec();
    // From the first, so each lands where it was.
    rows.sort_by_key(|(at, _)| *at);
    let ops = rows
        .into_iter()
        .map(|(at, uri)| Op {
            kind: Some(op::Kind::ADD.into()),
            add: MessageField::some(Add {
                from_index: Some(at as i32),
                items: vec![item(uri)],
                ..Default::default()
            }),
            ..Default::default()
        })
        .collect();
    let revision = revision(session, playlist).await?;
    post(session, playlist, revision, ops).await.map(|_| ())
}

/// Takes songs (track URIs) out of Liked Songs.
pub async fn unlike(session: &Session, songs: &[String]) -> Result<()> {
    let items = songs
        .iter()
        .map(|uri| CollectionItem {
            uri: uri.clone(),
            is_removed: true,
            ..Default::default()
        })
        .collect();
    session
        .spclient()
        .write_collection("collection", items)
        .await
        .context("Spotify didn't un-like the songs")?;
    Ok(())
}

/// Likes songs (track URIs), each liked at the time given (seconds since
/// 1970), as when un-liked, so they go back where they were.
pub async fn like(session: &Session, songs: &[(String, i64)]) -> Result<()> {
    let items = songs
        .iter()
        .map(|(uri, added_at)| CollectionItem {
            uri: uri.clone(),
            added_at: *added_at,
            ..Default::default()
        })
        .collect();
    session
        .spclient()
        .write_collection("collection", items)
        .await
        .context("Spotify didn't like the songs")?;
    Ok(())
}

async fn revision(session: &Session, playlist: &str) -> Result<Option<Vec<u8>>> {
    let id = playlist_id(playlist)?;
    let bytes = session.spclient().get_playlist_range(&id, 0, 0).await?;
    Ok(SelectedListContent::parse_from_bytes(&bytes)?.revision)
}

/// Sends one change, answering the new revision.
async fn post(
    session: &Session,
    playlist: &str,
    base: Option<Vec<u8>>,
    ops: Vec<Op>,
) -> Result<Vec<u8>> {
    let id = playlist_id(playlist)?;
    let reply = session
        .spclient()
        .post_playlist_changes(&id, &changes(session, base, ops))
        .await?;
    Ok(SelectedListContent::parse_from_bytes(&reply)?
        .revision
        .unwrap_or_default())
}

fn item(uri: String) -> Item {
    Item {
        uri: Some(uri),
        ..Default::default()
    }
}

/// Applies `first` (if any) and adds the songs, a batch per change, each
/// change based on the revision the one before answered.
async fn write(
    session: &Session,
    playlist: &str,
    mut revision: Option<Vec<u8>>,
    mut first: Option<Op>,
    songs: &[String],
) -> Result<()> {
    let mut batches: Vec<&[String]> = songs.chunks(BATCH).collect();
    if batches.is_empty() {
        batches.push(&[]);
    }
    for batch in batches {
        let mut ops: Vec<Op> = first.take().into_iter().collect();
        if !batch.is_empty() {
            ops.push(Op {
                kind: Some(op::Kind::ADD.into()),
                add: MessageField::some(Add {
                    items: batch.iter().cloned().map(item).collect(),
                    add_last: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            });
        }
        if ops.is_empty() {
            continue;
        }
        let next = post(session, playlist, revision.take(), ops)
            .await
            .context("Spotify didn't take the songs")?;
        revision = Some(next);
    }
    Ok(())
}

fn changes(session: &Session, base: Option<Vec<u8>>, ops: Vec<Op>) -> ListChanges {
    ListChanges {
        base_revision: base,
        deltas: vec![Delta {
            ops,
            info: info(session),
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn info(session: &Session) -> MessageField<ChangeInfo> {
    MessageField::some(ChangeInfo {
        user: Some(session.username()),
        timestamp: Some(now_ms()),
        ..Default::default()
    })
}

fn playlist_id(uri: &str) -> Result<SpotifyId> {
    match SpotifyUri::from_uri(uri) {
        Ok(SpotifyUri::Playlist { id, .. }) => Ok(id),
        _ => anyhow::bail!("not a Spotify playlist: {uri}"),
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |t| t.as_millis() as i64)
}
