//! All the playlists, of which Default always exists, and the ID counters.

use super::playlist::{Entry, EntryId, Playlist, PlaylistId, TrackRef};

#[derive(Clone, Debug)]
pub struct Library {
    playlists: Vec<Playlist>,
    next_entry_id: u64,
    next_playlist_id: u64,
}

impl Default for Library {
    fn default() -> Self {
        Self::new()
    }
}

impl Library {
    pub fn new() -> Self {
        Self {
            playlists: vec![Playlist::new(PlaylistId::DEFAULT, "Default")],
            next_entry_id: 1,
            next_playlist_id: 1,
        }
    }

    /// In tab order. Default is never missing.
    pub fn playlists(&self) -> &[Playlist] {
        &self.playlists
    }

    pub fn get(&self, id: PlaylistId) -> Option<&Playlist> {
        self.playlists.iter().find(|list| list.id == id)
    }

    pub fn get_mut(&mut self, id: PlaylistId) -> Option<&mut Playlist> {
        self.playlists.iter_mut().find(|list| list.id == id)
    }

    /// Wraps a track in an entry with a new ID.
    pub fn entry(&mut self, track: TrackRef) -> Entry {
        let id = EntryId(self.next_entry_id);
        self.next_entry_id += 1;
        Entry { id, track }
    }

    /// Adds a playlist as the last tab.
    pub fn create(&mut self, name: impl Into<String>) -> PlaylistId {
        let id = PlaylistId(self.next_playlist_id);
        self.next_playlist_id += 1;
        self.playlists.push(Playlist::new(id, name));
        id
    }

    /// Deletes a playlist. Default cannot be deleted.
    pub fn delete(&mut self, id: PlaylistId) -> bool {
        if id == PlaylistId::DEFAULT {
            return false;
        }
        let before = self.playlists.len();
        self.playlists.retain(|list| list.id != id);
        self.playlists.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::playlist::tests::entry;

    #[test]
    fn default_always_exists() {
        let mut library = Library::new();
        assert_eq!(library.playlists()[0].id, PlaylistId::DEFAULT);
        assert!(!library.delete(PlaylistId::DEFAULT));
        let mix = library.create("Mix");
        assert!(library.delete(mix));
        assert!(!library.delete(mix));
        assert_eq!(library.playlists().len(), 1);
    }

    #[test]
    fn ids_are_never_reused() {
        let mut library = Library::new();
        let track = entry(0).track;
        let a = library.entry(track.clone());
        let b = library.entry(track);
        assert_ne!(a.id, b.id);
        let mix = library.create("Mix");
        library.delete(mix);
        assert_ne!(library.create("Mix"), mix);
    }
}
