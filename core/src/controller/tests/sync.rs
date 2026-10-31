//! Synced playlists.

use super::*;
use crate::model::sync::{ItemId, Position};
use crate::spotify::synced::{Changed, Done, Job, Remote};

const URI: &str = "spotify:playlist:gym";

fn item(n: u64) -> ItemId {
    ItemId(format!("{n:04x}"))
}

fn ready(rig: &mut Rig) {
    rig.tell(Event::Session(SessionState::Ready { user: "me".into() }));
}

fn jobs(rig: &mut Rig) -> Vec<Job> {
    rig.sent()
        .into_iter()
        .filter_map(|command| match command {
            Command::Sync { job, .. } => Some(job),
            _ => None,
        })
        .collect()
}

/// Spotify's answer: these songs (by number: item and track
/// alike), the job's songs given their items by track.
fn done(job: &Job, songs: &[u64], position: Option<Position>) -> Done {
    let sent = job
        .local
        .iter()
        .map(|(id, item, uri)| {
            let n: u64 = uri.rsplit(':').next().unwrap().parse().unwrap();
            (*id, item.clone().or(Some(item_n(n, songs))))
        })
        .collect();
    Done {
        remote: Remote {
            revision: songs.iter().map(|&n| n as u8).collect(),
            name: "Gym".into(),
            description: position.as_ref().map(Position::encode).unwrap_or_default(),
            songs: songs
                .iter()
                .map(|&n| (item(n), format!("spotify:track:{n}")))
                .collect(),
        },
        sent,
        tracks: Vec::new(),
        position,
    }
}

fn item_n(n: u64, _songs: &[u64]) -> ItemId {
    item(n)
}

/// A playlist "Gym" of songs 1 and 2, synced and sent.
fn synced_gym(rig: &mut Rig) -> PlaylistId {
    synced_gym_named(rig, URI)
}

/// The same, as this Spotify playlist.
fn synced_gym_named(rig: &mut Rig, uri: &str) -> PlaylistId {
    ready(rig);
    rig.controller
        .apply(Action::NewPlaylistWith(tracks([1, 2])));
    let list = rig.controller.state().viewed;
    rig.controller
        .apply(Action::RenamePlaylist(list, "Gym".into()));
    rig.sent();
    rig.controller.apply(Action::SetSync(list, true));
    assert!(rig.sent().iter().any(
        |c| matches!(c, Command::CreateSynced { list: l, name, replace: false } if *l == list && name == "Gym")
    ));
    assert_eq!(
        rig.controller.state().sync.get(&list),
        Some(&SyncStatus::Waiting)
    );
    rig.tell(Event::SyncCreated {
        list,
        result: Ok(uri.into()),
    });
    let job = jobs(rig).pop().expect("sent whole");
    assert_eq!(job.uri, uri);
    assert!(job.base.is_empty());
    assert!(job.local.iter().all(|(_, item, _)| item.is_none()));
    // Not yet: its song isn't on Spotify.
    assert_eq!(job.position, None);
    rig.tell(Event::Synced {
        list,
        result: Ok(done(&job, &[1, 2], None)),
    });
    // Now that its songs have IDs, where it's up to follows.
    let job = jobs(rig).pop().expect("the position");
    let position = job.position.clone().expect("a position");
    assert_eq!(position.song, Some(item(1)));
    rig.tell(Event::Synced {
        list,
        result: Ok(done(&job, &[1, 2], Some(position))),
    });
    list
}

fn titles(rig: &Rig, list: PlaylistId) -> Vec<String> {
    super::titles(rig, list)
}

#[test]
fn a_playlist_made_synced_is_sent_whole_then_its_position() {
    let mut rig = Rig::new("sync-whole");
    let list = synced_gym(&mut rig);
    let state = rig.controller.state();
    let gym = state.library.get(list).unwrap();
    let items: Vec<_> = gym.entries().iter().map(|e| e.item.clone()).collect();
    assert_eq!(items, [Some(item(1)), Some(item(2))]);
    assert_eq!(state.sync.get(&list), Some(&SyncStatus::UpToDate));
    assert!(state.library.synced_seen.contains(&URI.to_string()));
    // Unchanged, it isn't sent again.
    rig.controller.save();
    assert!(jobs(&mut rig).is_empty());
}

#[test]
fn songs_from_another_device_arrive_with_its_newer_position() {
    let mut rig = Rig::new("sync-arrive");
    let list = synced_gym(&mut rig);
    jobs(&mut rig);
    rig.tell(Event::PlaylistChanged(Changed {
        uri: URI.into(),
        revision: vec![9],
    }));
    let job = jobs(&mut rig).pop().expect("read again");
    let theirs = Position {
        song: Some(item(3)),
        resume_s: 30,
        saved_at: u64::MAX / 2,
        marks: vec![item(2)],
    };
    let mut answer = done(&job, &[1, 3, 2], Some(theirs));
    answer.tracks = vec![(item(3), Some(entry(3).track))];
    rig.tell(Event::Synced {
        list,
        result: Ok(answer),
    });
    assert_eq!(titles(&rig, list), ["Song 1", "Song 3", "Song 2"]);
    let gym = rig.controller.state().library.get(list).unwrap();
    let cursor = gym.get(gym.cursor().unwrap()).unwrap();
    assert_eq!(cursor.track.title, "Song 3");
    // Stopped, not paused where it was then.
    assert_eq!(gym.resume_ms, 0);
    // Q marks don't sync.
    assert_eq!(gym.mark_number(gym.entries()[2].id), None);
    // Its own change, told back, isn't read again.
    let revision = vec![1, 3, 2];
    rig.tell(Event::PlaylistChanged(Changed {
        uri: URI.into(),
        revision,
    }));
    assert!(jobs(&mut rig).iter().all(|job| job.position.is_some()));
}

#[test]
fn edits_made_offline_wait_and_go_once_connected() {
    let mut rig = Rig::new("sync-offline");
    let list = synced_gym(&mut rig);
    jobs(&mut rig);
    rig.tell(Event::Session(SessionState::Connecting));
    let first = rig.controller.state().library.get(list).unwrap().entries()[0].id;
    rig.controller.apply(Action::Remove(list, vec![first]));
    rig.controller.save();
    assert!(jobs(&mut rig).is_empty());
    assert_eq!(
        rig.controller.state().sync.get(&list),
        Some(&SyncStatus::Waiting)
    );
    ready(&mut rig);
    assert!(rig.sent().iter().any(|c| matches!(c, Command::SyncFolder)));
    rig.tell(Event::SyncFolder(Ok(vec![(URI.into(), "Gym".into())])));
    let job = jobs(&mut rig).pop().expect("sent once connected");
    assert_eq!(job.base, [item(1), item(2)]);
    assert_eq!(job.local.len(), 1);
    rig.tell(Event::Synced {
        list,
        result: Ok(done(&job, &[2], None)),
    });
    assert_eq!(titles(&rig, list), ["Song 2"]);
    assert_eq!(
        rig.controller.state().sync.get(&list),
        Some(&SyncStatus::UpToDate)
    );
}

#[test]
fn the_folder_opens_new_playlists_and_unsyncs_deleted_ones() {
    let mut rig = Rig::new("sync-folder");
    let list = synced_gym(&mut rig);
    // Made synced a while ago, as far as the folder goes.
    rig.controller.syncing.get_mut(&list).unwrap().created = None;
    rig.sent();
    rig.tell(Event::SyncFolder(Ok(vec![(
        "spotify:playlist:road".into(),
        "Road trip".into(),
    )])));
    let state = rig.controller.state();
    assert!(state.library.get(list).unwrap().sync.is_none());
    assert!(
        state
            .notice
            .as_deref()
            .unwrap()
            .starts_with("Gym was taken off Spotify")
    );
    let road = state
        .library
        .synced("spotify:playlist:road")
        .expect("opened");
    assert_eq!(road.name, "Road trip");
    let road = road.id;
    // Read, with no position written before it has read Spotify's.
    let road_jobs: Vec<Job> = jobs(&mut rig)
        .into_iter()
        .filter(|job| job.uri == "spotify:playlist:road")
        .collect();
    assert_eq!(road_jobs.len(), 1);
    assert_eq!(road_jobs[0].position, None);

    // Closed here, it doesn't open again.
    rig.controller.apply(Action::ClosePlaylist(road));
    rig.tell(Event::SyncFolder(Ok(vec![(
        "spotify:playlist:road".into(),
        "Road trip".into(),
    )])));
    assert!(
        rig.controller
            .state()
            .library
            .synced("spotify:playlist:road")
            .is_none()
    );
}

#[test]
fn renamed_here_or_there_the_name_follows() {
    let mut rig = Rig::new("sync-rename");
    let list = synced_gym(&mut rig);
    jobs(&mut rig);
    rig.controller
        .apply(Action::RenamePlaylist(list, "Gym days".into()));
    rig.controller.save();
    let job = jobs(&mut rig).pop().expect("renamed");
    assert_eq!(job.rename.as_deref(), Some("Gym days"));
    let mut answer = done(&job, &[1, 2], None);
    answer.remote.name = "Gym days".into();
    rig.tell(Event::Synced {
        list,
        result: Ok(answer),
    });
    rig.tell(Event::PlaylistChanged(Changed {
        uri: URI.into(),
        revision: vec![7],
    }));
    let job = jobs(&mut rig).pop().expect("read again");
    assert_eq!(job.rename, None);
    let mut answer = done(&job, &[1, 2], None);
    answer.remote.name = "Leg day".into();
    rig.tell(Event::Synced {
        list,
        result: Ok(answer),
    });
    assert_eq!(
        rig.controller.state().library.get(list).unwrap().name,
        "Leg day"
    );
}

#[test]
fn a_name_taken_in_the_folder_is_asked_about_then_replaced() {
    let mut rig = Rig::new("sync-taken");
    ready(&mut rig);
    rig.controller
        .apply(Action::NewPlaylistWith(tracks([1, 2])));
    let list = rig.controller.state().viewed;
    rig.controller
        .apply(Action::RenamePlaylist(list, "Gym".into()));
    rig.controller.apply(Action::SetSync(list, true));
    rig.sent();
    rig.tell(Event::SyncNameTaken { list });
    let state = rig.controller.state();
    assert_eq!(state.sync_name_taken, Some(list));
    assert!(state.library.get(list).unwrap().sync.is_none());
    assert_eq!(state.sync.get(&list), None);
    // Renamed, it's synced by its new name.
    rig.controller
        .apply(Action::RenamePlaylist(list, "Leg day".into()));
    assert_eq!(rig.controller.state().sync_name_taken, None);
    assert!(rig.sent().iter().any(|c| matches!(
        c,
        Command::CreateSynced { list: l, name, replace: false } if *l == list && name == "Leg day"
    )));
    rig.tell(Event::SyncNameTaken { list });
    // Dismissed, nothing is sent; replaced, it's made anew.
    rig.controller.apply(Action::DismissSyncNameTaken);
    assert_eq!(rig.controller.state().sync_name_taken, None);
    assert!(rig.sent().is_empty());
    rig.tell(Event::SyncNameTaken { list });
    rig.controller.apply(Action::ReplaceSynced(list));
    assert_eq!(rig.controller.state().sync_name_taken, None);
    assert!(rig.sent().iter().any(|c| matches!(
        c,
        Command::CreateSynced { list: l, replace: true, .. } if *l == list
    )));
}

#[test]
fn a_song_playing_is_written_once_it_has_played_a_few_seconds() {
    let mut rig = Rig::new("sync-settle");
    let list = synced_gym(&mut rig);
    jobs(&mut rig);
    let second = rig.controller.state().library.get(list).unwrap().entries()[1].id;
    // Still loading as it saves, it waits all the same.
    rig.controller.apply(Action::PlayEntry(list, second));
    assert_eq!(rig.controller.state().player.status, Status::Loading);
    rig.controller.unsaved = Some(Instant::now() - SAVE_DELAY);
    assert!(
        rig.controller
            .tick()
            .is_some_and(|wait| wait <= crate::controller::sync::SETTLE)
    );
    assert!(jobs(&mut rig).is_empty(), "not as it starts");
    rig.tell(Event::Player(PlayerSnapshot {
        status: Status::Playing,
        ..PlayerSnapshot::default()
    }));
    let syncing = rig.controller.syncing.get_mut(&list).unwrap();
    syncing.moved.as_mut().unwrap().1 -= crate::controller::sync::SETTLE;
    rig.controller.tick();
    let job = jobs(&mut rig).pop().expect("written once settled");
    assert_eq!(job.position.unwrap().song, Some(item(2)));
    assert!(rig.controller.syncing[&list].moved.is_none());
}

#[test]
fn the_same_song_and_q_marks_alone_are_not_written() {
    let mut rig = Rig::new("sync-same");
    let list = synced_gym(&mut rig);
    jobs(&mut rig);
    let entries = rig
        .controller
        .state()
        .library
        .get(list)
        .unwrap()
        .entries()
        .to_vec();
    rig.controller.apply(Action::PlayEntry(list, entries[0].id));
    rig.controller.apply(Action::Stop);
    rig.controller
        .apply(Action::ToggleMarks(vec![entries[1].id]));
    rig.tell(Event::Player(PlayerSnapshot::default()));
    rig.controller.unsaved = Some(Instant::now() - SAVE_DELAY);
    rig.controller.tick();
    assert!(jobs(&mut rig).iter().all(|job| job.position.is_none()));
}

#[test]
fn stopping_writes_the_song_at_once() {
    let mut rig = Rig::new("sync-stop");
    let list = synced_gym(&mut rig);
    jobs(&mut rig);
    let second = rig.controller.state().library.get(list).unwrap().entries()[1].id;
    rig.controller.apply(Action::PlayEntry(list, second));
    rig.controller.apply(Action::Stop);
    rig.tell(Event::Player(PlayerSnapshot::default()));
    rig.controller.unsaved = Some(Instant::now() - SAVE_DELAY);
    rig.controller.tick();
    let job = jobs(&mut rig).pop().expect("written as it stops");
    assert_eq!(job.position.unwrap().song, Some(item(2)));
}

#[test]
fn coming_back_reads_the_synced_playlists_again() {
    let mut rig = Rig::new("sync-back");
    synced_gym(&mut rig);
    rig.sent();
    rig.controller.apply(Action::NetworkBack);
    assert!(rig.sent().iter().any(|c| matches!(c, Command::SyncFolder)));
}

#[test]
fn a_change_to_the_list_of_playlists_reads_the_folder() {
    let mut rig = Rig::new("sync-list");
    ready(&mut rig);
    rig.sent();
    rig.tell(Event::PlaylistsChanged);
    assert!(rig.sent().iter().any(|c| matches!(c, Command::SyncFolder)));
    // One being made here, in the folder before Spotify answered,
    // isn't opened as another tab.
    rig.controller
        .apply(Action::NewPlaylistWith(tracks([1, 2])));
    let list = rig.controller.state().viewed;
    rig.controller.apply(Action::SetSync(list, true));
    let tabs = rig.controller.state().library.playlists().len();
    rig.tell(Event::SyncFolder(Ok(vec![(URI.into(), "Playlist".into())])));
    rig.tell(Event::SyncCreated {
        list,
        result: Ok(URI.into()),
    });
    let library = &rig.controller.state().library;
    assert_eq!(library.playlists().len(), tabs);
    assert_eq!(library.synced(URI).map(|l| l.id), Some(list));
}

#[test]
fn stopped_it_goes_from_spotify_closed_it_stays_and_deleted_it_closes() {
    let mut rig = Rig::new("sync-stop");
    let list = synced_gym(&mut rig);
    rig.sent();
    rig.controller.apply(Action::SetSync(list, false));
    let gym = rig.controller.state().library.get(list).unwrap();
    assert!(gym.sync.is_none());
    assert_eq!(gym.len(), 2);
    assert!(gym.entries().iter().all(|e| e.item.is_none()));
    // Stopped, its Spotify playlist is deleted, and it stays here,
    // with nothing for UNDO to put back.
    let notice = rig
        .sent()
        .into_iter()
        .find_map(|c| match c {
            Command::UnlistSpotifyPlaylist {
                playlist, notice, ..
            } if playlist == URI => Some(notice),
            _ => None,
        })
        .expect("deleted on Spotify");
    assert_eq!(
        notice,
        "Gym isn't synced any more: it's gone from Spotify, and only here."
    );
    rig.tell(Event::SpotifyUnlisted {
        result: Ok(Unlisted {
            playlist: URI.into(),
            name: "Gym".into(),
            at: 1,
            item: vec![1],
            notice,
        }),
        undo: false,
    });
    assert!(rig.controller.state().library.get(list).is_some());
    assert_eq!(rig.controller.state().browse.undo, None);

    let list = {
        rig.controller.apply(Action::SetSync(list, true));
        rig.tell(Event::SyncCreated {
            list,
            result: Ok(URI.into()),
        });
        list
    };
    rig.sent();
    // Closed here, it's only gone from here.
    rig.controller.apply(Action::ClosePlaylist(list));
    assert!(rig.controller.state().library.get(list).is_none());
    assert!(rig.sent().is_empty());
    // Deleted from Browse, its open tab closes too.
    let road = synced_gym_named(&mut rig, "spotify:playlist:road");
    rig.tell(Event::SpotifyPlaylists(Ok(vec![SpotifyPlaylist {
        uri: "spotify:playlist:road".into(),
        name: "Gym".into(),
        tracks: 2,
        owner: None,
        private: true,
    }])));
    rig.controller.apply(Action::UnlistSpotifyPlaylist(
        "spotify:playlist:road".into(),
    ));
    let notice = rig
        .sent()
        .into_iter()
        .find_map(|c| match c {
            Command::UnlistSpotifyPlaylist { notice, .. } => Some(notice),
            _ => None,
        })
        .expect("sent to Spotify");
    assert_eq!(notice, "Deleted “Gym” on Spotify and every device.");
    rig.tell(Event::SpotifyUnlisted {
        result: Ok(Unlisted {
            playlist: "spotify:playlist:road".into(),
            name: "Gym".into(),
            at: 1,
            item: vec![1],
            notice,
        }),
        undo: false,
    });
    let list = road;
    assert!(rig.controller.state().library.get(list).is_none());
}
