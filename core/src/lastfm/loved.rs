//! The songs loved on Last.fm, for the hearts in the playlists and the
//! player.
//!
//! A song is the artist and the title exactly as Spotless scrobbles them
//! (the first artist, the title as Spotify has it), so a live version or
//! one featuring someone else is a song of its own, loved or not on its
//! own. Only case is set aside, as Last.fm does.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::model::TrackRef;
use crate::playback::engine::TrackInfo;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Loved(HashSet<String>);

impl Loved {
    pub fn contains(&self, artist: &str, title: &str) -> bool {
        self.0.contains(&key(artist, title))
    }

    /// Loves the song, or doesn't any more; says whether that changed.
    pub fn set(&mut self, artist: &str, title: &str, loved: bool) -> bool {
        let key = key(artist, title);
        if loved {
            self.0.insert(key)
        } else {
            self.0.remove(&key)
        }
    }

    /// A playlist's song.
    pub fn track(&self, track: &TrackRef) -> bool {
        let artist = track.artists.first().map_or("", |a| a.name.as_str());
        self.contains(artist, &track.title)
    }

    /// The player's song.
    pub fn playing(&self, track: &TrackInfo) -> bool {
        self.contains(&track.main_artist, &track.title)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<A: AsRef<str>, T: AsRef<str>> FromIterator<(A, T)> for Loved {
    fn from_iter<I: IntoIterator<Item = (A, T)>>(songs: I) -> Self {
        Self(
            songs
                .into_iter()
                .map(|(artist, title)| key(artist.as_ref(), title.as_ref()))
                .collect(),
        )
    }
}

impl Extend<(String, String)> for Loved {
    fn extend<I: IntoIterator<Item = (String, String)>>(&mut self, songs: I) {
        self.0.extend(
            songs
                .into_iter()
                .map(|(artist, title)| key(&artist, &title)),
        );
    }
}

fn key(artist: &str, title: &str) -> String {
    format!("{}\u{1f}{}", artist.to_lowercase(), title.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_songs_of_their_own_and_case_is_set_aside() {
        let loved: Loved = [("Radiohead", "Creep")].into_iter().collect();
        assert!(loved.contains("radiohead", "CREEP"));
        assert!(!loved.contains("Radiohead", "Creep - Live"));
        assert!(!loved.contains("Radiohead", "Creep (feat. Someone)"));
        assert!(!loved.contains("Radio", "headCreep"));
    }

    #[test]
    fn setting_says_whether_it_changed() {
        let mut loved = Loved::default();
        assert!(loved.set("A", "B", true));
        assert!(!loved.set("a", "b", true));
        assert!(loved.set("A", "B", false));
        assert!(!loved.set("A", "B", false));
        assert!(loved.is_empty());
    }
}
