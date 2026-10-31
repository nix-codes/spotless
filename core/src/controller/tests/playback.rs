//! The player, and a song waiting offline.

use super::*;

#[test]
fn volume_spans_librespots_range() {
    assert_eq!(volume_level(0), 0);
    assert_eq!(volume_level(100), u16::MAX);
    assert_eq!(volume_level(250), u16::MAX);
}

#[test]
fn losing_the_session_stops_the_queue() {
    let mut rig = Rig::new("session");
    rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
    rig.controller.apply(Action::AddTracks(tracks([1, 2])));
    rig.controller.apply(Action::Play);
    assert!(
        rig.sent()
            .iter()
            .any(|c| matches!(c, Command::Player(Request::Load { .. })))
    );
    assert_eq!(
        rig.controller.state().transport.playing_list(),
        Some(PlaylistId::DEFAULT)
    );
    rig.tell(Event::Session(SessionState::SignedOut));
    assert_eq!(rig.controller.state().transport.playing_list(), None);
    assert_eq!(rig.controller.state().player, PlayerSnapshot::default());
}

#[test]
fn offline_waits_on_the_song_and_plays_it_once_reconnected() {
    let mut rig = Rig::new("offline");
    rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
    rig.controller.apply(Action::AddTracks(tracks([1, 2, 3])));
    rig.controller.apply(Action::Play);
    rig.sent();
    rig.tell(Event::Unavailable { offline: true });
    let sent = rig.sent();
    assert!(
        sent.iter()
            .any(|c| matches!(c, Command::Player(Request::Stop)))
    );
    assert!(
        !sent
            .iter()
            .any(|c| matches!(c, Command::Player(Request::Load { .. })))
    );
    rig.tell(Event::Reconnected);
    assert!(rig.sent().iter().any(|c| matches!(
        c,
        Command::Player(Request::Load { uri, position_ms: 0, .. }) if uri.ends_with(":1")
    )));
    // Only once.
    rig.tell(Event::Reconnected);
    assert!(rig.sent().is_empty());
}

#[test]
fn a_song_cut_offline_goes_on_where_it_stopped_once_reconnected() {
    let mut rig = Rig::new("cut");
    rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
    rig.controller.apply(Action::AddTracks(tracks([1, 2, 3])));
    rig.controller.apply(Action::Play);
    rig.sent();
    rig.tell(Event::Cut {
        position_ms: 95_000,
    });
    assert!(
        rig.sent()
            .iter()
            .any(|c| matches!(c, Command::Player(Request::Stop)))
    );
    rig.tell(Event::Reconnected);
    assert!(rig.sent().iter().any(|c| matches!(
        c,
        Command::Player(Request::Load { uri, position_ms: 95_000, .. }) if uri.ends_with(":1")
    )));
}

#[test]
fn a_song_waiting_offline_shows_paused_and_play_goes_on_from_there() {
    let mut rig = Rig::new("paused");
    rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
    rig.controller.apply(Action::AddTracks(tracks([1, 2])));
    rig.controller.apply(Action::Play);
    rig.tell(Event::Cut {
        position_ms: 95_000,
    });
    // librespot's second word of it, and the engine's own news of
    // stopping, don't undo it.
    rig.tell(Event::Cut {
        position_ms: 95_000,
    });
    rig.tell(Event::Player(PlayerSnapshot::default()));
    let state = rig.controller.state();
    assert!(state.offline);
    assert_eq!(state.error, None);
    assert_eq!(state.player.status, Status::Paused);
    assert_eq!(state.player.position_ms, 95_000);
    assert_eq!(state.player.track.as_ref().unwrap().title, "Song 1");
    rig.sent();
    rig.controller.apply(Action::TogglePause);
    assert!(!rig.controller.state().offline);
    assert!(rig.sent().iter().any(|c| matches!(
        c,
        Command::Player(Request::Load { uri, position_ms: 95_000, .. }) if uri.ends_with(":1")
    )));
    rig.tell(Event::Reconnected);
    assert!(rig.sent().is_empty());
    // Until the new player knows the song, the song stays shown.
    rig.tell(Event::Player(PlayerSnapshot {
        status: Status::Loading,
        ..PlayerSnapshot::default()
    }));
    let player = &rig.controller.state().player;
    assert_eq!(player.status, Status::Loading);
    assert_eq!(player.position_ms, 95_000);
    assert_eq!(player.track.as_ref().unwrap().title, "Song 1");
    let known = PlayerSnapshot {
        status: Status::Playing,
        track: player.track.clone(),
        position_ms: 95_100,
        ..PlayerSnapshot::default()
    };
    rig.tell(Event::Player(known.clone()));
    assert_eq!(rig.controller.state().player, known);
    rig.tell(Event::Player(PlayerSnapshot::default()));
    assert_eq!(rig.controller.state().player, PlayerSnapshot::default());
}

#[test]
fn a_song_left_playing_shows_paused_there_after_a_restart() {
    let mut rig = Rig::new("restart-paused");
    rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
    rig.controller.apply(Action::AddTracks(tracks([1, 2])));
    rig.controller.apply(Action::Play);
    rig.tell(Event::Player(PlayerSnapshot {
        status: Status::Playing,
        track: Some(TrackInfo::from(&entry(1).track)),
        position_ms: 95_000,
        ..PlayerSnapshot::default()
    }));
    rig.controller.save();

    let paths = rig.controller.paths.clone();
    let (backend, mut commands, events) = Backend::detached();
    let settings = Settings::load(&paths);
    let mut controller = Controller::with_backend(paths, settings, Ok(backend));
    events
        .send(Event::Session(SessionState::Connecting))
        .unwrap();
    controller.receive();
    events
        .send(Event::Session(SessionState::Ready { user: "u".into() }))
        .unwrap();
    controller.receive();
    let shown = controller.state().now_playing();
    assert_eq!(shown.status, Status::Paused);
    assert_eq!(shown.position_ms, 95_000);
    assert_eq!(shown.track.unwrap().title, "Song 1");
    controller.apply(Action::Seek(60_000));
    assert_eq!(controller.state().now_playing().position_ms, 60_000);
    controller.apply(Action::TogglePause);
    let sent: Vec<_> = std::iter::from_fn(|| commands.try_recv().ok()).collect();
    assert!(sent.iter().any(|c| matches!(
        c,
        Command::Player(Request::Load { uri, position_ms: 60_000, .. }) if uri.ends_with(":1")
    )));
}

#[test]
fn a_stop_while_offline_stays_stopped_when_reconnected() {
    let mut rig = Rig::new("offline-stop");
    rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
    rig.controller.apply(Action::AddTracks(tracks([1, 2])));
    rig.controller.apply(Action::Play);
    rig.tell(Event::Unavailable { offline: true });
    rig.controller.apply(Action::Stop);
    rig.sent();
    rig.tell(Event::Reconnected);
    assert!(rig.sent().is_empty());
}

#[test]
fn after_a_restart_the_song_play_goes_on_with_is_shown() {
    let mut rig = Rig::new("restart");
    rig.controller.apply(Action::AddTracks(tracks([1, 2])));
    let second = rig
        .controller
        .state()
        .library
        .get(PlaylistId::DEFAULT)
        .unwrap()
        .entries()[1]
        .id;
    rig.controller
        .apply(Action::PlayEntry(PlaylistId::DEFAULT, second));
    rig.controller.apply(Action::Stop);
    rig.controller.save();
    let (backend, _, _) = Backend::detached();
    let paths = rig.controller.paths.clone();
    let settings = Settings::load(&paths);
    let again = Controller::with_backend(paths, settings, Ok(backend));
    let shown = again.state().now_playing();
    assert_eq!(shown.status, Status::Stopped);
    assert_eq!(shown.track.unwrap().title, "Song 2");
    assert_eq!(
        again.state().transport.playback.playing(),
        Some(PlaylistId::DEFAULT)
    );
}

#[test]
fn stopped_the_player_shows_the_song_play_starts() {
    let mut rig = Rig::new("waiting");
    rig.controller.apply(Action::AddTracks(tracks([1, 2])));
    let entries: Vec<_> = rig
        .controller
        .state()
        .library
        .get(PlaylistId::DEFAULT)
        .unwrap()
        .entries()
        .iter()
        .map(|entry| entry.id)
        .collect();
    rig.controller
        .apply(Action::PlayEntry(PlaylistId::DEFAULT, entries[0]));
    rig.controller.apply(Action::Stop);
    // The engine still has the stopped song.
    rig.tell(Event::Player(PlayerSnapshot {
        track: Some(TrackInfo::from(&entry(1).track)),
        ..PlayerSnapshot::default()
    }));
    let title = |rig: &Rig| {
        rig.controller
            .state()
            .now_playing()
            .track
            .map(|track| track.title)
    };
    assert_eq!(title(&rig).as_deref(), Some("Song 1"));
    rig.controller
        .apply(Action::Remove(PlaylistId::DEFAULT, vec![entries[0]]));
    assert_eq!(
        title(&rig).as_deref(),
        Some("Song 2"),
        "Play goes on with the next"
    );
    rig.controller.apply(Action::Play);
    assert_eq!(
        title(&rig).as_deref(),
        Some("Song 2"),
        "loading, before the engine says so"
    );
    rig.controller.apply(Action::Stop);
    rig.controller
        .apply(Action::Remove(PlaylistId::DEFAULT, vec![entries[1]]));
    assert_eq!(title(&rig), None, "an empty playlist plays nothing");
}
