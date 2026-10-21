//! All the playlists, of which Default always exists, and the ID counters.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::playlist::{Entry, EntryId, Playlist, PlaylistId, TrackRef};

#[derive(Clone, Debug, Serialize, Deserialize)]
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

    /// Restores the invariants of a library read from disk: Default exists,
    /// IDs are unique and the counters are past every ID in use.
    pub fn repair(&mut self) {
        let mut seen = HashSet::new();
        self.playlists.retain(|list| seen.insert(list.id));
        if !seen.contains(&PlaylistId::DEFAULT) {
            self.playlists
                .insert(0, Playlist::new(PlaylistId::DEFAULT, "Default"));
        }
        // An entry ID in two playlists is kept in the first only.
        let mut entries = HashSet::new();
        for list in &mut self.playlists {
            list.repair();
            let duplicates: Vec<EntryId> = list
                .entries()
                .iter()
                .map(|entry| entry.id)
                .filter(|&id| !entries.insert(id))
                .collect();
            list.remove(&duplicates);
            list.repair();
        }
        let max_entry = entries.iter().map(|id| id.0).max().unwrap_or(0);
        self.next_entry_id = self.next_entry_id.max(max_entry + 1);
        let max_list = self
            .playlists
            .iter()
            .map(|list| list.id.0)
            .max()
            .unwrap_or(0);
        self.next_playlist_id = self.next_playlist_id.max(max_list + 1);
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

    /// Renames a playlist. A blank name is refused.
    pub fn rename(&mut self, id: PlaylistId, name: &str) -> bool {
        let name = name.trim();
        match self.get_mut(id) {
            Some(list) if !name.is_empty() => {
                list.name = name.to_string();
                true
            }
            _ => false,
        }
    }

    /// Moves a playlist's tab to `index` (clamped), counted after it is
    /// taken out.
    pub fn move_playlist(&mut self, id: PlaylistId, index: usize) {
        let Some(from) = self.playlists.iter().position(|list| list.id == id) else {
            return;
        };
        let list = self.playlists.remove(from);
        let index = index.min(self.playlists.len());
        self.playlists.insert(index, list);
    }

    /// A name no playlist has yet: "Playlist 2", "Playlist 3"…
    pub fn fresh_name(&self) -> String {
        (2..)
            .map(|n| format!("Playlist {n}"))
            .find(|name| self.playlists.iter().all(|list| list.name != *name))
            .unwrap_or_default()
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
    fn repair_restores_default_and_counters() {
        let mut library = Library::new();
        let mix = library.create("Mix");
        let list = library.get_mut(mix).unwrap();
        list.append(vec![entry(40), entry(40), entry(41)]);
        library.playlists.retain(|list| list.id == mix);
        library.next_entry_id = 1;
        library.repair();
        assert_eq!(library.playlists()[0].id, PlaylistId::DEFAULT);
        assert_eq!(library.get(mix).unwrap().len(), 2);
        assert_eq!(library.entry(entry(0).track).id, EntryId(42));
    }

    #[test]
    fn playlists_rename_and_move() {
        let mut library = Library::new();
        let a = library.create(library.fresh_name());
        let b = library.create(library.fresh_name());
        assert_eq!(library.get(b).unwrap().name, "Playlist 3");
        assert!(!library.rename(a, "  "));
        assert!(library.rename(a, " Mix "));
        assert_eq!(library.get(a).unwrap().name, "Mix");
        library.move_playlist(b, 0);
        let order: Vec<_> = library.playlists().iter().map(|l| l.id).collect();
        assert_eq!(order, [b, PlaylistId::DEFAULT, a]);
        library.move_playlist(b, 99);
        assert_eq!(library.playlists().last().unwrap().id, b);
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
