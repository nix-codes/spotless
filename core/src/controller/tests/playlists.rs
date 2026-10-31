//! The playlists and their songs.

use super::*;

#[test]
fn closing_a_tab_shows_the_one_before_it() {
    let mut rig = Rig::new("close");
    rig.controller.apply(Action::NewPlaylist);
    let second = rig.controller.state().viewed;
    rig.controller.apply(Action::NewPlaylist);
    let third = rig.controller.state().viewed;
    assert_ne!(second, third);
    rig.controller.apply(Action::ClosePlaylist(third));
    assert_eq!(rig.controller.state().viewed, second);
    assert!(rig.controller.state().library.get(third).is_none());
}

#[test]
fn closing_the_only_tab_shows_a_new_empty_default() {
    let mut rig = Rig::new("close-last");
    rig.controller.apply(Action::AddTracks(tracks([1, 2])));
    rig.controller.apply(Action::Play);
    rig.controller
        .apply(Action::ClosePlaylist(PlaylistId::DEFAULT));
    let state = rig.controller.state();
    let [fresh] = state.library.playlists() else {
        panic!("one tab");
    };
    assert_eq!((fresh.name.as_str(), fresh.len()), ("Default", 0));
    assert_eq!(state.viewed, fresh.id);
    assert_eq!(state.transport.playing_list(), None);
}

#[test]
fn songs_moved_to_another_playlist_leave_this_one() {
    let mut rig = Rig::new("move-to");
    rig.controller.apply(Action::AddTracks(tracks([1, 2, 3])));
    rig.controller.apply(Action::NewPlaylist);
    let other = rig.controller.state().viewed;
    let library = &rig.controller.state().library;
    let entries = library.get(PlaylistId::DEFAULT).unwrap().entries();
    let moved = vec![entries[2].id, entries[0].id];
    rig.controller
        .apply(Action::MoveTo(PlaylistId::DEFAULT, other, moved));
    assert_eq!(titles(&rig, PlaylistId::DEFAULT), ["Song 2"]);
    assert_eq!(titles(&rig, other), ["Song 3", "Song 1"]);
    assert_eq!(
        rig.controller.state().notice.as_deref(),
        Some("Moved 2 songs to Playlist 2")
    );
}

#[test]
fn a_new_playlist_can_start_with_songs() {
    let mut rig = Rig::new("new-with");
    rig.controller
        .apply(Action::NewPlaylistWith(vec![entry(4).track]));
    let shown = rig.controller.state().viewed;
    assert_ne!(shown, PlaylistId::DEFAULT);
    assert_eq!(titles(&rig, shown), ["Song 4"]);
}
