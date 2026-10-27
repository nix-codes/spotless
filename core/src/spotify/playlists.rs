//! Writes to the account's Spotify playlists over the librespot session,
//! as Spotify's own clients do: a new playlist, and songs added to the end
//! of one. Spotless never removes anything from them.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use librespot_core::session::Session;
use librespot_core::{SpotifyId, SpotifyUri};
use librespot_protocol::playlist4_external::{
    Add, ChangeInfo, CreateListReply, Delta, Item, ItemAttributes, ListAttributes,
    ListAttributesPartialState, ListChanges, ListUpdateRequest, Op, SelectedListContent,
    UpdateListAttributes, op,
};
use protobuf::{Message, MessageField};
use reqwest::Method;

/// Songs added in one change. Spotify took 432 at once; this leaves a
/// margin.
const BATCH: usize = 400;

/// Makes a private playlist by this name, first in the account's list,
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
    let id = playlist_id(playlist)?;
    let bytes = session.spclient().get_playlist_range(&id, 0, 0).await?;
    let header = SelectedListContent::parse_from_bytes(&bytes)?;
    write(session, playlist, header.revision, None, songs).await
}

/// Applies `first` (if any) and adds the songs, a batch per change, each
/// change based on the revision the one before answered.
async fn write(
    session: &Session,
    playlist: &str,
    mut revision: Option<Vec<u8>>,
    first: Option<Op>,
    songs: &[String],
) -> Result<()> {
    let id = playlist_id(playlist)?;
    let mut first = first;
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
                    items: batch
                        .iter()
                        .map(|uri| Item {
                            uri: Some(uri.clone()),
                            ..Default::default()
                        })
                        .collect(),
                    add_last: Some(true),
                    ..Default::default()
                }),
                ..Default::default()
            });
        }
        if ops.is_empty() {
            continue;
        }
        let reply = session
            .spclient()
            .post_playlist_changes(&id, &changes(session, revision.take(), ops))
            .await
            .context("Spotify didn't take the songs")?;
        revision = SelectedListContent::parse_from_bytes(&reply)?.revision;
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
