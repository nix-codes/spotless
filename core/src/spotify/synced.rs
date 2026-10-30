//! Synced playlists' Spotify side (see `model::sync`): private playlists
//! in a "Spotless" folder of the account's list, read with their songs'
//! IDs, changed a few edits at a time on the revision they were read at,
//! and followed as Spotify tells of changes.

use anyhow::{Context, Result, bail};
use futures_util::{Stream, StreamExt};
use librespot_core::dealer::protocol::Message as DealerMessage;
use librespot_core::session::Session;
use librespot_core::{SpotifyId, SpotifyUri};
use librespot_protocol::playlist4_external::{
    Add, Item, ListAttributes, ListAttributesPartialState, Mov, Op, PlaylistModificationInfo, Rem,
    SelectedListContent, UpdateListAttributes, op,
};
use protobuf::{Message, MessageField};

use super::catalog;
use super::playlists::{self, now_ms, post, post_rootlist};
use crate::model::sync::{Change, ItemId, Position, Slot, changes, merge};
use crate::model::{EntryId, TrackRef};

/// The folder synced playlists are kept in.
pub const FOLDER: &str = "Spotless";

/// Songs added in one change, as `playlists` adds them.
const ADD_BATCH: usize = 400;

/// Items of the account's list read in one request.
const ROOTLIST_PAGE: usize = 200;

/// Songs read in one request of a long playlist.
const PAGE: usize = 500;

/// A synced playlist as Spotify has it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Remote {
    pub revision: Vec<u8>,
    pub name: String,
    pub description: String,
    /// Its songs in order, each by its ID there and its track URI.
    pub songs: Vec<(ItemId, String)>,
}

/// Reads a playlist with its songs' IDs.
pub async fn read(session: &Session, uri: &str) -> Result<Remote> {
    let id = playlist_id(uri)?;
    let bytes = session.spclient().get_playlist(&id).await?;
    let list = SelectedListContent::parse_from_bytes(&bytes)?;
    let mut remote = Remote {
        revision: list.revision().to_vec(),
        name: list.attributes.name().to_string(),
        description: list.attributes.description().to_string(),
        songs: songs(&list),
    };
    let length = list.length().max(0) as usize;
    while remote.songs.len() < length {
        let bytes = session
            .spclient()
            .get_playlist_range(&id, remote.songs.len(), PAGE)
            .await?;
        let page = SelectedListContent::parse_from_bytes(&bytes)?;
        if page.revision() != remote.revision.as_slice() {
            bail!("the playlist changed while it was read");
        }
        let more = songs(&page);
        if more.is_empty() {
            break;
        }
        remote.songs.extend(more);
    }
    Ok(remote)
}

fn songs(list: &SelectedListContent) -> Vec<(ItemId, String)> {
    list.contents
        .items
        .iter()
        .map(|item| {
            let id = ItemId::from_bytes(item.attributes.item_id());
            (id, item.uri().to_string())
        })
        .collect()
}

/// An edit to a synced playlist, by index in it as it is when the edit is
/// made (see `model::sync::Change`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    /// Removes the songs at `at` on, which are these, by track URI.
    Remove {
        at: usize,
        songs: Vec<String>,
    },
    /// Moves the song at `from`, which is this one, to `to`, counted after
    /// it's taken out.
    Move {
        from: usize,
        to: usize,
        song: String,
    },
    Add {
        at: usize,
        songs: Vec<String>,
    },
    Rename(String),
    /// Sets the description, where the position is kept.
    Describe(String),
}

/// Makes these edits, as one change on `revision`. If the playlist
/// changed since, Spotify makes them on what it is now, finding the songs
/// removed and moved by the URIs given, or refuses them. Answers the new
/// revision.
pub async fn edit(
    session: &Session,
    uri: &str,
    revision: Vec<u8>,
    edits: &[Edit],
) -> Result<Vec<u8>> {
    let ops = edits.iter().map(op).collect();
    post(session, uri, Some(revision), ops).await
}

fn op(edit: &Edit) -> Op {
    let at = |n: usize| Some(n as i32);
    match edit {
        Edit::Remove { at: from, songs } => Op {
            kind: Some(op::Kind::REM.into()),
            rem: MessageField::some(Rem {
                from_index: at(*from),
                length: at(songs.len()),
                items: items(songs),
                ..Default::default()
            }),
            ..Default::default()
        },
        // Spotify counts where it goes before the song is taken out.
        Edit::Move { from, to, song } => Op {
            kind: Some(op::Kind::MOV.into()),
            mov: MessageField::some(Mov {
                from_index: at(*from),
                length: Some(1),
                to_index: at(if to > from { to + 1 } else { *to }),
                items: items(std::slice::from_ref(song)),
                ..Default::default()
            }),
            ..Default::default()
        },
        Edit::Add { at: to, songs } => Op {
            kind: Some(op::Kind::ADD.into()),
            add: MessageField::some(Add {
                from_index: at(*to),
                items: items(songs),
                ..Default::default()
            }),
            ..Default::default()
        },
        Edit::Rename(name) => attributes(ListAttributes {
            name: Some(name.clone()),
            ..Default::default()
        }),
        Edit::Describe(text) => attributes(ListAttributes {
            description: Some(text.clone()),
            ..Default::default()
        }),
    }
}

fn items(uris: &[String]) -> Vec<Item> {
    uris.iter()
        .map(|uri| Item {
            uri: Some(uri.clone()),
            ..Default::default()
        })
        .collect()
}

fn attributes(values: ListAttributes) -> Op {
    Op {
        kind: Some(op::Kind::UPDATE_LIST_ATTRIBUTES.into()),
        update_list_attributes: MessageField::some(UpdateListAttributes {
            new_attributes: MessageField::some(ListAttributesPartialState {
                values: MessageField::some(values),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// What a synced playlist's sync starts from: this device's side.
#[derive(Clone, Debug, Default)]
pub struct Job {
    pub uri: String,
    /// Its songs when they were last the same here and there.
    pub base: Vec<ItemId>,
    /// This device's songs: each entry, with its Spotify ID if it has one,
    /// and its track URI.
    pub local: Vec<(EntryId, Option<ItemId>, String)>,
    /// A name to give it, renamed here.
    pub rename: Option<String>,
    /// A position to write, unless Spotify's is newer or says the same.
    pub position: Option<Position>,
}

/// What a sync made of it.
#[derive(Clone, Debug)]
pub struct Done {
    /// The playlist as Spotify has it after.
    pub remote: Remote,
    /// The job's songs, those added given the IDs Spotify gave them.
    pub sent: Vec<(EntryId, Option<ItemId>)>,
    /// The songs it has that this device didn't, by ID (None: Spotify
    /// doesn't know the track any more).
    pub tracks: Vec<(ItemId, Option<TrackRef>)>,
    /// The position its description holds, if any.
    pub position: Option<Position>,
}

/// Makes Spotify's playlist this device's and the others' changes met
/// ([`merge`]), writes the position if it's the newer, and reads it back.
pub async fn sync(session: &Session, job: Job) -> Result<Done> {
    let remote = read(session, &job.uri).await?;
    let remote_items: Vec<ItemId> = remote.songs.iter().map(|(id, _)| id.clone()).collect();
    let rows: Vec<(EntryId, Option<ItemId>)> = job
        .local
        .iter()
        .map(|(id, item, _)| (*id, item.clone()))
        .collect();
    let merged = merge(&job.base, &rows, &remote_items);
    let new_uri: std::collections::HashMap<EntryId, &str> = job
        .local
        .iter()
        .map(|(id, _, uri)| (*id, uri.as_str()))
        .collect();
    let uri_of = |slot: &Slot| -> String {
        match slot {
            Slot::Item(item) => remote
                .songs
                .iter()
                .find(|(id, _)| id == item)
                .map(|(_, uri)| uri.clone())
                .unwrap_or_default(),
            Slot::New(id) => new_uri.get(id).copied().unwrap_or_default().to_string(),
        }
    };

    // The changes as edits, with the songs they touch, found by making
    // them on the list's track URIs.
    let mut now: Vec<String> = remote.songs.iter().map(|(_, uri)| uri.clone()).collect();
    let mut edits = Vec::new();
    for change in changes(&remote_items, &merged) {
        match change {
            Change::Remove { at, len } => {
                let songs: Vec<String> = now.drain(at..at + len).collect();
                edits.push(Edit::Remove { at, songs });
            }
            Change::Move { from, to } => {
                let song = now.remove(from);
                now.insert(to, song.clone());
                edits.push(Edit::Move { from, to, song });
            }
            Change::Add { at, songs } => {
                let songs: Vec<String> = songs.iter().map(|id| uri_of(&Slot::New(*id))).collect();
                now.splice(at..at, songs.iter().cloned());
                for (n, batch) in songs.chunks(ADD_BATCH).enumerate() {
                    edits.push(Edit::Add {
                        at: at + n * ADD_BATCH,
                        songs: batch.to_vec(),
                    });
                }
            }
        }
    }
    if let Some(name) = job.rename.filter(|name| *name != remote.name) {
        edits.push(Edit::Rename(name));
    }
    let theirs = Position::decode(&remote.description);
    if let Some(position) = job.position {
        let newer = theirs.as_ref().is_none_or(|theirs| {
            position.saved_at > theirs.saved_at && !position.same_place(theirs)
        });
        if newer {
            edits.push(Edit::Describe(position.encode()));
        }
    }

    let after = if edits.is_empty() {
        remote.clone()
    } else {
        // A change at most a batch of songs added, each on the revision the
        // one before answered.
        let mut revision = remote.revision.clone();
        let mut batch: Vec<Edit> = Vec::new();
        let mut added = 0;
        for edit in edits {
            let adds = match &edit {
                Edit::Add { songs, .. } => songs.len(),
                _ => 0,
            };
            if added + adds > ADD_BATCH && !batch.is_empty() {
                revision = self::edit(session, &job.uri, revision, &batch).await?;
                batch.clear();
                added = 0;
            }
            added += adds;
            batch.push(edit);
        }
        if !batch.is_empty() {
            self::edit(session, &job.uri, revision, &batch).await?;
        }
        read(session, &job.uri).await?
    };

    // The songs added get the IDs Spotify gave them: where the merge put
    // them, or else the first not taken of the same track.
    let before: std::collections::HashSet<&ItemId> = remote_items.iter().collect();
    let mut taken: std::collections::HashSet<ItemId> = std::collections::HashSet::new();
    let mut given: std::collections::HashMap<EntryId, ItemId> = std::collections::HashMap::new();
    let same_shape = after.songs.len() == merged.len();
    for (k, slot) in merged.iter().enumerate() {
        let Slot::New(id) = slot else { continue };
        let uri = uri_of(slot);
        let at_place = same_shape
            .then(|| &after.songs[k])
            .filter(|(item, song)| *song == uri && !before.contains(item) && !taken.contains(item));
        let found = at_place.or_else(|| {
            remote.songs.iter().find(|(item, song)| {
                *song == uri && !before.contains(item) && !taken.contains(item)
            })
        });
        if let Some((item, _)) = found {
            taken.insert(item.clone());
            given.insert(*id, item.clone());
        }
    }
    let sent: Vec<(EntryId, Option<ItemId>)> = rows
        .into_iter()
        .map(|(id, item)| (id, item.or_else(|| given.get(&id).cloned())))
        .collect();

    // The songs this device doesn't have, read.
    let known: std::collections::HashSet<&ItemId> =
        sent.iter().filter_map(|(_, item)| item.as_ref()).collect();
    let missing: Vec<(SpotifyUri, ItemId)> = remote
        .songs
        .iter()
        .filter(|(item, _)| !known.contains(item))
        .filter_map(|(item, uri)| Some((SpotifyUri::from_uri(uri).ok()?, item.clone())))
        .collect();
    let mut tracks: Vec<(ItemId, Option<TrackRef>)> = remote
        .songs
        .iter()
        .filter(|(item, _)| !known.contains(item))
        .map(|(item, _)| (item.clone(), None))
        .collect();
    if !missing.is_empty()
        && let Ok(found) = catalog::tracks_with(session, missing).await
    {
        for (track, item) in found {
            if let Some(slot) = tracks.iter_mut().find(|(id, _)| *id == item) {
                slot.1 = Some(track);
            }
        }
    }
    let position = Position::decode(&after.description);
    Ok(Done {
        remote: after,
        sent,
        tracks,
        position,
    })
}

/// The playlists in the Spotless folder, by URI and name, in order.
pub async fn folder(session: &Session) -> Result<Vec<(String, String)>> {
    let root = rootlist(session).await?;
    let Some((start, end)) = find_folder(&root) else {
        return Ok(Vec::new());
    };
    let items = &root.contents.items;
    let metas = &root.contents.meta_items;
    Ok((start + 1..end)
        .filter(|&i| items[i].uri().starts_with("spotify:playlist:"))
        .map(|i| {
            let name = metas
                .get(i)
                .map(|m| m.attributes.name())
                .unwrap_or_default();
            (items[i].uri().to_string(), name.to_string())
        })
        .collect())
}

/// What [`create`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Created {
    /// Made, by this URI.
    Made(String),
    /// Not made: the Spotless folder has a playlist by that name already.
    NameTaken,
}

/// Whether two playlists' names are the same, case and spaces round them
/// aside.
pub fn same_name(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// Makes a private playlist by this name, last in the Spotless folder
/// (made first in the account's list if there's none), with these songs.
/// Unless told to `replace` them, deleting them first, it makes none if
/// the folder has one by that name already.
pub async fn create(
    session: &Session,
    name: &str,
    songs: &[String],
    replace: bool,
) -> Result<Created> {
    let taken: Vec<String> = folder(session)
        .await?
        .into_iter()
        .filter(|(_, other)| same_name(other, name))
        .map(|(uri, _)| uri)
        .collect();
    if !taken.is_empty() && !replace {
        return Ok(Created::NameTaken);
    }
    for uri in &taken {
        playlists::unlist(session, uri)
            .await
            .context("the playlist by that name couldn't be deleted")?;
    }
    create_in_folder(session, name, songs)
        .await
        .map(Created::Made)
}

/// Makes the playlist and moves it into the folder; answers its URI.
async fn create_in_folder(session: &Session, name: &str, songs: &[String]) -> Result<String> {
    let uri = playlists::create(session, name, true, songs).await?;
    // `create` lists it first; it moves into the folder.
    let root = rootlist(session).await?;
    let at = root
        .contents
        .items
        .iter()
        .position(|item| item.uri() == uri)
        .context("the new playlist isn't in your list")?;
    let ops = match find_folder(&root) {
        Some((_, end)) => vec![Op {
            kind: Some(op::Kind::MOV.into()),
            mov: MessageField::some(Mov {
                from_index: Some(at as i32),
                length: Some(1),
                to_index: Some(end as i32),
                ..Default::default()
            }),
            ..Default::default()
        }],
        // Around it: the folder's start before, its end after.
        None => {
            let group = format!("{:016x}", now_ms());
            let start = format!("spotify:start-group:{group}:{FOLDER}");
            let end = format!("spotify:end-group:{group}");
            let add = |uri: String, to: usize| Op {
                kind: Some(op::Kind::ADD.into()),
                add: MessageField::some(Add {
                    from_index: Some(to as i32),
                    items: vec![Item {
                        uri: Some(uri),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
                ..Default::default()
            };
            vec![add(end, at + 1), add(start, at)]
        }
    };
    post_rootlist(session, root.revision.clone(), ops)
        .await
        .context("Spotify made the playlist but didn't put it in the Spotless folder")?;
    Ok(uri)
}

/// Takes the Spotless folder out of the account's list if nothing is
/// left in it. Answers whether it did.
pub async fn remove_empty_folder(session: &Session) -> Result<bool> {
    let root = rootlist(session).await?;
    let Some((start, end)) = find_folder(&root) else {
        return Ok(false);
    };
    if end != start + 1 {
        return Ok(false);
    }
    let ops = vec![Op {
        kind: Some(op::Kind::REM.into()),
        rem: MessageField::some(Rem {
            from_index: Some(start as i32),
            length: Some(2),
            ..Default::default()
        }),
        ..Default::default()
    }];
    post_rootlist(session, root.revision.clone(), ops).await?;
    Ok(true)
}

/// The account's whole list, folders' starts and ends counted, read a
/// page at a time.
async fn rootlist(session: &Session) -> Result<SelectedListContent> {
    let mut root: Option<SelectedListContent> = None;
    loop {
        let from = root.as_ref().map_or(0, |r| r.contents.items.len());
        let bytes = session
            .spclient()
            .get_rootlist(from, Some(ROOTLIST_PAGE))
            .await?;
        let page = SelectedListContent::parse_from_bytes(&bytes)?;
        let done = page.contents.items.is_empty()
            || from + page.contents.items.len() >= page.length().max(0) as usize;
        match &mut root {
            None => root = Some(page),
            Some(root) => {
                if root.revision != page.revision {
                    bail!("your list changed while it was read");
                }
                let contents = root.contents.mut_or_insert_default();
                contents.items.extend(page.contents.items.iter().cloned());
                contents
                    .meta_items
                    .extend(page.contents.meta_items.iter().cloned());
            }
        }
        if done {
            return root.context("your list can't be read");
        }
    }
}

/// Where the Spotless folder starts and ends in the list.
fn find_folder(root: &SelectedListContent) -> Option<(usize, usize)> {
    let items = &root.contents.items;
    let (start, group) = items.iter().enumerate().find_map(|(i, item)| {
        let rest = item.uri().strip_prefix("spotify:start-group:")?;
        let (group, name) = rest.split_once(':')?;
        (decode_name(name) == FOLDER).then(|| (i, group.to_string()))
    })?;
    let end_uri = format!("spotify:end-group:{group}");
    let end = items[start..]
        .iter()
        .position(|item| item.uri() == end_uri)?
        + start;
    Some((start, end))
}

/// A folder's name as its URI has it: `+` for spaces, `%XX` escapes.
fn decode_name(name: &str) -> String {
    let bytes = name.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escaped = (bytes[i] == b'%')
            .then(|| bytes.get(i + 1..i + 3))
            .flatten()
            .and_then(|hex| u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok());
        match (bytes[i], escaped) {
            (_, Some(b)) => {
                out.push(b);
                i += 2;
            }
            (b'+', None) => out.push(b' '),
            (b, None) => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A change Spotify tells of: the playlist's URI and its new revision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Changed {
    pub uri: String,
    pub revision: Vec<u8>,
}

/// What Spotify tells of the account's playlists' changes, on `session`'s
/// dealer, which may start before or after.
pub fn listen(session: &Session) -> Result<impl Stream<Item = Changed> + Send + use<>> {
    let changes =
        session
            .dealer()
            .listen_for("hm://playlist/v2/playlist/", |message: DealerMessage| {
                let id = message
                    .uri
                    .rsplit('/')
                    .next()
                    .unwrap_or_default()
                    .to_string();
                let info = DealerMessage::from_raw::<PlaylistModificationInfo>(message)?;
                Ok(Changed {
                    uri: format!("spotify:playlist:{id}"),
                    revision: info.new_revision().to_vec(),
                })
            })?;
    Ok(changes.filter_map(
        |change: Result<Changed, librespot_core::Error>| async move {
            change
                .map_err(|e| log::warn!("unreadable playlist change: {e}"))
                .ok()
        },
    ))
}

/// When Spotify tells of a change to the account's list of playlists (its
/// rootlist, folders and all): one taken out, put in, moved or renamed.
pub fn listen_list(session: &Session) -> Result<impl Stream<Item = ()> + Send + use<>> {
    let changes =
        session
            .dealer()
            .listen_for("hm://playlist/v2/user/", |message: DealerMessage| {
                log::debug!("the list of playlists changed: {}", message.uri);
                Ok(())
            })?;
    Ok(changes.filter_map(|change: Result<(), librespot_core::Error>| async move { change.ok() }))
}

fn playlist_id(uri: &str) -> Result<SpotifyId> {
    match SpotifyUri::from_uri(uri) {
        Ok(SpotifyUri::Playlist { id, .. }) => Ok(id),
        _ => bail!("not a Spotify playlist: {uri}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_names_are_unescaped() {
        assert_eq!(decode_name("Spotless"), "Spotless");
        assert_eq!(decode_name("My+Mixes%21"), "My Mixes!");
        assert_eq!(decode_name("100%"), "100%");
    }

    #[test]
    fn the_folder_is_found_by_name_with_its_end() {
        let item = |uri: &str| Item {
            uri: Some(uri.into()),
            ..Default::default()
        };
        let mut root = SelectedListContent::new();
        root.contents.mut_or_insert_default().items = vec![
            item("spotify:playlist:a"),
            item("spotify:start-group:0f:Other"),
            item("spotify:end-group:0f"),
            item("spotify:start-group:1e:Spotless"),
            item("spotify:playlist:b"),
            item("spotify:end-group:1e"),
        ];
        assert_eq!(find_folder(&root), Some((3, 5)));
    }
}
