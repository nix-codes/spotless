//! What the catalog view shows: search results, discographies and the
//! account's own Spotify playlists.

use super::playlist::{ArtistRef, TrackRef};
use crate::spotify::catalog::LIKED_SONGS;

/// Which tab of an artist's discography a release belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReleaseGroup {
    Album,
    Single,
    Compilation,
    AppearsOn,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub uri: String,
    pub title: String,
    /// The release's main artists, joined, for search results.
    pub artists: String,
    pub year: i32,
    pub group: ReleaseGroup,
    /// ALBUM, EP, SINGLE or COMPILATION.
    pub kind: String,
    pub tracks: usize,
    /// The cover's image URL, about 300 px.
    pub cover: Option<String>,
}

impl Release {
    /// Spotify's single type covers EPs too: four songs or more is an EP.
    pub fn kind_label(album_type: &str, tracks: usize) -> String {
        match album_type.to_ascii_lowercase().as_str() {
            "single" if tracks >= 4 => "EP".into(),
            "ep" => "EP".into(),
            other => other.to_uppercase(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchResults {
    pub artists: Vec<ArtistRef>,
    pub albums: Vec<Release>,
    pub songs: Vec<TrackRef>,
    /// Spotify has more songs for the search than these.
    pub more_songs: bool,
    /// The songs Spotify has sent so far, those that weren't kept too:
    /// where its next page starts.
    pub songs_read: usize,
    /// The next page of songs is being read.
    pub reading_songs: bool,
}

impl SearchResults {
    pub fn is_empty(&self) -> bool {
        self.artists.is_empty() && self.albums.is_empty() && self.songs.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Discography {
    pub artist: ArtistRef,
    /// Oldest first.
    pub releases: Vec<Release>,
}

/// One of the account's playlists on Spotify, or its Liked Songs.
#[derive(Clone, Debug, PartialEq)]
pub struct SpotifyPlaylist {
    pub uri: String,
    pub name: String,
    pub tracks: usize,
    /// Who made it, when someone else did: their name on Spotify.
    pub owner: Option<String>,
    /// Only those it's shared with can see it.
    pub private: bool,
}

/// Where a song read from a Spotify playlist or Liked Songs is there:
/// what removing it, and putting it back, needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// Its position in the playlist, counting what Spotless leaves out
    /// (podcast episodes, songs gone from Spotify).
    At(usize),
    /// When it was liked, in seconds since 1970.
    Liked(i64),
}

/// The songs of an album or a Spotify playlist, as read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Songs {
    pub tracks: Vec<TrackRef>,
    /// Each song's place, for a Spotify playlist or Liked Songs; empty for
    /// an album.
    pub places: Vec<Place>,
    /// The playlist's revision they were read at.
    pub revision: Vec<u8>,
}

/// Where songs copied to Spotify go.
#[derive(Clone, Debug, PartialEq)]
pub enum SpotifyTarget {
    /// A new playlist by this name; private, only those it's shared with
    /// see it.
    New { name: String, private: bool },
    /// One of the account's own playlists.
    Playlist { uri: String, name: String },
}

impl SpotifyTarget {
    pub fn name(&self) -> &str {
        match self {
            Self::New { name, .. } | Self::Playlist { name, .. } => name,
        }
    }
}

/// The artist page's tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReleaseTab {
    All,
    /// The artist's albums and compilations.
    #[default]
    Albums,
    SinglesAndEps,
    AppearsOn,
}

impl ReleaseTab {
    pub const ALL: [Self; 4] = [
        Self::All,
        Self::Albums,
        Self::SinglesAndEps,
        Self::AppearsOn,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "ALL",
            Self::Albums => "ALBUMS",
            Self::SinglesAndEps => "SINGLES & EPS",
            Self::AppearsOn => "APPEARS ON",
        }
    }

    pub fn includes(self, group: ReleaseGroup) -> bool {
        match self {
            Self::All => true,
            Self::Albums => matches!(group, ReleaseGroup::Album | ReleaseGroup::Compilation),
            Self::SinglesAndEps => group == ReleaseGroup::Single,
            Self::AppearsOn => group == ReleaseGroup::AppearsOn,
        }
    }
}

/// Drops repeats of a release (same title, year and group, as Spotify
/// lists some sessions twice) and sorts oldest first, keeping Spotify's
/// order within a year.
pub fn tidy(mut releases: Vec<Release>) -> Vec<Release> {
    let mut seen = std::collections::HashSet::new();
    releases.retain(|r| seen.insert((r.title.to_lowercase(), r.year, r.group)));
    releases.sort_by_key(|r| r.year);
    releases
}

/// Puts Liked Songs first and the playlists after it by name, whatever
/// the case; those named alike keep the order they came in.
pub fn sort_playlists(playlists: &mut [SpotifyPlaylist]) {
    playlists.sort_by_cached_key(|p| (p.uri != LIKED_SONGS, p.name.to_lowercase()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(title: &str, year: i32, group: ReleaseGroup) -> Release {
        Release {
            uri: format!("spotify:album:{title}{year}"),
            title: title.into(),
            artists: String::new(),
            year,
            group,
            kind: "ALBUM".into(),
            tracks: 10,
            cover: None,
        }
    }

    #[test]
    fn tidy_drops_repeats_and_sorts_by_year() {
        let releases = tidy(vec![
            release("Geogaddi", 2002, ReleaseGroup::Album),
            release("Peel Session", 1999, ReleaseGroup::Single),
            release("Peel session", 1999, ReleaseGroup::Single),
            release("Peel Session", 1999, ReleaseGroup::AppearsOn),
        ]);
        let titles: Vec<_> = releases.iter().map(|r| (r.year, r.group)).collect();
        assert_eq!(
            titles,
            [
                (1999, ReleaseGroup::Single),
                (1999, ReleaseGroup::AppearsOn),
                (2002, ReleaseGroup::Album)
            ]
        );
    }

    #[test]
    fn playlists_go_by_name_after_liked_songs() {
        let playlist = |uri: &str, name: &str| SpotifyPlaylist {
            uri: uri.into(),
            name: name.into(),
            tracks: 1,
            owner: None,
            private: false,
        };
        let mut playlists = vec![
            playlist("spotify:playlist:b", "basement"),
            playlist("spotify:playlist:c", "Zeros"),
            playlist(LIKED_SONGS, "Liked Songs"),
            playlist("spotify:playlist:a", "Attic"),
            playlist("spotify:playlist:d", "attic"),
        ];
        sort_playlists(&mut playlists);
        let uris: Vec<_> = playlists.iter().map(|p| p.uri.as_str()).collect();
        assert_eq!(
            uris,
            [
                LIKED_SONGS,
                "spotify:playlist:a",
                "spotify:playlist:d",
                "spotify:playlist:b",
                "spotify:playlist:c"
            ]
        );
    }

    #[test]
    fn singles_with_four_songs_are_eps() {
        assert_eq!(Release::kind_label("single", 1), "SINGLE");
        assert_eq!(Release::kind_label("single", 5), "EP");
        assert_eq!(Release::kind_label("album", 12), "ALBUM");
        assert_eq!(Release::kind_label("COMPILATION", 30), "COMPILATION");
    }

    #[test]
    fn the_albums_tab_includes_compilations() {
        assert!(ReleaseTab::Albums.includes(ReleaseGroup::Compilation));
        assert!(!ReleaseTab::Albums.includes(ReleaseGroup::Single));
        assert_eq!(ReleaseTab::default(), ReleaseTab::Albums);
    }
}
