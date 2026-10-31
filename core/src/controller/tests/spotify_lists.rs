//! The account's Spotify playlists in YOUR SPOTIFY.

use super::*;

#[test]
fn spotify_playlists_are_read_again_whenever_browse_opens() {
    let mut rig = Rig::new("spotify-playlists");
    let asked = |rig: &mut Rig| {
        rig.sent()
            .iter()
            .filter(|c| matches!(c, Command::SpotifyPlaylists))
            .count()
    };
    rig.controller.apply(Action::ToggleBrowse);
    assert_eq!(asked(&mut rig), 0, "not before signing in");
    rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
    assert_eq!(asked(&mut rig), 1);
    let playlist = |uri: &str, tracks| SpotifyPlaylist {
        uri: uri.into(),
        name: uri.into(),
        tracks,
        owner: None,
        private: false,
    };
    // Songs read before: one playlist shown, one not.
    for uri in ["spotify:playlist:shown", "spotify:playlist:hidden"] {
        rig.controller.apply(Action::ToggleAlbum(uri.into()));
        rig.tell(Event::Album {
            album: uri.into(),
            result: Ok(songs(tracks([1, 2]))),
        });
    }
    rig.controller
        .apply(Action::ToggleAlbum("spotify:playlist:hidden".into()));
    rig.tell(Event::SpotifyPlaylists(Ok(vec![
        playlist("spotify:playlist:shown", 2),
        playlist("spotify:playlist:hidden", 2),
    ])));
    let browse = &rig.controller.state().browse;
    assert!(matches!(&browse.spotify_playlists, Some(Loadable::Ready(p)) if p.len() == 2));
    assert!(browse.albums.contains_key("spotify:playlist:shown"));
    assert!(!browse.albums.contains_key("spotify:playlist:hidden"));

    rig.controller.apply(Action::ToggleBrowse);
    rig.controller.apply(Action::ToggleBrowse);
    assert_eq!(asked(&mut rig), 1);
    // A song added on Spotify: the songs shown are read again.
    rig.tell(Event::SpotifyPlaylists(Ok(vec![playlist(
        "spotify:playlist:shown",
        3,
    )])));
    assert!(
        rig.sent()
            .iter()
            .any(|c| matches!(c, Command::Album(a) if a == "spotify:playlist:shown"))
    );
    // A failed read keeps those read before.
    rig.controller.apply(Action::ToggleBrowse);
    rig.controller.apply(Action::ToggleBrowse);
    rig.tell(Event::SpotifyPlaylists(Err("offline".into())));
    let browse = &rig.controller.state().browse;
    assert!(matches!(&browse.spotify_playlists, Some(Loadable::Ready(p)) if p.len() == 1));
}

#[test]
fn songs_removed_from_a_spotify_playlist_go_by_their_places_and_come_back() {
    let mut rig = Rig::new("remove-from-spotify");
    rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
    let playlist = |uri: &str, owner: Option<&str>| SpotifyPlaylist {
        uri: uri.into(),
        name: "Mine".into(),
        tracks: 4,
        owner: owner.map(String::from),
        private: false,
    };
    rig.tell(Event::SpotifyPlaylists(Ok(vec![
        playlist("spotify:playlist:mine", None),
        playlist("spotify:playlist:theirs", Some("Them")),
    ])));
    let uri = "spotify:playlist:mine".to_string();
    rig.controller
        .apply(Action::OpenSpotifyPlaylist(uri.clone()));
    assert_eq!(
        rig.controller.state().browse.spotify_open.as_ref(),
        Some(&uri)
    );
    // An episode at 1 and a song gone at 4 aren't shown.
    let places = [0, 2, 3, 5].map(Place::At).to_vec();
    rig.tell(Event::Album {
        album: uri.clone(),
        result: Ok(Songs {
            tracks: tracks([1, 2, 3, 4]),
            places,
            revision: vec![7],
        }),
    });
    rig.sent();
    rig.controller
        .apply(Action::RemoveFromSpotify(uri.clone(), vec![3, 1]));
    let shown = |rig: &Rig| match rig.controller.state().browse.albums.get(&uri) {
        Some(Loadable::Ready(tracks)) => tracks.iter().map(|t| t.title.clone()).collect(),
        _ => Vec::new(),
    };
    assert_eq!(shown(&rig), ["Song 1", "Song 3"]);
    let removal = match rig.sent().pop() {
        Some(Command::RemoveFromSpotify { removal, revision }) => {
            assert_eq!(revision, [7]);
            removal
        }
        other => panic!("sent {other:?}"),
    };
    let places: Vec<Place> = removal.songs.iter().map(|(place, _)| *place).collect();
    assert_eq!(places, [Place::At(2), Place::At(5)]);
    rig.tell(Event::SpotifyEdited {
        removal: removal.clone(),
        undo: false,
        result: Ok(()),
    });
    assert_eq!(
        rig.controller.state().notice.as_deref(),
        Some("Removed 2 songs from “Mine”.")
    );
    assert!(
        rig.sent()
            .iter()
            .any(|c| matches!(c, Command::Album(a) if *a == uri))
    );
    rig.controller.apply(Action::UndoSpotifyRemoval);
    assert!(
        rig.sent()
            .iter()
            .any(|c| matches!(c, Command::PutBackOnSpotify(r) if *r == removal))
    );
    assert!(rig.controller.state().browse.undo.is_none());

    // Others' playlists can't be changed.
    let theirs = "spotify:playlist:theirs".to_string();
    rig.tell(Event::Album {
        album: theirs.clone(),
        result: Ok(Songs {
            tracks: tracks([1]),
            places: vec![Place::At(0)],
            revision: vec![1],
        }),
    });
    rig.controller
        .apply(Action::RemoveFromSpotify(theirs, vec![0]));
    assert!(rig.sent().is_empty());
}

#[test]
fn spotify_playlists_are_deleted_or_unfollowed_and_come_back() {
    let mut rig = Rig::new("unlist-spotify");
    rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
    let playlist = |uri: &str, name: &str, owner: Option<&str>| SpotifyPlaylist {
        uri: uri.into(),
        name: name.into(),
        tracks: 1,
        owner: owner.map(String::from),
        private: false,
    };
    // Listed as Spotify has them; shown the account's own first.
    rig.tell(Event::SpotifyPlaylists(Ok(vec![
        playlist(crate::spotify::catalog::LIKED_SONGS, "Liked Songs", None),
        playlist("spotify:playlist:theirs", "Theirs", Some("Them")),
        playlist("spotify:playlist:mine", "Mine", None),
    ])));
    rig.controller
        .apply(Action::OpenSpotifyPlaylist("spotify:playlist:mine".into()));
    rig.sent();
    // Liked Songs stays.
    rig.controller.apply(Action::UnlistSpotifyPlaylist(
        crate::spotify::catalog::LIKED_SONGS.into(),
    ));
    assert!(rig.sent().is_empty());

    rig.controller.apply(Action::UnlistSpotifyPlaylist(
        "spotify:playlist:mine".into(),
    ));
    let browse = &rig.controller.state().browse;
    let listed = |browse: &Browse| match &browse.spotify_playlists {
        Some(Loadable::Ready(p)) => p.iter().map(|p| p.name.clone()).collect(),
        _ => Vec::new(),
    };
    assert_eq!(listed(browse), ["Liked Songs", "Theirs"]);
    // The one after it in the list is shown instead.
    assert_eq!(
        browse.spotify_open.as_deref(),
        Some("spotify:playlist:theirs")
    );
    let sent = rig.sent();
    let notice = sent
        .iter()
        .find_map(|c| match c {
            Command::UnlistSpotifyPlaylist {
                playlist, notice, ..
            } if playlist == "spotify:playlist:mine" => Some(notice.clone()),
            _ => None,
        })
        .expect("sent to Spotify");
    assert_eq!(notice, "Deleted “Mine” on Spotify.");
    let unlisted = Unlisted {
        playlist: "spotify:playlist:mine".into(),
        name: "Mine".into(),
        at: 1,
        item: vec![1],
        notice,
    };
    rig.tell(Event::SpotifyUnlisted {
        result: Ok(unlisted.clone()),
        undo: false,
    });
    assert_eq!(
        rig.controller.state().notice.as_deref(),
        Some("Deleted “Mine” on Spotify.")
    );
    assert!(
        rig.sent()
            .iter()
            .any(|c| matches!(c, Command::SpotifyPlaylists))
    );
    rig.controller.apply(Action::UndoSpotifyRemoval);
    assert!(
        rig.sent()
            .iter()
            .any(|c| matches!(c, Command::RelistSpotifyPlaylist(u) if *u == unlisted))
    );
    assert!(rig.controller.state().browse.undo.is_none());

    // Someone else's is unfollowed.
    rig.controller.apply(Action::UnlistSpotifyPlaylist(
        "spotify:playlist:theirs".into(),
    ));
    assert!(rig.sent().iter().any(|c| matches!(
        c,
        Command::UnlistSpotifyPlaylist { notice, .. } if notice == "Unfollowed “Theirs”."
    )));
}

#[test]
fn songs_copied_to_spotify_are_sent_once_signed_in() {
    let mut rig = Rig::new("copy-to-spotify");
    let target = SpotifyTarget::New {
        name: "Garage".into(),
        private: true,
    };
    let copy = Action::CopyToSpotify(target.clone(), tracks([1, 2]));
    rig.controller.apply(copy.clone());
    assert!(rig.controller.state().error.is_some());
    rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
    rig.sent();
    rig.controller.apply(copy);
    let sent = rig.sent();
    assert!(sent.iter().any(|c| matches!(
        c,
        Command::CopyToSpotify { target: SpotifyTarget::New { name, private: true }, songs }
            if name == "Garage" && songs.len() == 2
    )));
    rig.tell(Event::CopiedToSpotify {
        target,
        songs: 2,
        result: Ok("spotify:playlist:new".into()),
    });
    assert_eq!(
        rig.controller.state().notice.as_deref(),
        Some("Copied 2 songs to “Garage” on Spotify.")
    );
    assert!(
        rig.sent()
            .iter()
            .any(|c| matches!(c, Command::SpotifyPlaylists))
    );
}
