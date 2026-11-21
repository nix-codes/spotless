//! Where a song is: what plays it, what the cache finds it by, what the
//! player and Last.fm follow while it plays. Only on Spotify is that also
//! what the song is; another source's songs are told apart by where they
//! are.
//!
//! Kept as the text it was read from, so a playlist saved by a newer
//! Spotless, with songs of a source this one doesn't know, reads and saves
//! again unchanged: those songs are only unavailable here.

use std::fmt;

use serde::{Deserialize, Serialize};

const SPOTIFY_TRACK: &str = "spotify:track:";
const MEGA: &str = "mega:";

/// A song's URI, its scheme naming its source.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub struct SongUri(String);

/// The source a song's URI names, with what that source finds it by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SongSource<'a> {
    /// `spotify:track:<id>`, the ID in base 62.
    Spotify { id: &'a str },
    /// `mega:<folder>:<file>#<key>`: a file of a public MEGA folder, by
    /// their handles, with the folder's key (base 64, as in its link).
    Mega {
        folder: &'a str,
        file: &'a str,
        key: &'a str,
    },
    /// A source this Spotless doesn't know, or no URI at all: it doesn't
    /// play here.
    Unknown,
}

impl SongUri {
    /// Spotify's track `id`, in base 62.
    pub fn spotify(id: &str) -> Self {
        Self(format!("{SPOTIFY_TRACK}{id}"))
    }

    /// A file of a public MEGA folder.
    pub fn mega(folder: &str, file: &str, key: &str) -> Self {
        Self(format!("{MEGA}{folder}:{file}#{key}"))
    }

    pub fn source(&self) -> SongSource<'_> {
        if let Some(id) = self.0.strip_prefix(SPOTIFY_TRACK) {
            return match id.is_empty() {
                false => SongSource::Spotify { id },
                true => SongSource::Unknown,
            };
        }
        if let Some(rest) = self.0.strip_prefix(MEGA)
            && let Some((place, key)) = rest.split_once('#')
            && let Some((folder, file)) = place.split_once(':')
            && [folder, file, key].iter().all(|part| !part.is_empty())
        {
            return SongSource::Mega { folder, file, key };
        }
        SongSource::Unknown
    }

    /// Spotify's ID for the song, if it's a Spotify song.
    pub fn spotify_id(&self) -> Option<&str> {
        match self.source() {
            SongSource::Spotify { id } => Some(id),
            SongSource::Mega { .. } | SongSource::Unknown => None,
        }
    }

    pub fn is_spotify(&self) -> bool {
        self.spotify_id().is_some()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<String> for SongUri {
    fn from(text: String) -> Self {
        Self(text)
    }
}

impl From<&str> for SongUri {
    fn from(text: &str) -> Self {
        Self(text.into())
    }
}

impl From<SongUri> for String {
    fn from(uri: SongUri) -> Self {
        uri.0
    }
}

impl fmt::Display for SongUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scheme_names_the_source() {
        let song = SongUri::from("spotify:track:4uLU6hMCjMI75M1A2tKUQC");
        assert_eq!(song.source(), SongSource::Spotify { id: "4uLU6hMCjMI75M1A2tKUQC" });
        assert_eq!(song, SongUri::spotify("4uLU6hMCjMI75M1A2tKUQC"));
        let song = SongUri::mega("abc", "def", "key");
        assert_eq!(song.as_str(), "mega:abc:def#key");
        assert_eq!(
            song.source(),
            SongSource::Mega {
                folder: "abc",
                file: "def",
                key: "key"
            }
        );
        for other in ["mega:abc:def", "mega::def#key", "spotify:episode:x", "spotify:track:", ""] {
            assert_eq!(SongUri::from(other).source(), SongSource::Unknown, "{other}");
        }
    }

    #[test]
    fn a_song_of_a_source_not_known_saves_as_it_was_read() {
        let text = "\"local:/music/song.ogg\"";
        let song: SongUri = serde_json::from_str(text).unwrap();
        assert_eq!(serde_json::to_string(&song).unwrap(), text);
    }
}
