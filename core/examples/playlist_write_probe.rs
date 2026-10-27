//! Can Spotless make a Spotify playlist over the librespot session?
//!
//! `create`: makes a private "Spotless test" playlist of two Liked Songs
//! (`spotify::playlists`), adds a third, and reads it all back, checking
//! it is first in the account's list and not on the public profile.
//! `fill URI` adds 432 songs to it, more than one change takes. `delete
//! URI` takes it out of the list again (as Spotify's own Delete does).
//!
//!   cargo run -p spotless-core --example playlist_write_probe -- create
//!   cargo run -p spotless-core --example playlist_write_probe -- fill spotify:playlist:…
//!   cargo run -p spotless-core --example playlist_write_probe -- delete spotify:playlist:…
//!
//! Sign in through the app first: this reuses its saved credential.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use librespot_core::cache::Cache;
use librespot_core::{Session, SessionConfig, SpotifyUri};
use librespot_metadata::{Metadata, Playlist};
use librespot_protocol::playlist4_external::{
    ChangeInfo, Delta, Item, ListChanges, Op, Rem, SelectedListContent, op,
};
use protobuf::{Message, MessageField};
use reqwest::Method;
use spotless_core::paths::Paths;
use spotless_core::spotify::playlists;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let paths = Paths::new()?;
    let cache = Cache::new(Some(paths.config.as_path()), None, None, None)?;
    let Some(credentials) = cache.credentials() else {
        bail!("not signed in: start Spotless and sign in first");
    };
    let session = Session::new(SessionConfig::default(), Some(cache));
    session.connect(credentials, true).await?;
    match args.first().map(String::as_str) {
        Some("create") => create(&session).await,
        Some("edit") => edit(&session).await,
        Some("liked") => liked(&session).await,
        Some("leftovers") => {
            let root = rootlist(&session).await?;
            for (item, meta) in root.contents.items.iter().zip(&root.contents.meta_items) {
                if meta.attributes.name() == "Spotless test" {
                    delete(&session, item.uri()).await?;
                }
            }
            Ok(())
        }
        Some("fill") => fill(&session, args.get(1).context("fill needs the URI")?).await,
        Some("delete") => delete(&session, args.get(1).context("delete needs the URI")?).await,
        _ => bail!("create, or delete URI"),
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn item(uri: &str) -> Item {
    Item {
        uri: Some(uri.into()),
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

async fn rootlist(session: &Session) -> Result<SelectedListContent> {
    let bytes = session.spclient().get_rootlist(0, Some(500)).await?;
    Ok(SelectedListContent::parse_from_bytes(&bytes)?)
}

async fn create(session: &Session) -> Result<()> {
    let liked = session
        .spclient()
        .get_collection_page("collection", "", 3)
        .await?;
    let songs: Vec<String> = liked.items.iter().map(|i| i.uri.clone()).collect();
    let began = std::time::Instant::now();
    let uri = playlists::create(session, "Spotless test", &songs[..2]).await?;
    println!("created {uri} in {:?}", began.elapsed());
    playlists::add(session, &uri, &songs[2..]).await?;
    let playlist = Playlist::get(session, &SpotifyUri::from_uri(&uri)?).await?;
    println!(
        "read back: {:?}, songs {:?} (expected {songs:?})",
        playlist.attributes.name,
        playlist.tracks().map(|t| t.to_uri()).collect::<Vec<_>>()
    );
    let root = rootlist(session).await?;
    let first = &root.contents.items[0];
    println!(
        "list's first: {} public={:?}",
        first.uri(),
        first.attributes.public
    );
    let profile = session
        .spclient()
        .get_user_profile(&session.username(), Some(200), Some(0))
        .await?;
    println!(
        "on the public profile: {}",
        String::from_utf8_lossy(&profile).contains(&uri)
    );
    Ok(())
}

/// Removes songs by position (one copy of a song twice in it), puts them
/// back where they were, and tries a removal on a stale revision, on a
/// throwaway "Spotless test" playlist, taken out of the list after.
async fn edit(session: &Session) -> Result<()> {
    let liked = session
        .spclient()
        .get_collection_page("collection", "", 3)
        .await?;
    let s: Vec<String> = liked.items.iter().map(|i| i.uri.clone()).collect();
    // a b a c: the second "a" is removed, with c.
    let songs = vec![s[0].clone(), s[1].clone(), s[0].clone(), s[2].clone()];
    let uri = playlists::create(session, "Spotless test", &songs).await?;
    let target = SpotifyUri::from_uri(&uri)?;
    let show = |p: &Playlist| -> Vec<String> {
        p.tracks()
            .map(|t| {
                let u = t.to_uri();
                ["a", "b", "c"][s.iter().position(|x| *x == u).unwrap()].to_string()
            })
            .collect()
    };
    let before = Playlist::get(session, &target).await?;
    println!("made: {:?}", show(&before));
    let stale = before.revision.clone();
    let removed = vec![(2, s[0].clone()), (3, s[2].clone())];
    let revision = playlists::remove(session, &uri, &removed).await?;
    let after = Playlist::get(session, &target).await?;
    println!("removed 2 and 3: {:?}", show(&after));
    let _ = revision;
    // c, at 3, is gone since that revision: removing it again must not
    // take another song.
    match playlists::remove_at(session, &uri, stale, &[(3, s[2].clone())]).await {
        Ok(_) => println!(
            "stale removal answered OK, now: {:?}",
            show(&Playlist::get(session, &target).await?)
        ),
        Err(e) => println!("stale removal refused: {e:#}"),
    }
    playlists::put_back(session, &uri, &removed).await?;
    let back = Playlist::get(session, &target).await?;
    println!("put back: {:?}", show(&back));
    delete(session, &uri).await
}

/// Un-likes the latest liked song and likes it again with its date.
async fn liked(session: &Session) -> Result<()> {
    let page = session
        .spclient()
        .get_collection_page("collection", "", 500)
        .await?;
    let mut items: Vec<_> = page
        .items
        .iter()
        .filter(|i| i.uri.starts_with("spotify:track:"))
        .cloned()
        .collect();
    items.sort_by_key(|i| std::cmp::Reverse(i.added_at));
    let latest = items[0].clone();
    let second = items[1].clone();
    println!(
        "latest {} at {}, then {} at {}",
        latest.uri, latest.added_at, second.uri, second.added_at
    );
    playlists::unlike(session, std::slice::from_ref(&latest.uri)).await?;
    let gone = !session
        .spclient()
        .get_collection_page("collection", "", 500)
        .await?
        .items
        .iter()
        .any(|i| i.uri == latest.uri && !i.is_removed);
    println!("un-liked: {gone}");
    playlists::like(session, &[(latest.uri.clone(), latest.added_at)]).await?;
    let found = session
        .spclient()
        .get_collection_page("collection", "", 500)
        .await?
        .items
        .into_iter()
        .find(|i| i.uri == latest.uri && !i.is_removed);
    println!(
        "liked again: {:?}",
        found.map(|i| (i.added_at, i.added_at == latest.added_at))
    );
    Ok(())
}

/// Adds a big playlist's songs, 432 of them (more than a batch).
async fn fill(session: &Session, uri: &str) -> Result<()> {
    let big = SpotifyUri::from_uri("spotify:playlist:5FI8rn340FgsOB7B8Ic0DZ")?;
    let songs: Vec<String> = Playlist::get(session, &big)
        .await?
        .tracks()
        .map(|t| t.to_uri())
        .collect();
    let target = SpotifyUri::from_uri(uri)?;
    let before = Playlist::get(session, &target).await?.tracks().len();
    let began = std::time::Instant::now();
    playlists::add(session, uri, &songs).await?;
    let after = Playlist::get(session, &target).await?.tracks().len();
    println!(
        "added {} in {:?}: {before} -> {after} songs",
        songs.len(),
        began.elapsed()
    );
    Ok(())
}

async fn delete(session: &Session, uri: &str) -> Result<()> {
    let root = rootlist(session).await?;
    let Some(at) = root.contents.items.iter().position(|i| i.uri() == uri) else {
        bail!("{uri} isn't in the list");
    };
    let changes = ListChanges {
        base_revision: root.revision.clone(),
        deltas: vec![Delta {
            ops: vec![Op {
                kind: Some(op::Kind::REM.into()),
                rem: MessageField::some(Rem {
                    from_index: Some(at as i32),
                    length: Some(1),
                    items: vec![item(uri)],
                    ..Default::default()
                }),
                ..Default::default()
            }],
            info: info(session),
            ..Default::default()
        }],
        ..Default::default()
    };
    let endpoint = format!("/playlist/v2/user/{}/rootlist/changes", session.username());
    session
        .spclient()
        .request_with_protobuf(&Method::POST, &endpoint, None, &changes)
        .await
        .context("remove from rootlist")?;
    let root = rootlist(session).await?;
    println!(
        "removed: {}",
        !root.contents.items.iter().any(|i| i.uri() == uri)
    );
    Ok(())
}
