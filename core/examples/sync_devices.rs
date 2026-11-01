//! Do synced playlists sync? Two Spotless controllers, as two devices of
//! the account, on folders of their own: A syncs a new "Spotless e2e"
//! playlist, B finds it in the Spotless folder as it signs in, B removes
//! a song, A renames it, each followed by the other; closed, B misses a
//! second rename, and opened again has it once, by its new name; then B
//! stops syncing it, which deletes it from Spotify: B keeps it, and A, open
//! all along, keeps it unsynced. Then A syncs another, and B, syncing one
//! by the same name, is asked, and replaces it. Each step's time is
//! printed. Nothing plays; the playlist is gone after (the
//! Spotless folder stays: `sync_probe clean` takes it out if empty).
//!
//!   SPOTLESS_AUDIO_DEVICE=nope cargo run -p spotless-core --features probes --example sync_devices
//!
//! Sign in through the app first: this reuses its saved credential.
//! Close Spotless on every device first: one open while this runs opens
//! the "Spotless e2e" playlists as tabs, and keeps them once deleted.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use spotless_core::action::{PlaylistsAction, SpotifyListsAction, SyncAction};
use spotless_core::model::{AlbumRef, PlaylistId, TrackRef};
use spotless_core::session::SessionState;
use spotless_core::{Controller, Paths, SyncStatus};

const NAME: &str = "Spotless e2e";

fn device(root: &Path, name: &str) -> Result<Controller> {
    let config = root.join(name).join("config");
    std::fs::create_dir_all(&config)?;
    let real = Paths::new()?;
    std::fs::copy(real.credentials(), config.join("credentials.json"))?;
    std::fs::write(
        config.join("settings.json"),
        r#"{"spotify_tab_enabled": false}"#,
    )?;
    let paths = Paths::from_dirs(config, root.join(name).join("cache"))?;
    Ok(Controller::new(paths, || {}))
}

fn run(c: &mut Controller) {
    c.receive();
    c.tick();
}

/// Runs both until `done` holds, or fails after `most`.
fn until(
    a: &mut Controller,
    b: &mut Controller,
    what: &str,
    most: u64,
    done: impl Fn(&Controller, &Controller) -> bool,
) -> Result<()> {
    let began = Instant::now();
    while !done(a, b) {
        if began.elapsed() > Duration::from_secs(most) {
            bail!("timed out: {what}");
        }
        run(a);
        run(b);
        std::thread::sleep(Duration::from_millis(50));
    }
    println!("{:5.1}s  {what}", began.elapsed().as_secs_f32());
    Ok(())
}

const URIS: [&str; 3] = [
    "spotify:track:4uLU6hMCjMI75M1A2tKUQC",
    "spotify:track:3n3Ppam7vgaVa1iaRUc9Lp",
    "spotify:track:7qiZfU4dY1lWllzX7mPBI3",
];

/// One of [`URIS`], with a made-up title.
fn track(n: usize) -> TrackRef {
    TrackRef {
        uri: URIS[n].to_string(),
        title: format!("A's song {n}"),
        artists: vec![],
        album: AlbumRef {
            uri: String::new(),
            name: String::new(),
        },
        duration_ms: 200_000,
        source: None,
    }
}

fn ready(c: &Controller) -> bool {
    matches!(c.state().session, SessionState::Ready { .. })
}

fn synced_list(c: &Controller) -> Option<PlaylistId> {
    c.state()
        .library
        .playlists()
        .iter()
        .find(|l| l.name == NAME && l.synced())
        .map(|l| l.id)
}

fn titles(c: &Controller, id: PlaylistId) -> Vec<String> {
    c.state()
        .library
        .get(id)
        .map(|l| l.entries().iter().map(|e| e.track.title.clone()).collect())
        .unwrap_or_default()
}

fn up_to_date(c: &Controller, id: PlaylistId) -> bool {
    c.state().sync.get(&id) == Some(&SyncStatus::UpToDate)
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let root = std::env::temp_dir().join(format!("spotless-e2e-{}", std::process::id()));
    let mut a = device(&root, "a")?;
    let mut b = device(&root, "b")?;
    until(&mut a, &mut b, "both signed in", 60, |a, b| {
        ready(a) && ready(b)
    })?;

    // A makes a playlist of three songs (titles filled in by A only) and
    // syncs it.
    let tracks: Vec<TrackRef> = (0..URIS.len()).map(track).collect();
    a.apply(PlaylistsAction::NewPlaylistWith(tracks));
    let list_a = a.state().viewed;
    a.apply(PlaylistsAction::RenamePlaylist(list_a, NAME.into()));
    a.apply(SyncAction::SetSync(list_a, true));
    until(&mut a, &mut b, "A synced, songs with IDs", 60, |a, _| {
        up_to_date(a, list_a) && a.state().library.get(list_a).is_some_and(|l| l.all_sent())
    })?;
    let uri = a
        .state()
        .library
        .get(list_a)
        .and_then(|l| l.synced_uri())
        .unwrap()
        .to_string();
    println!("        on Spotify as {uri}");

    // B, "started" now, finds it in the folder: dropped and made again so
    // it reads the folder on signing in.
    drop(b);
    let mut b = device(&root, "b2")?;
    until(
        &mut a,
        &mut b,
        "B opened it, with Spotify's titles",
        60,
        |_, b| {
            synced_list(b)
                .is_some_and(|id| titles(b, id).len() == 3 && !titles(b, id)[0].starts_with("A's"))
        },
    )?;
    let list_b = synced_list(&b).unwrap();
    println!("        B has {:?}", titles(&b, list_b));

    // B removes the second song; A follows.
    let second = b.state().library.get(list_b).unwrap().entries()[1].id;
    b.apply(PlaylistsAction::Remove(list_b, vec![second]));
    until(&mut a, &mut b, "A sees B's removal", 30, |a, _| {
        titles(a, list_a).len() == 2
    })?;
    println!("        A has {:?}", titles(&a, list_a));

    // A renames it; B follows.
    a.apply(PlaylistsAction::RenamePlaylist(list_a, format!("{NAME} 2")));
    until(&mut a, &mut b, "B sees A's rename", 30, |_, b| {
        b.state()
            .library
            .get(list_b)
            .is_some_and(|l| l.name == format!("{NAME} 2"))
    })?;

    // B is closed while A renames it again: opened, B has it once, by
    // the new name, and no tab by the old one.
    b.save();
    b.wait_for_syncs(Duration::from_secs(5));
    drop(b);
    let renamed = format!("{NAME} 3");
    a.apply(PlaylistsAction::RenamePlaylist(list_a, renamed.clone()));
    a.save();
    let began = Instant::now();
    while !up_to_date(&a, list_a) {
        if began.elapsed() > Duration::from_secs(30) {
            bail!("timed out: A sent the rename");
        }
        run(&mut a);
        std::thread::sleep(Duration::from_millis(50));
    }
    println!("{:5.1}s  A sent the rename", began.elapsed().as_secs_f32());
    let mut b = device(&root, "b2")?;
    until(
        &mut a,
        &mut b,
        "B, opened again, has it once, renamed",
        60,
        |_, b| {
            up_to_date(b, list_b)
                && b.state()
                    .library
                    .get(list_b)
                    .is_some_and(|l| l.name == renamed)
        },
    )?;
    let tabs: Vec<(String, bool)> = (b.state().library.playlists().iter())
        .map(|l| (l.name.clone(), l.synced()))
        .collect();
    println!("        B's tabs {tabs:?}");
    if tabs
        .iter()
        .filter(|(name, _)| name.starts_with(NAME))
        .count()
        != 1
    {
        bail!("B has it more than once");
    }

    // B stops syncing it: it's deleted from Spotify, B keeps it, and A,
    // open all along, keeps it too, unsynced, as Spotify tells it the
    // list of playlists changed.
    b.apply(SyncAction::SetSync(list_b, false));
    until(
        &mut a,
        &mut b,
        "A unsynced it, kept here, still open",
        30,
        |a, _| {
            a.state()
                .library
                .get(list_a)
                .is_some_and(|l| !l.synced() && l.len() == 2)
        },
    )?;
    let kept = b.state().library.get(list_b);
    if !kept.is_some_and(|l| !l.synced() && l.len() == 2) {
        bail!("B didn't keep it");
    }
    println!("        A: {:?}", a.state().notice);
    println!("        B: {:?}", b.state().notice);
    b.apply(SpotifyListsAction::ReadSpotifyPlaylists);
    until(
        &mut a,
        &mut b,
        "gone from B's Spotify playlists",
        30,
        |_, b| {
            matches!(&b.state().browse.spotify_playlists,
            Some(spotless_core::browse::Loadable::Ready(lists)) if lists.iter().all(|l| l.uri != uri))
        },
    )?;

    // A syncs another; B, syncing one by the same name, is asked, then
    // replaces it: A, open, keeps its own unsynced.
    let clash = format!("{NAME} clash");
    a.apply(PlaylistsAction::NewPlaylistWith(vec![track(0)]));
    let first = a.state().viewed;
    a.apply(PlaylistsAction::RenamePlaylist(first, clash.clone()));
    a.apply(SyncAction::SetSync(first, true));
    until(&mut a, &mut b, "A synced another", 60, |a, _| {
        up_to_date(a, first)
    })?;
    b.apply(PlaylistsAction::NewPlaylistWith(vec![track(1), track(2)]));
    let second = b.state().viewed;
    b.apply(PlaylistsAction::RenamePlaylist(
        second,
        clash.to_uppercase(),
    ));
    b.apply(SyncAction::SetSync(second, true));
    until(&mut a, &mut b, "B asked: the name is taken", 30, |_, b| {
        b.state().sync_name_taken == Some(second)
    })?;
    b.apply(SyncAction::ReplaceSynced(second));
    until(
        &mut a,
        &mut b,
        "B replaced it; A keeps its own, unsynced",
        60,
        |a, b| {
            up_to_date(b, second)
                && a.state()
                    .library
                    .get(first)
                    .is_some_and(|l| !l.synced() && l.len() == 1)
        },
    )?;
    println!("        A: {:?}", a.state().notice);
    b.apply(SyncAction::SetSync(second, false));
    until(&mut a, &mut b, "B stopped syncing it: gone", 30, |_, b| {
        b.state()
            .notice
            .as_deref()
            .is_some_and(|n| n.contains("gone from Spotify"))
    })?;
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}
