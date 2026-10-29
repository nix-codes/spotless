//! Can a Spotify playlist carry Spotless's own data, for synced playlists?
//!
//! `list`: makes an empty private "Spotless test" playlist, sets two
//! made-up pairs in its `format_attributes` (`spotless.cursor`,
//! `spotless.resume_ms`), and reads them back on a second, fresh session.
//! Then it changes only `spotless.resume_ms` and reads again, to see
//! whether a change replaces the whole list of pairs or just that key.
//! `songs`: the same per song. Makes another, adds a liked song plainly
//! (the control), adds a second with a pair, gives the first a pair
//! afterwards (`UPDATE_ITEM_ATTRIBUTES`), and reads the songs back on a
//! fresh session.
//! `description`: how much a description takes and keeps exactly. Writes
//! longer and longer ones (from 300 characters, doubling) and compares
//! what a second session reads back; then one with separators, newlines
//! and accents; then times how long a session already open takes to see
//! a change.
//! `push`: does Spotify tell an open session that a playlist changed? A
//! listening session starts its dealer, logging every message it gets
//! (run with `RUST_LOG=librespot_core::dealer=debug` to see those nobody
//! asked for too); another changes the playlist's description, then adds
//! a song, and the listener says what arrived and when.
//! `synced`: `spotify::synced` against Spotify: makes two playlists in
//! the Spotless folder (making it), reads them with their songs' IDs,
//! moves songs both ways, removes and adds in one change, sends a change
//! on a stale revision, and sets the position.
//! The playlist is kept for the next experiments; `clean` takes every
//! "Spotless test" playlist out of the account's list.
//!
//!   cargo run -p spotless-core --example sync_probe -- list
//!   cargo run -p spotless-core --example sync_probe -- songs
//!   cargo run -p spotless-core --example sync_probe -- description
//!   cargo run -p spotless-core --example sync_probe -- push
//!   cargo run -p spotless-core --example sync_probe -- synced
//!   cargo run -p spotless-core --example sync_probe -- folder
//!   cargo run -p spotless-core --example sync_probe -- control spotify:playlist:…
//!   cargo run -p spotless-core --example sync_probe -- clean
//!
//! Sign in through the app first: this reuses its saved credential.

use anyhow::{Context, Result, bail};
use librespot_core::cache::Cache;
use librespot_core::{Session, SessionConfig, SpotifyId, SpotifyUri};
use librespot_protocol::playlist4_external::{
    Add, ChangeInfo, Delta, FormatListAttribute, Item, ItemAttributes, ItemAttributesPartialState,
    ListAttributes, ListAttributesPartialState, ListChanges, Op, PlaylistModificationInfo,
    SelectedListContent, UpdateItemAttributes, UpdateListAttributes, op,
};
use protobuf::{Message, MessageField};
use spotless_core::paths::Paths;
use spotless_core::spotify::playlists;
use spotless_core::spotify::synced::{self, Edit};

const NAME: &str = "Spotless test";

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("list") => list().await,
        Some("control") => control(args.get(1).context("control needs the URI")?).await,
        Some("clean") => clean().await,
        Some("songs") => songs().await,
        Some("description") => description().await,
        Some("push") => push().await,
        Some("synced") => synced_side().await,
        Some("folder") => {
            let s = session().await?;
            println!("{:?}", synced::folder(&s).await?);
            Ok(())
        }
        Some("variants") => variants(args.get(1).context("variants needs the URI")?).await,
        _ => bail!("list, or clean"),
    }
}

async fn session() -> Result<Session> {
    let paths = Paths::new()?;
    let cache = Cache::new(Some(paths.config.as_path()), None, None, None)?;
    let Some(credentials) = cache.credentials() else {
        bail!("not signed in: start Spotless and sign in first");
    };
    let session = Session::new(SessionConfig::default(), Some(cache));
    session.connect(credentials, true).await?;
    Ok(session)
}

async fn list() -> Result<()> {
    let writer = session().await?;
    let uri = playlists::create(&writer, NAME, true, &[]).await?;
    println!("made {uri}");

    set(
        &writer,
        &uri,
        &[
            ("spotless.cursor", "item-1234"),
            ("spotless.resume_ms", "61000"),
        ],
    )
    .await
    .context("Spotify refused the first pairs")?;
    println!("set cursor + resume_ms");
    show(&uri).await?;

    set(&writer, &uri, &[("spotless.resume_ms", "99000")])
        .await
        .context("Spotify refused the second change")?;
    println!("set resume_ms only");
    show(&uri).await?;
    Ok(())
}

/// Reads the playlist's attributes on a new session, as another device
/// would.
async fn show(uri: &str) -> Result<()> {
    let reader = session().await?;
    let bytes = reader.spclient().get_playlist(&playlist_id(uri)?).await?;
    let list = SelectedListContent::parse_from_bytes(&bytes)?;
    let attributes = &list.attributes;
    println!("  name {:?}", attributes.name());
    println!("  format {:?}", attributes.format);
    if attributes.format_attributes.is_empty() {
        println!("  no format_attributes");
    }
    for pair in &attributes.format_attributes {
        println!("  {} = {}", pair.key(), pair.value());
    }
    Ok(())
}

async fn set(session: &Session, uri: &str, pairs: &[(&str, &str)]) -> Result<()> {
    let attributes = ListAttributes {
        format_attributes: pairs
            .iter()
            .map(|(key, value)| FormatListAttribute {
                key: Some(key.to_string()),
                value: Some(value.to_string()),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    change(session, uri, attributes).await
}

async fn change(session: &Session, uri: &str, attributes: ListAttributes) -> Result<()> {
    let change = Op {
        kind: Some(op::Kind::UPDATE_LIST_ATTRIBUTES.into()),
        update_list_attributes: MessageField::some(UpdateListAttributes {
            new_attributes: MessageField::some(ListAttributesPartialState {
                values: MessageField::some(attributes),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    send(session, uri, change).await
}

/// Sends one op, based on the playlist's revision now.
async fn send(session: &Session, uri: &str, change: Op) -> Result<()> {
    let id = playlist_id(uri)?;
    let bytes = session.spclient().get_playlist_range(&id, 0, 0).await?;
    let base = SelectedListContent::parse_from_bytes(&bytes)?.revision;
    let changes = ListChanges {
        base_revision: base,
        deltas: vec![Delta {
            ops: vec![change],
            info: MessageField::some(ChangeInfo {
                user: Some(session.username()),
                timestamp: Some(now_ms()),
                ..Default::default()
            }),
            ..Default::default()
        }],
        ..Default::default()
    };
    session
        .spclient()
        .post_playlist_changes(&id, &changes)
        .await?;
    Ok(())
}

/// The same change with a description instead of pairs, to tell a
/// refused key from a bad request.
async fn control(uri: &str) -> Result<()> {
    let writer = session().await?;
    let attributes = ListAttributes {
        description: Some("probe".into()),
        ..Default::default()
    };
    change(&writer, uri, attributes)
        .await
        .context("Spotify refused the description")?;
    println!("set description");
    let reader = session().await?;
    let bytes = reader.spclient().get_playlist(&playlist_id(uri)?).await?;
    let list = SelectedListContent::parse_from_bytes(&bytes)?;
    println!("  description {:?}", list.attributes.description);
    Ok(())
}

/// Pairs that differ in one way each, to find what Spotify objects to.
async fn variants(uri: &str) -> Result<()> {
    let writer = session().await?;
    let pair = |key: &str, value: &str| FormatListAttribute {
        key: Some(key.into()),
        value: Some(value.into()),
        ..Default::default()
    };
    let tries = [
        ("key without a dot", None, pair("spotlesscursor", "x")),
        ("a key Spotify uses", None, pair("primary_color", "#ff0000")),
        (
            "format set too",
            Some("spotless"),
            pair("spotless.cursor", "x"),
        ),
        (
            "Spotify's format",
            Some("format-shows-shuffle"),
            pair("spotless.cursor", "x"),
        ),
    ];
    for (what, format, pair) in tries {
        let attributes = ListAttributes {
            format: format.map(str::to_string),
            format_attributes: vec![pair],
            ..Default::default()
        };
        match change(&writer, uri, attributes).await {
            Ok(()) => println!("{what}: taken"),
            Err(e) => println!("{what}: refused ({e:#})"),
        }
    }
    let attributes = ListAttributes {
        format: Some("spotless".into()),
        ..Default::default()
    };
    match change(&writer, uri, attributes).await {
        Ok(()) => println!("format alone: taken"),
        Err(e) => println!("format alone: refused ({e:#})"),
    }
    show(uri).await
}

async fn songs() -> Result<()> {
    let writer = session().await?;
    let liked = writer
        .spclient()
        .get_collection_page("collection", "", 2)
        .await?;
    let liked: Vec<String> = liked.items.iter().map(|i| i.uri.clone()).collect();
    if liked.len() < 2 {
        bail!("needs two Liked Songs");
    }
    let uri = playlists::create(&writer, NAME, true, &[]).await?;
    println!("made {uri}");
    let pair = |value: &str| FormatListAttribute {
        key: Some("spotless.source".into()),
        value: Some(value.into()),
        ..Default::default()
    };
    let add = |song: &str, attributes: Option<ItemAttributes>| Op {
        kind: Some(op::Kind::ADD.into()),
        add: MessageField::some(Add {
            items: vec![Item {
                uri: Some(song.to_string()),
                attributes: attributes.into(),
                ..Default::default()
            }],
            add_last: Some(true),
            ..Default::default()
        }),
        ..Default::default()
    };

    let tries = [
        ("plain song", add(&liked[0], None)),
        (
            "song with a pair",
            add(
                &liked[1],
                Some(ItemAttributes {
                    format_attributes: vec![pair("added-with")],
                    ..Default::default()
                }),
            ),
        ),
        (
            "pair given afterwards",
            Op {
                kind: Some(op::Kind::UPDATE_ITEM_ATTRIBUTES.into()),
                update_item_attributes: MessageField::some(UpdateItemAttributes {
                    index: Some(0),
                    new_attributes: MessageField::some(ItemAttributesPartialState {
                        values: MessageField::some(ItemAttributes {
                            format_attributes: vec![pair("given-after")],
                            ..Default::default()
                        }),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ),
    ];
    for (what, change) in tries {
        match send(&writer, &uri, change).await {
            Ok(()) => println!("{what}: taken"),
            Err(e) => println!("{what}: refused ({e:#})"),
        }
    }

    let reader = session().await?;
    let bytes = reader.spclient().get_playlist(&playlist_id(&uri)?).await?;
    let list = SelectedListContent::parse_from_bytes(&bytes)?;
    for item in &list.contents.items {
        let attributes = &item.attributes;
        let pairs: Vec<String> = attributes
            .format_attributes
            .iter()
            .map(|pair| format!("{}={}", pair.key(), pair.value()))
            .collect();
        println!(
            "  {} item_id {} pairs {:?}",
            item.uri(),
            attributes.item_id.as_deref().map(hex).unwrap_or_default(),
            pairs
        );
    }
    Ok(())
}

async fn description() -> Result<()> {
    let writer = session().await?;
    let reader = session().await?;
    let uri = playlist_id_of(playlists::create(&writer, NAME, true, &[]).await?)?;
    println!("made {}", uri.0);

    // What the data would look like: IDs, numbers and separators.
    let filler = |n: usize| -> String {
        "d671005843c68fbb=61000|q:0a1b2c3d;"
            .chars()
            .cycle()
            .take(n)
            .collect()
    };
    // The longest taken, between 300 (taken) and 600 (refused).
    let (mut taken, mut refused) = (300, 600);
    while refused - taken > 1 {
        let n = (taken + refused) / 2;
        if write_description(&writer, &uri.0, &filler(n)).await.is_ok() {
            taken = n;
        } else {
            refused = n;
        }
    }
    let back = read_description(&reader, &uri.1).await?;
    println!(
        "longest taken: {taken} (read back {}, same: {})",
        back.chars().count(),
        back == filler(taken)
    );
    // Characters or bytes: the same length in two-byte letters.
    let accents: String = "é".repeat(taken);
    match write_description(&writer, &uri.0, &accents).await {
        Ok(()) => println!("{taken} two-byte letters: taken, so the limit is in characters"),
        Err(_) => println!("{taken} two-byte letters: refused, so the limit is in bytes"),
    }

    let tries = [
        "a|b=c;d:e,f",
        "line\ntwo",
        "tab\there",
        "two  spaces",
        "é ñ ü",
        "日本",
        "\"quoted\"",
        "<x>",
        "a & b",
        "back\\slash",
        "  edges  ",
        "http://x.y/z",
        "~!@#$%^*()_+-[]{}'`",
    ];
    for text in tries {
        match write_description(&writer, &uri.0, text).await {
            Err(_) => println!("{text:?}: refused"),
            Ok(()) => {
                let back = read_description(&reader, &uri.1).await?;
                if back == text {
                    println!("{text:?}: kept");
                } else {
                    println!("{text:?}: read back {back:?}");
                }
            }
        }
    }

    let text = format!("timed {}", now_ms());
    let began = std::time::Instant::now();
    write_description(&writer, &uri.0, &text).await?;
    let written = began.elapsed();
    loop {
        if read_description(&reader, &uri.1).await? == text {
            println!(
                "open session saw the change {:?} after it was written",
                began.elapsed() - written
            );
            break;
        }
        if began.elapsed().as_secs() > 60 {
            println!("open session didn't see the change within 60 s");
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    Ok(())
}

async fn push() -> Result<()> {
    use futures_util::StreamExt;
    use librespot_core::dealer::protocol::Message as DealerMessage;

    let writer = session().await?;
    let liked = writer
        .spclient()
        .get_collection_page("collection", "", 1)
        .await?;
    let song = liked
        .items
        .first()
        .context("needs a Liked Song")?
        .uri
        .clone();
    let uri = playlists::create(&writer, NAME, true, &[]).await?;
    println!("made {uri}");

    let paths = Paths::new()?;
    let cache = Cache::new(Some(paths.config.as_path()), None, None, None)?;
    let credentials = cache.credentials().context("not signed in")?;
    let listener = Session::new(SessionConfig::default(), Some(cache));
    let summary = |message: DealerMessage| -> Result<String, librespot_core::Error> {
        let mut headers: Vec<String> = message.headers.keys().cloned().collect();
        headers.sort();
        Ok(format!("{} headers {headers:?}", message.uri))
    };
    let modification = |message: DealerMessage| -> Result<String, librespot_core::Error> {
        let uri = message.uri.clone();
        let info = DealerMessage::from_raw::<PlaylistModificationInfo>(message)?;
        let ops: Vec<String> = info
            .ops
            .iter()
            .map(|op| {
                format!(
                    "{:?} {}",
                    op.kind(),
                    protobuf::text_format::print_to_string(op)
                )
            })
            .collect();
        Ok(format!(
            "{uri} revision {} (from {}) ops {ops:?}",
            hex(info.new_revision()),
            hex(info.parent_revision())
        ))
    };
    let mut playlist = listener
        .dealer()
        .listen_for("hm://playlist", modification)?;
    let mut collection = listener.dealer().listen_for("hm://collection", summary)?;
    let mut pusher = listener.dealer().listen_for("hm://pusher", summary)?;
    listener.connect(credentials, true).await?;
    listener.dealer().start().await?;
    println!("listener's dealer started");

    let began = std::time::Instant::now();
    let mut steps = 0;
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(15));
    tick.tick().await;
    loop {
        let t = began.elapsed().as_secs_f32();
        tokio::select! {
            Some(m) = playlist.next() => println!("{t:6.2}s playlist: {}", m?),
            Some(m) = collection.next() => println!("{t:6.2}s collection: {}", m?),
            Some(m) = pusher.next() => println!("{t:6.2}s pusher: {}", m?),
            _ = tick.tick() => {
                steps += 1;
                match steps {
                    1 => {
                        write_description(&writer, &uri, "pushed?").await?;
                        println!("{t:6.2}s wrote the description");
                    }
                    2 => {
                        playlists::add(&writer, &uri, std::slice::from_ref(&song)).await?;
                        println!("{t:6.2}s added a song");
                    }
                    _ => break,
                }
            }
        }
    }
    Ok(())
}

fn playlist_id_of(uri: String) -> Result<(String, SpotifyId)> {
    let id = playlist_id(&uri)?;
    Ok((uri, id))
}

async fn write_description(session: &Session, uri: &str, text: &str) -> Result<()> {
    let attributes = ListAttributes {
        description: Some(text.to_string()),
        ..Default::default()
    };
    change(session, uri, attributes).await
}

async fn read_description(session: &Session, id: &SpotifyId) -> Result<String> {
    let bytes = session.spclient().get_playlist(id).await?;
    let list = SelectedListContent::parse_from_bytes(&bytes)?;
    Ok(list.attributes.description().to_string())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

async fn clean() -> Result<()> {
    let session = session().await?;
    for playlist in spotless_core::spotify::catalog::playlists(&session).await? {
        if playlist.name == NAME
            || playlist.name == SECOND
            || playlist.name.starts_with("Spotless e2e")
        {
            playlists::unlist(&session, &playlist.uri).await?;
            println!("took out {}", playlist.uri);
        }
    }
    if synced::remove_empty_folder(&session).await? {
        println!("took out the empty Spotless folder");
    }
    Ok(())
}

const SECOND: &str = "Spotless test 2";

async fn synced_side() -> Result<()> {
    let s = session().await?;
    let liked = s
        .spclient()
        .get_collection_page("collection", "", 4)
        .await?;
    let liked: Vec<String> = liked.items.iter().map(|i| i.uri.clone()).collect();
    if liked.len() < 4 {
        bail!("needs four Liked Songs");
    }
    let short = |uri: &str| uri.rsplit(':').next().unwrap_or_default()[..6].to_string();
    let names = |remote: &synced::Remote| -> Vec<String> {
        remote.songs.iter().map(|(_, uri)| short(uri)).collect()
    };
    let (a, b, c, d) = (
        short(&liked[0]),
        short(&liked[1]),
        short(&liked[2]),
        short(&liked[3]),
    );
    println!("songs a={a} b={b} c={c} d={d}");

    let uri = synced::create(&s, NAME, &liked[..3]).await?;
    println!("made {uri}");
    let folder = synced::folder(&s).await?;
    println!("folder: {folder:?}");
    check(
        "first is in the folder",
        folder.iter().any(|(u, _)| *u == uri),
    );

    let remote = synced::read(&s, &uri).await?;
    println!(
        "read {:?}, ids {:?}",
        names(&remote),
        remote.songs.iter().map(|(id, _)| &id.0).collect::<Vec<_>>()
    );
    check(
        "three songs, a b c",
        names(&remote) == [a.as_str(), b.as_str(), c.as_str()],
    );
    check(
        "each has an ID",
        remote.songs.iter().all(|(id, _)| !id.0.is_empty()),
    );

    let revision = synced::edit(
        &s,
        &uri,
        remote.revision.clone(),
        &[Edit::Move {
            from: 0,
            to: 2,
            song: liked[0].clone(),
        }],
    )
    .await?;
    let now = synced::read(&s, &uri).await?;
    println!("moved first to last: {:?}", names(&now));
    check("b c a", names(&now) == [b.as_str(), c.as_str(), a.as_str()]);
    check(
        "the edit answered the revision read",
        now.revision == revision,
    );
    check(
        "IDs follow their songs",
        now.songs[2].0 == remote.songs[0].0,
    );

    synced::edit(
        &s,
        &uri,
        now.revision.clone(),
        &[Edit::Move {
            from: 2,
            to: 0,
            song: liked[0].clone(),
        }],
    )
    .await?;
    let now = synced::read(&s, &uri).await?;
    println!("moved last to first: {:?}", names(&now));
    check("a b c", names(&now) == [a.as_str(), b.as_str(), c.as_str()]);

    // Moved on, so that b isn't where the old revision had it.
    let moved = synced::read(&s, &uri).await?;
    synced::edit(
        &s,
        &uri,
        moved.revision.clone(),
        &[Edit::Move {
            from: 1,
            to: 2,
            song: liked[1].clone(),
        }],
    )
    .await?;
    let stale = synced::edit(
        &s,
        &uri,
        moved.revision.clone(),
        &[Edit::Remove {
            at: 1,
            songs: vec![liked[1].clone()],
        }],
    )
    .await;
    println!(
        "b removed on a stale revision: {:?}",
        stale.as_ref().map(|_| ()).map_err(|e| format!("{e:#}"))
    );
    let after = synced::read(&s, &uri).await?;
    println!("  now {:?}", names(&after));
    check(
        "refused, or b removed and nothing else",
        if stale.is_err() {
            names(&after) == [a.as_str(), c.as_str(), b.as_str()]
        } else {
            names(&after) == [a.as_str(), c.as_str()]
        },
    );
    // Back to a b c for what follows.
    let after = if stale.is_ok() {
        synced::edit(
            &s,
            &uri,
            after.revision.clone(),
            &[Edit::Add {
                at: 1,
                songs: vec![liked[1].clone()],
            }],
        )
        .await?;
        synced::read(&s, &uri).await?
    } else {
        synced::edit(
            &s,
            &uri,
            after.revision.clone(),
            &[Edit::Move {
                from: 2,
                to: 1,
                song: liked[1].clone(),
            }],
        )
        .await?;
        synced::read(&s, &uri).await?
    };
    check(
        "a b c again",
        names(&after) == [a.as_str(), b.as_str(), c.as_str()],
    );

    synced::edit(
        &s,
        &uri,
        after.revision.clone(),
        &[
            Edit::Remove {
                at: 0,
                songs: vec![liked[0].clone()],
            },
            Edit::Add {
                at: 1,
                songs: vec![liked[3].clone()],
            },
            Edit::Describe("s1:00:0:0:".into()),
        ],
    )
    .await?;
    let now = synced::read(&s, &uri).await?;
    println!(
        "removed first, added d second, described: {:?} {:?}",
        names(&now),
        now.description
    );
    check("b d c", names(&now) == [b.as_str(), d.as_str(), c.as_str()]);
    check("description set", now.description == "s1:00:0:0:");

    let second = synced::create(&s, SECOND, &[]).await?;
    let folder = synced::folder(&s).await?;
    println!("folder: {folder:?}");
    check(
        "second goes last in the same folder",
        folder.iter().map(|(u, _)| u.as_str()).collect::<Vec<_>>()
            == [uri.as_str(), second.as_str()],
    );
    Ok(())
}

fn check(what: &str, ok: bool) {
    println!("  {} {what}", if ok { "OK  " } else { "FAIL" });
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |t| t.as_millis() as i64)
}

fn playlist_id(uri: &str) -> Result<SpotifyId> {
    match SpotifyUri::from_uri(uri) {
        Ok(SpotifyUri::Playlist { id, .. }) => Ok(id),
        _ => bail!("not a Spotify playlist: {uri}"),
    }
}
