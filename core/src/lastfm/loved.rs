//! The songs loved on Last.fm, for the hearts in the playlists and the
//! player, and for Browse's LAST.FM tab.
//!
//! A song is its artist and title, compared by their letters and digits
//! alone: case, accents, spaces and punctuation are set aside, so "AC/DC"
//! is "ACDC" and "Beyoncé" is "Beyonce". Words stay, so a live version or
//! one featuring someone else is a song of its own, loved or not on its
//! own. The artist is the first, as Spotless scrobbles it.

use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

use crate::model::TrackRef;
use crate::playback::engine::TrackInfo;

/// A song loved, as Last.fm has it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LovedSong {
    pub artist: String,
    pub title: String,
    /// When it was loved, in Unix seconds (0: not known).
    #[serde(default)]
    pub date: u64,
}

impl LovedSong {
    /// Who and what it is, by which songs are told apart.
    pub fn key(&self) -> String {
        key(&self.artist, &self.title)
    }
}

/// The songs loved, each once, by artist and then title as they are
/// compared.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Vec<LovedSong>", into = "Vec<LovedSong>")]
pub struct Loved {
    songs: Vec<LovedSong>,
    keys: HashSet<String>,
}

impl Loved {
    pub fn contains(&self, artist: &str, title: &str) -> bool {
        self.keys.contains(&key(artist, title))
    }

    /// Loves the song now, or doesn't any more; says whether that changed.
    pub fn set(&mut self, artist: &str, title: &str, loved: bool) -> bool {
        let key = key(artist, title);
        if !loved {
            if !self.keys.remove(&key) {
                return false;
            }
            self.songs.retain(|song| song.key() != key);
            return true;
        }
        if !self.keys.insert(key.clone()) {
            return false;
        }
        let date = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        let at = self.place(&key);
        self.songs.insert(
            at,
            LovedSong {
                artist: artist.into(),
                title: title.into(),
                date,
            },
        );
        true
    }

    /// Adds the songs not here yet.
    pub fn add(&mut self, songs: Vec<LovedSong>) {
        for song in songs {
            let key = song.key();
            if self.keys.insert(key.clone()) {
                let at = self.place(&key);
                self.songs.insert(at, song);
            }
        }
    }

    /// Where a song not here goes, by its key.
    fn place(&self, key: &str) -> usize {
        self.songs.partition_point(|song| song.key().as_str() < key)
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

    /// By artist, then title.
    pub fn songs(&self) -> &[LovedSong] {
        &self.songs
    }

    pub fn len(&self) -> usize {
        self.songs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.songs.is_empty()
    }
}

impl FromIterator<LovedSong> for Loved {
    fn from_iter<I: IntoIterator<Item = LovedSong>>(songs: I) -> Self {
        let mut keys = HashSet::new();
        let mut songs: Vec<LovedSong> = songs
            .into_iter()
            .filter(|song| keys.insert(song.key()))
            .collect();
        songs.sort_by_cached_key(LovedSong::key);
        Self { songs, keys }
    }
}

impl From<Vec<LovedSong>> for Loved {
    fn from(songs: Vec<LovedSong>) -> Self {
        songs.into_iter().collect()
    }
}

impl From<Loved> for Vec<LovedSong> {
    fn from(loved: Loved) -> Self {
        loved.songs
    }
}

/// An artist's or a title's letters and digits, lowercase and without
/// accents: what is left to compare. A name with none is kept whole,
/// lowercase.
pub fn name_key(name: &str) -> String {
    let key: String = name
        .nfkd()
        .filter(|&c| !is_combining_mark(c) && c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    if key.is_empty() {
        name.trim().to_lowercase()
    } else {
        key
    }
}

fn key(artist: &str, title: &str) -> String {
    format!("{}\u{1f}{}", name_key(artist), name_key(title))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(artist: &str, title: &str) -> LovedSong {
        LovedSong {
            artist: artist.into(),
            title: title.into(),
            date: 0,
        }
    }

    #[test]
    fn only_letters_and_digits_count_without_accents() {
        let same = [
            ("Don't Stop Me Now", "Dont Stop Me Now"),
            ("AC/DC", "AC DC"),
            ("Mr. Brightside", "mr brightside"),
            ("Beyoncé", "Beyonce"),
            ("Sigur Rós", "Sigur Ros"),
            ("Ｆｕｌｌ　Ｗｉｄｔｈ", "Full Width"),
        ];
        for (a, b) in same {
            assert_eq!(name_key(a), name_key(b), "{a} / {b}");
        }
        let different = [
            ("Creep", "Creep - Live"),
            ("Get Lucky", "Get Lucky (feat. Pharrell Williams)"),
            ("Heroes", "Heroes - 2017 Remaster"),
            ("Simon & Garfunkel", "Simon and Garfunkel"),
            ("Part 2", "Pt. 2"),
        ];
        for (a, b) in different {
            assert_ne!(name_key(a), name_key(b), "{a} / {b}");
        }
        assert_eq!(name_key("Кино"), "кино");
        assert_eq!(name_key(" ... "), "...");
        assert_ne!(name_key("..."), name_key("?"));
    }

    #[test]
    fn versions_are_songs_of_their_own() {
        let loved: Loved = [song("Radiohead", "Creep")].into_iter().collect();
        assert!(loved.contains("radiohead", "CREEP"));
        assert!(!loved.contains("Radiohead", "Creep - Live"));
        assert!(!loved.contains("Radio", "headCreep"));
    }

    #[test]
    fn setting_says_whether_it_changed() {
        let mut loved: Loved = [song("B", "Old")].into_iter().collect();
        assert!(loved.set("A", "New", true));
        assert!(!loved.set("a", "new", true));
        assert_eq!(loved.songs()[0].title, "New");
        assert!(loved.set("A", "New", false));
        assert_eq!(loved.songs()[0].title, "Old");
        assert!(loved.set("B", "New", true));
        assert!(loved.set("B", "New", false));
        assert!(!loved.set("B", "New", false));
        assert_eq!(loved.len(), 1);
    }

    #[test]
    fn they_go_by_artist_then_title_once_each() {
        let mut loved: Loved = [song("Björk", "2"), song("Abba", "1")]
            .into_iter()
            .collect();
        loved.add(vec![
            song("björk", "1"),
            song("The Cure", "1"),
            song("Bjork", "2"),
            song("B-52's", "1"),
        ]);
        let songs: Vec<(&str, &str)> = loved
            .songs()
            .iter()
            .map(|s| (s.artist.as_str(), s.title.as_str()))
            .collect();
        assert_eq!(
            songs,
            [
                ("Abba", "1"),
                ("B-52's", "1"),
                ("björk", "1"),
                ("Björk", "2"),
                ("The Cure", "1"),
            ]
        );
    }

    #[test]
    fn it_is_kept_as_its_songs() {
        let loved: Loved = [song("A", "1"), song("B", "2")].into_iter().collect();
        let json = serde_json::to_string(&loved).unwrap();
        assert_eq!(serde_json::from_str::<Loved>(&json).unwrap(), loved);
        assert!(
            serde_json::from_str::<Loved>(&json)
                .unwrap()
                .contains("b", "2")
        );
    }
}
