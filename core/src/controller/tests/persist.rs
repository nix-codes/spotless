//! Saving.

use super::*;

#[test]
fn changes_are_saved_once_they_rest() {
    let mut rig = Rig::new("save");
    assert_eq!(rig.controller.tick(), None);
    rig.controller.apply(BrowseAction::AddTracks(tracks([1])));
    assert!(rig.controller.tick().is_some());
    rig.controller.unsaved = Some(Instant::now() - SAVE_DELAY);
    assert_eq!(rig.controller.tick(), None);
    let (library, _) = store::load_library(&rig.controller.paths);
    let saved = library.get(PlaylistId::DEFAULT).unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved.entries()[0].track.title, "Song 1");
}
