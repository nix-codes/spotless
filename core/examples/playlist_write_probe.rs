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
