//! Signing out, and what the backend answers after it.

use super::*;

fn playlist(uri: &str) -> SpotifyPlaylist {
    SpotifyPlaylist {
        uri: uri.into(),
        name: uri.into(),
        tracks: 1,
        owner: None,
        private: false,
    }
}

fn spotify_playlists(rig: &Rig) -> Option<Vec<String>> {
    match &rig.controller.state().browse.spotify_playlists {
        Some(Loadable::Ready(lists)) => Some(lists.iter().map(|l| l.uri.clone()).collect()),
        _ => None,
    }
}

/// Alice signed in, with Browse open on her playlists, one of them open
/// with its songs read, and an album's songs read.
fn alice_reading(name: &str) -> Rig {
    let mut rig = Rig::new(name);
    rig.controller.apply(BrowseAction::ToggleBrowse);
    rig.tell(Event::Session(SessionState::Ready {
        user: "alice".into(),
    }));
    rig.tell(Event::SpotifyPlaylists(Ok(vec![playlist(
        "spotify:playlist:alices",
    )])));
    for uri in ["spotify:playlist:alices", "spotify:album:public"] {
        rig.controller
            .apply(SpotifyListsAction::OpenSpotifyPlaylist(uri.into()));
        rig.tell(Event::Album {
            album: uri.into(),
            result: Ok(songs(tracks([1]))),
        });
    }
    rig
}

#[test]
fn signing_out_forgets_the_accounts_spotify_playlists() {
    let mut rig = alice_reading("sign-out-forgets");
    rig.tell(Event::Session(SessionState::SignedOut));
    assert_eq!(spotify_playlists(&rig), None);
    let browse = &rig.controller.state().browse;
    assert_eq!(browse.spotify_open, None);
    assert!(!browse.albums.contains_key("spotify:playlist:alices"));
    // An album is the same for everyone: its songs stay.
    assert!(browse.albums.contains_key("spotify:album:public"));
}

#[test]
fn losing_the_network_forgets_nothing() {
    let mut rig = alice_reading("network-keeps");
    rig.tell(Event::Session(SessionState::Connecting));
    assert_eq!(
        spotify_playlists(&rig),
        Some(vec!["spotify:playlist:alices".into()])
    );
    let browse = &rig.controller.state().browse;
    assert!(browse.albums.contains_key("spotify:playlist:alices"));
}

#[test]
fn a_read_answered_after_signing_out_is_not_kept() {
    let mut rig = Rig::new("late-read");
    rig.controller.apply(BrowseAction::ToggleBrowse);
    rig.tell(Event::Session(SessionState::Ready {
        user: "alice".into(),
    }));
    // Alice's playlists were asked for; she signs out before the answer.
    rig.tell(Event::Session(SessionState::SignedOut));
    rig.tell(Event::SpotifyPlaylists(Ok(vec![playlist(
        "spotify:playlist:alices",
    )])));
    assert_eq!(spotify_playlists(&rig), None, "signed out");
    // Bob signs in: until his own read answers, nothing of Alice's shows.
    rig.tell(Event::Session(SessionState::Ready { user: "bob".into() }));
    assert_eq!(spotify_playlists(&rig), None, "signed in as Bob");
}
