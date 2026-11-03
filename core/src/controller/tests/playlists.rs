//! The playlists and their songs.

use super::*;

#[test]
fn closing_a_tab_shows_the_one_before_it() {
    let mut rig = Rig::new("close");
    rig.controller.apply(PlaylistsAction::NewPlaylist);
    let second = rig.controller.state().viewed;
    rig.controller.apply(PlaylistsAction::NewPlaylist);
    let third = rig.controller.state().viewed;
    assert_ne!(second, third);
    rig.controller.apply(PlaylistsAction::ClosePlaylist(third));
    assert_eq!(rig.controller.state().viewed, second);
    assert!(rig.controller.state().library.get(third).is_none());
}

#[test]
fn closing_the_only_tab_shows_a_new_empty_default() {
    let mut rig = Rig::new("close-last");
    rig.controller
        .apply(BrowseAction::AddTracks(tracks([1, 2])));
    rig.controller.apply(PlaybackAction::Play);
    rig.controller
        .apply(PlaylistsAction::ClosePlaylist(PlaylistId::DEFAULT));
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
    rig.controller
        .apply(BrowseAction::AddTracks(tracks([1, 2, 3])));
    rig.controller.apply(PlaylistsAction::NewPlaylist);
    let other = rig.controller.state().viewed;
    let library = &rig.controller.state().library;
    let entries = library.get(PlaylistId::DEFAULT).unwrap().entries();
    let moved = vec![entries[2].id, entries[0].id];
    rig.controller
        .apply(PlaylistsAction::MoveTo(PlaylistId::DEFAULT, other, moved));
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
        .apply(PlaylistsAction::NewPlaylistWith(vec![entry(4).track]));
    let shown = rig.controller.state().viewed;
    assert_ne!(shown, PlaylistId::DEFAULT);
    assert_eq!(titles(&rig, shown), ["Song 4"]);
}

/// The titles of the default playlist with songs 1 to 4, reversed.
fn reversed_default(rig: &mut Rig) {
    rig.controller
        .apply(BrowseAction::AddTracks(tracks([1, 2, 3, 4])));
    rig.controller.apply(PlaylistsAction::Reorder(
        PlaylistId::DEFAULT,
        Order::Reversed,
    ));
}

#[test]
fn a_reorder_says_what_it_did_and_undo_puts_it_back_once() {
    let mut rig = Rig::new("reorder-undo");
    reversed_default(&mut rig);
    assert_eq!(
        titles(&rig, PlaylistId::DEFAULT),
        ["Song 4", "Song 3", "Song 2", "Song 1"]
    );
    let state = rig.controller.state();
    assert_eq!(state.notice.as_deref(), Some("Reversed: Default"));
    assert!(state.can_undo_reorder());
    rig.controller.apply(PlaylistsAction::UndoReorder);
    assert_eq!(
        titles(&rig, PlaylistId::DEFAULT),
        ["Song 1", "Song 2", "Song 3", "Song 4"]
    );
    assert!(!rig.controller.state().can_undo_reorder());
    rig.controller.apply(PlaylistsAction::UndoReorder);
    assert_eq!(titles(&rig, PlaylistId::DEFAULT)[0], "Song 1", "only once");
}

#[test]
fn changing_the_songs_ends_the_undo_and_playing_doesnt() {
    let mut rig = Rig::new("reorder-undo-ends");
    reversed_default(&mut rig);
    rig.controller.apply(PlaybackAction::Play);
    assert!(rig.controller.state().can_undo_reorder(), "playing");
    rig.controller.apply(BrowseAction::AddTracks(tracks([5])));
    assert!(!rig.controller.state().can_undo_reorder(), "added");
    rig.controller.apply(PlaylistsAction::UndoReorder);
    assert_eq!(titles(&rig, PlaylistId::DEFAULT)[0], "Song 4", "undo ended");

    let mut rig = Rig::new("reorder-undo-moved");
    reversed_default(&mut rig);
    let first = rig.controller.state().library.get(PlaylistId::DEFAULT);
    let first = first.unwrap().entries()[0].id;
    rig.controller.apply(PlaylistsAction::Move(vec![first], 3));
    assert!(!rig.controller.state().can_undo_reorder(), "moved");

    let mut rig = Rig::new("reorder-undo-removed");
    reversed_default(&mut rig);
    let first = rig.controller.state().library.get(PlaylistId::DEFAULT);
    let first = first.unwrap().entries()[0].id;
    rig.controller
        .apply(PlaylistsAction::Remove(PlaylistId::DEFAULT, vec![first]));
    assert!(!rig.controller.state().can_undo_reorder(), "removed");
}

#[test]
fn undo_shows_the_playlist_it_puts_back() {
    let mut rig = Rig::new("reorder-undo-tab");
    reversed_default(&mut rig);
    rig.controller.apply(PlaylistsAction::NewPlaylist);
    rig.controller.apply(PlaylistsAction::UndoReorder);
    let state = rig.controller.state();
    assert_eq!(state.viewed, PlaylistId::DEFAULT);
    assert_eq!(titles(&rig, PlaylistId::DEFAULT)[0], "Song 1");
}

#[test]
fn a_list_of_one_song_has_nothing_to_reorder_or_undo() {
    let mut rig = Rig::new("reorder-one");
    rig.controller.apply(BrowseAction::AddTracks(tracks([1])));
    rig.controller.apply(PlaylistsAction::Reorder(
        PlaylistId::DEFAULT,
        Order::Shuffled,
    ));
    assert!(!rig.controller.state().can_undo_reorder());
}

#[test]
fn headers_are_each_playlists_own_and_survive_a_restart() {
    let mut rig = Rig::new("headers-own");
    rig.controller.apply(PlaylistsAction::NewPlaylist);
    let other = rig.controller.state().viewed;
    rig.controller
        .apply(PlaylistsAction::SetHeaders(other, false));
    let library = &rig.controller.state().library;
    assert!(!library.get(other).unwrap().headers_on());
    assert!(library.get(PlaylistId::DEFAULT).unwrap().headers_on());
    let saved = serde_json::to_value(library.get(other).unwrap()).unwrap();
    let read: Playlist = serde_json::from_value(saved).unwrap();
    assert!(!read.headers_on());
}
