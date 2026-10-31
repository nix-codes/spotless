//! The Spotify tab and the queue.

use super::*;

#[test]
fn enter_on_a_song_to_come_plays_the_queue_here_from_it() {
    let mut rig = Rig::new("play-from");
    rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
    rig.tell(Event::Spotify(Some(SpotifyQueue {
        device: Some("Phone".into()),
        device_id: Some("phone".into()),
        status: Status::Playing,
        now: Some(entry(7).track),
        before: vec![entry(6).track],
        parts: vec![crate::spotify::queue::Part {
            title: "Next in queue".into(),
            queued: true,
            songs: vec![crate::spotify::queue::Queued {
                track: entry(8).track,
                index: 0,
            }],
        }],
        ..SpotifyQueue::default()
    })));
    rig.sent();
    let play = |rig: &mut Rig, row| {
        rig.controller.apply(PlaybackAction::PlayEntry(
            PlaylistId::SPOTIFY,
            SpotifyQueue::row_id(row),
        ));
        rig.sent()
            .into_iter()
            .filter_map(|c| match c {
                Command::Spotify(SpotifyRequest::PlayHere(song)) => Some(song),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(play(&mut rig, 0), []);
    assert_eq!(play(&mut rig, 1), [None]);
    assert_eq!(play(&mut rig, 2), [Some(entry(8).track.uri)]);
}

#[test]
fn the_spotify_tab_closes_and_opens_again_and_is_remembered() {
    let mut rig = Rig::new("spotify-tab");
    // Off at first.
    assert!(!rig.controller.state().settings.spotify_tab_shown());
    rig.controller
        .apply(SpotifyTabAction::SetSpotifyTabEnabled(true));
    rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
    rig.tell(Event::Spotify(Some(SpotifyQueue::default())));
    rig.controller
        .apply(PlaylistsAction::SelectPlaylist(PlaylistId::SPOTIFY));
    rig.sent();
    rig.controller
        .apply(PlaylistsAction::ClosePlaylist(PlaylistId::SPOTIFY));
    assert!(
        rig.sent()
            .iter()
            .any(|c| matches!(c, Command::SpotifyTab(false)))
    );
    let state = rig.controller.state();
    assert!(!state.settings.spotify_tab);
    assert_ne!(state.viewed, PlaylistId::SPOTIFY);
    rig.controller.apply(SpotifyTabAction::OpenSpotifyTab);
    assert!(
        rig.sent()
            .iter()
            .any(|c| matches!(c, Command::SpotifyTab(true)))
    );
    assert!(rig.controller.state().settings.spotify_tab);
    // Off in Settings, it leaves Connect; on again, it opens.
    rig.controller
        .apply(SpotifyTabAction::SetSpotifyTabEnabled(false));
    assert!(
        rig.sent()
            .iter()
            .any(|c| matches!(c, Command::SpotifyTab(false)))
    );
    rig.controller
        .apply(PlaylistsAction::ClosePlaylist(PlaylistId::SPOTIFY));
    rig.controller
        .apply(SpotifyTabAction::SetSpotifyTabEnabled(true));
    assert!(
        rig.sent()
            .iter()
            .any(|c| matches!(c, Command::SpotifyTab(true)))
    );
    assert!(rig.controller.state().settings.spotify_tab_shown());
}

#[test]
fn the_spotify_tab_moves_like_any_tab_and_keeps_its_place() {
    let mut rig = Rig::new("spotify-move");
    rig.controller.apply(PlaylistsAction::NewPlaylist);
    let second = rig.controller.state().viewed;
    rig.tell(Event::Spotify(Some(SpotifyQueue::default())));
    let spotify = PlaylistId::SPOTIFY;
    assert_eq!(
        rig.controller.state().tabs(),
        [spotify, PlaylistId::DEFAULT, second]
    );
    rig.controller
        .apply(PlaylistsAction::MovePlaylist(spotify, 2));
    assert_eq!(
        rig.controller.state().tabs(),
        [PlaylistId::DEFAULT, second, spotify]
    );
    rig.controller
        .apply(PlaylistsAction::MovePlaylist(second, 0));
    assert_eq!(
        rig.controller.state().tabs(),
        [second, PlaylistId::DEFAULT, spotify]
    );
    rig.controller.save();
    let paths = rig.controller.paths.clone();
    let settings = Settings::load(&paths);
    let mut again =
        Controller::with_backend(paths, settings, Arc::default(), Ok(Backend::detached().0));
    assert_eq!(again.state().spotify_at, 2);
    again.state.spotify = Some(SpotifyQueue::default());
    assert_eq!(again.state().tabs(), [second, PlaylistId::DEFAULT, spotify]);
}

#[test]
fn clear_asks_spotify_to_clear_and_ends_a_wait_for_the_queue() {
    let mut rig = Rig::new("clear");
    rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
    let elsewhere = SpotifyQueue {
        device: Some("Phone".into()),
        device_id: Some("phone".into()),
        status: Status::Paused,
        now: Some(entry(7).track),
        ..SpotifyQueue::default()
    };
    rig.tell(Event::Spotify(Some(elsewhere.clone())));
    rig.controller.apply(PlaybackAction::PlayEntry(
        PlaylistId::SPOTIFY,
        SpotifyQueue::row_id(0),
    ));
    rig.tell(Event::Spotify(Some(SpotifyQueue {
        here: true,
        ..elsewhere
    })));
    rig.sent();
    rig.controller.apply(SpotifyTabAction::ClearSpotify);
    assert!(
        rig.sent()
            .iter()
            .any(|c| matches!(c, Command::Spotify(SpotifyRequest::Clear)))
    );
    assert_eq!(rig.controller.state().spotify_starting, None);
}

#[test]
fn a_cleared_queue_is_no_tab_playing() {
    let mut rig = Rig::new("cleared");
    rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
    let here = SpotifyQueue {
        device: Some("Spotless".into()),
        device_id: Some("spotless".into()),
        here: true,
        status: Status::Playing,
        now: Some(entry(7).track),
        position_ms: 30_000,
        ..SpotifyQueue::default()
    };
    rig.tell(Event::Spotify(Some(here.clone())));
    rig.controller.apply(PlaybackAction::PlayEntry(
        PlaylistId::SPOTIFY,
        SpotifyQueue::row_id(0),
    ));
    rig.tell(Event::Spotify(Some(here.clone())));
    assert_eq!(
        rig.controller.state().playing_list(),
        Some(PlaylistId::SPOTIFY)
    );
    // Empty Track, paused, is shown as no song.
    rig.tell(Event::Spotify(Some(SpotifyQueue {
        status: Status::Paused,
        now: None,
        position_ms: 0,
        ..here
    })));
    assert_eq!(rig.controller.state().playing_list(), None);
}

#[test]
fn a_queue_held_elsewhere_only_shows_until_taken_over() {
    let mut rig = Rig::new("take-over");
    rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
    rig.tell(Event::Spotify(Some(SpotifyQueue {
        device: Some("Phone".into()),
        device_id: Some("phone".into()),
        status: Status::Playing,
        now: Some(entry(7).track),
        ..SpotifyQueue::default()
    })));
    rig.sent();
    rig.controller.apply(SpotifyTabAction::ClearSpotify);
    assert!(rig.sent().is_empty());
    rig.controller.apply(SpotifyTabAction::TakeOverSpotify);
    assert!(
        rig.sent()
            .iter()
            .any(|c| matches!(c, Command::Spotify(SpotifyRequest::TakeOver)))
    );
    // The player shows the queue from now on.
    assert!(rig.controller.state().spotify_playing());
}

#[test]
fn play_here_shows_the_queue_loading_until_it_plays_here() {
    let mut rig = Rig::new("play-here");
    rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
    let elsewhere = SpotifyQueue {
        device: Some("Phone".into()),
        device_id: Some("phone".into()),
        status: Status::Paused,
        now: Some(entry(7).track),
        ..SpotifyQueue::default()
    };
    rig.tell(Event::Spotify(Some(elsewhere.clone())));
    rig.controller.apply(PlaybackAction::PlayEntry(
        PlaylistId::SPOTIFY,
        SpotifyQueue::row_id(0),
    ));
    let shown = rig.controller.state().now_playing();
    assert_eq!(shown.status, Status::Loading);
    assert_eq!(shown.track.unwrap().uri, entry(7).track.uri);
    rig.tell(Event::Spotify(Some(SpotifyQueue {
        here: true,
        status: Status::Playing,
        ..elsewhere
    })));
    assert_eq!(rig.controller.state().now_playing().status, Status::Playing);
}

#[test]
fn songs_go_to_a_queue_no_device_holds_but_not_to_one_another_does() {
    let mut rig = Rig::new("queue-add");
    rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
    rig.tell(Event::Spotify(Some(SpotifyQueue::default())));
    rig.controller
        .apply(BrowseAction::SetTarget(PlaylistId::SPOTIFY));
    rig.sent();
    let added = |rig: &mut Rig| {
        rig.controller.apply(BrowseAction::AddTracks(tracks([1])));
        rig.sent()
            .iter()
            .any(|c| matches!(c, Command::Spotify(SpotifyRequest::Add(_))))
    };
    assert!(added(&mut rig));
    assert_eq!(rig.controller.state().error, None);
    rig.tell(Event::Spotify(Some(SpotifyQueue {
        device: Some("Phone".into()),
        device_id: Some("phone".into()),
        status: Status::Playing,
        now: Some(entry(7).track),
        ..SpotifyQueue::default()
    })));
    assert!(!added(&mut rig));
    assert!(
        rig.controller
            .state()
            .error
            .as_ref()
            .unwrap()
            .contains("(Phone)")
    );
}

#[test]
fn release_pauses_the_queue_playing_here_but_not_elsewhere() {
    let mut rig = Rig::new("release");
    rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
    let elsewhere = SpotifyQueue {
        device: Some("Phone".into()),
        device_id: Some("phone".into()),
        status: Status::Playing,
        now: Some(entry(7).track),
        ..SpotifyQueue::default()
    };
    rig.tell(Event::Spotify(Some(elsewhere.clone())));
    rig.controller.apply(PlaybackAction::PlayEntry(
        PlaylistId::SPOTIFY,
        SpotifyQueue::row_id(0),
    ));
    let paused = |rig: &mut Rig| {
        rig.sent()
            .iter()
            .any(|c| matches!(c, Command::Spotify(SpotifyRequest::Pause)))
    };
    rig.tell(Event::Spotify(Some(SpotifyQueue {
        here: true,
        ..elsewhere.clone()
    })));
    rig.sent();
    rig.controller.apply(PlaybackAction::Release);
    assert!(paused(&mut rig));
    // Handed to another device, the queue is no sound here to let go.
    rig.tell(Event::Spotify(Some(elsewhere)));
    rig.sent();
    rig.controller.apply(PlaybackAction::Release);
    assert!(!paused(&mut rig));
}

#[test]
fn a_local_song_keeps_the_player_paused_until_the_queue_plays_again() {
    let mut rig = Rig::new("owner");
    rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
    let paused_here = SpotifyQueue {
        device: Some("Spotless".into()),
        device_id: Some("spotless".into()),
        here: true,
        status: Status::Paused,
        now: Some(entry(7).track),
        position_ms: 30_000,
        ..SpotifyQueue::default()
    };
    rig.tell(Event::Spotify(Some(paused_here.clone())));
    assert!(rig.controller.state().spotify_playing());
    rig.controller
        .apply(BrowseAction::AddTracks(tracks([1, 2])));
    let first = rig
        .controller
        .state()
        .library
        .get(PlaylistId::DEFAULT)
        .unwrap()
        .entries()[0]
        .id;
    rig.controller
        .apply(PlaybackAction::PlayEntry(PlaylistId::DEFAULT, first));
    let local = |status| PlayerSnapshot {
        status,
        track: Some(TrackInfo {
            uri: "spotify:track:1".into(),
            title: String::new(),
            artists: String::new(),
            main_artist: String::new(),
            album: String::new(),
            duration_ms: 1000,
            cover_url: None,
        }),
        ..PlayerSnapshot::default()
    };
    rig.tell(Event::Player(local(Status::Playing)));
    rig.sent();
    // Space on the Spotify tab pauses the local song, which stays shown.
    rig.controller
        .apply(PlaylistsAction::SelectPlaylist(PlaylistId::SPOTIFY));
    rig.controller.apply(PlaybackAction::TogglePause);
    assert!(
        rig.sent()
            .iter()
            .any(|c| matches!(c, Command::Player(Request::Pause)))
    );
    rig.tell(Event::Player(local(Status::Paused)));
    let state = rig.controller.state();
    assert!(!state.spotify_playing());
    assert_eq!(state.now_playing().track.unwrap().uri, "spotify:track:1");
    // Play on the player goes on with it, rather than the queue.
    rig.controller.apply(PlaybackAction::Play);
    assert!(
        rig.sent()
            .iter()
            .any(|c| matches!(c, Command::Player(Request::Resume)))
    );
    rig.tell(Event::Player(local(Status::Paused)));
    // The queue takes the player back once it plays.
    rig.tell(Event::Spotify(Some(SpotifyQueue {
        status: Status::Playing,
        ..paused_here
    })));
    assert!(rig.controller.state().spotify_playing());
}

#[test]
fn songs_moved_to_the_spotify_tab_stay_when_it_has_no_queue() {
    let mut rig = Rig::new("move-to-spotify");
    rig.controller
        .apply(BrowseAction::AddTracks(tracks([1, 2])));
    let library = &rig.controller.state().library;
    let first = library.get(PlaylistId::DEFAULT).unwrap().entries()[0].id;
    rig.controller.apply(PlaylistsAction::MoveTo(
        PlaylistId::DEFAULT,
        PlaylistId::SPOTIFY,
        vec![first],
    ));
    assert_eq!(titles(&rig, PlaylistId::DEFAULT), ["Song 1", "Song 2"]);
}

#[test]
fn songs_to_come_move_out_of_the_queue_only_while_spotless_has_it() {
    let mut rig = Rig::new("move-from-spotify");
    rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
    let queue = SpotifyQueue {
        device: Some("Phone".into()),
        device_id: Some("phone".into()),
        status: Status::Playing,
        now: Some(entry(7).track),
        parts: vec![crate::spotify::queue::Part {
            title: "Next in queue".into(),
            queued: true,
            songs: vec![crate::spotify::queue::Queued {
                track: entry(8).track,
                index: 0,
            }],
        }],
        ..SpotifyQueue::default()
    };
    rig.tell(Event::Spotify(Some(queue.clone())));
    rig.sent();
    let move_rows = |rig: &mut Rig, rows: Vec<usize>| {
        let rows = rows.into_iter().map(SpotifyQueue::row_id).collect();
        rig.controller.apply(PlaylistsAction::MoveTo(
            PlaylistId::SPOTIFY,
            PlaylistId::DEFAULT,
            rows,
        ));
        rig.sent()
            .into_iter()
            .filter_map(|c| match c {
                Command::Spotify(SpotifyRequest::Remove(songs)) => Some(songs),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    // Held by the phone: nothing moves.
    assert_eq!(move_rows(&mut rig, vec![1]), Vec::<Vec<_>>::new());
    assert!(titles(&rig, PlaylistId::DEFAULT).is_empty());
    // Held by no device: it moves, Spotless taking the queue over.
    rig.tell(Event::Spotify(Some(SpotifyQueue {
        device: None,
        device_id: None,
        status: Status::Stopped,
        ..queue.clone()
    })));
    assert_eq!(
        move_rows(&mut rig, vec![1]),
        [vec![(0, entry(8).track.uri)]]
    );
    rig.tell(Event::Spotify(Some(SpotifyQueue {
        device: Some("Spotless".into()),
        device_id: Some("spotless".into()),
        here: true,
        ..queue
    })));
    // The song playing can't be taken out.
    assert_eq!(move_rows(&mut rig, vec![0, 1]), Vec::<Vec<_>>::new());
    assert_eq!(
        move_rows(&mut rig, vec![1]),
        [vec![(0, entry(8).track.uri)]]
    );
    assert_eq!(titles(&rig, PlaylistId::DEFAULT), ["Song 8", "Song 8"]);
    assert_eq!(
        rig.controller.state().notice.as_deref(),
        Some("Moved 1 song to Default")
    );
}
