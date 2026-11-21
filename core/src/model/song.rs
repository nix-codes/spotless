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

/// A song's URI, its scheme naming its source.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub struct SongUri(String);

/// The source a song's URI names, with what that source finds it by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SongSource<'a> {
    /// `spotify:track:<id>`, the ID in base 62.
    Spotify { id: &'a str },
    /// A source this Spotless doesn't know, or no URI at all: it doesn't
    /// play here.
    Unknown,
}

impl SongUri {
    /// Spotify's track `id`, in base 62.
    pub fn spotify(id: &str) -> Self {
        Self(format!("{SPOTIFY_TRACK}{id}"))
    }

    pub fn source(&self) -> SongSource<'_> {
        match self.0.strip_prefix(SPOTIFY_TRACK) {
            Some(id) if !id.is_empty() => SongSource::Spotify { id },
            _ => SongSource::Unknown,
        }
    }

    /// Spotify's ID for the song, if it's a Spotify song.
    pub fn spotify_id(&self) -> Option<&str> {
        match self.source() {
            SongSource::Spotify { id } => Some(id),
            SongSource::Unknown => None,
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
        for other in ["mega:abc:def#key", "spotify:episode:x", "spotify:track:", ""] {
            assert_eq!(SongUri::from(other).source(), SongSource::Unknown, "{other}");
        }
    }

    #[test]
    fn a_song_of_a_source_not_known_saves_as_it_was_read() {
        let text = "\"mega:abc:def#key\"";
        let song: SongUri = serde_json::from_str(text).unwrap();
        assert_eq!(serde_json::to_string(&song).unwrap(), text);
    }
}
