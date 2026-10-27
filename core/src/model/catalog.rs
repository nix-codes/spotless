//! What the catalog view shows: search results, discographies and the
//! account's own Spotify playlists.

use super::playlist::{ArtistRef, TrackRef};

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
}

/// Where songs copied to Spotify go.
#[derive(Clone, Debug, PartialEq)]
pub enum SpotifyTarget {
    /// A new private playlist by this name.
    New(String),
    /// One of the account's own playlists.
    Playlist { uri: String, name: String },
}

impl SpotifyTarget {
    pub fn name(&self) -> &str {
        match self {
            Self::New(name) | Self::Playlist { name, .. } => name,
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
