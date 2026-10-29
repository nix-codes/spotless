//! Do synced playlists sync? Two Spotless controllers, as two devices of
//! the account, on folders of their own: A syncs a new "Spotless e2e"
//! playlist, B finds it in the Spotless folder as it signs in, B removes
//! a song, A marks one Q and renames it, each followed by the other, and B
//! deletes it from Browse, which A, signing in again, keeps unsynced. Each
//! step's time is printed. Nothing plays; the playlist is gone after (the
//! Spotless folder stays: `sync_probe clean` takes it out if empty).
//!
//!   SPOTLESS_AUDIO_DEVICE=nope cargo run -p spotless-core --example sync_devices
//!
//! Sign in through the app first: this reuses its saved credential.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use spotless_core::action::Action;
use spotless_core::backend::SessionState;
use spotless_core::controller::{Controller, SyncStatus};
use spotless_core::model::{AlbumRef, PlaylistId, TrackRef};
use spotless_core::paths::Paths;

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

fn ready(c: &Controller) -> bool {
    matches!(c.state().session, SessionState::Ready { .. })
}

fn synced_list(c: &Controller) -> Option<PlaylistId> {
    c.state()
        .library
        .playlists()
        .iter()
        .find(|l| l.name == NAME && l.sync.is_some())
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
    let uris = [
        "spotify:track:4uLU6hMCjMI75M1A2tKUQC",
        "spotify:track:3n3Ppam7vgaVa1iaRUc9Lp",
        "spotify:track:7qiZfU4dY1lWllzX7mPBI3",
    ];
    let tracks: Vec<TrackRef> = uris
        .iter()
        .enumerate()
        .map(|(n, uri)| TrackRef {
            uri: uri.to_string(),
            title: format!("A's song {n}"),
            artists: vec![],
            album: AlbumRef {
                uri: String::new(),
                name: String::new(),
            },
            duration_ms: 200_000,
            source: None,
        })
        .collect();
    a.apply(Action::NewPlaylistWith(tracks));
    let list_a = a.state().viewed;
    a.apply(Action::RenamePlaylist(list_a, NAME.into()));
    a.apply(Action::SetSync(list_a, true));
    until(&mut a, &mut b, "A synced, songs with IDs", 60, |a, _| {
        up_to_date(a, list_a)
            && a.state()
                .library
                .get(list_a)
                .is_some_and(|l| l.entries().iter().all(|e| e.item.is_some()))
    })?;
    let uri = a
        .state()
        .library
        .get(list_a)
        .unwrap()
        .sync
        .clone()
        .unwrap()
        .uri;
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
    b.apply(Action::Remove(list_b, vec![second]));
    until(&mut a, &mut b, "A sees B's removal", 30, |a, _| {
        titles(a, list_a).len() == 2
    })?;
    println!("        A has {:?}", titles(&a, list_a));

    // A marks its last song Q; B follows (A isn't playing: it's written).
    let last = a.state().library.get(list_a).unwrap().entries()[1].id;
    a.apply(Action::SelectPlaylist(list_a));
    a.apply(Action::ToggleMarks(vec![last]));
    until(&mut a, &mut b, "B sees A's Q mark", 30, |_, b| {
        let list = b.state().library.get(list_b).unwrap();
        list.play_next().len() == 1 && list.mark_number(list.entries()[1].id) == Some(1)
    })?;

    // A renames it; B follows.
    a.apply(Action::RenamePlaylist(list_a, format!("{NAME} 2")));
    until(&mut a, &mut b, "B sees A's rename", 30, |_, b| {
        b.state()
            .library
            .get(list_b)
            .is_some_and(|l| l.name == format!("{NAME} 2"))
    })?;

    // B deletes it from Browse, which closes its tab; A, signing in
    // again, keeps it unsynced.
    b.apply(Action::ReadSpotifyPlaylists);
    until(
        &mut a,
        &mut b,
        "B read its Spotify playlists",
        30,
        |_, b| b.state().browse.unlistable(&uri),
    )?;
    b.apply(Action::UnlistSpotifyPlaylist(uri.clone()));
    until(&mut a, &mut b, "B deleted it", 10, |_, b| {
        b.state().library.get(list_b).is_none()
    })?;
    std::thread::sleep(Duration::from_secs(3));
    drop(a);
    let config = root.join("a").join("config");
    let mut a = Controller::new(
        Paths::from_dirs(config, root.join("a").join("cache"))?,
        || {},
    );
    until(&mut a, &mut b, "A unsynced it, kept here", 60, |a, _| {
        a.state()
            .library
            .playlists()
            .iter()
            .any(|l| l.name.starts_with(NAME) && l.sync.is_none())
    })?;
    println!("        A: {:?}", a.state().notice);
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}
