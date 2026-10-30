//! The catalog view's state: where it is, its back history, and what has
//! been read from Spotify so far.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

use crate::model::catalog::{
    Discography, Place, Release, ReleaseTab, SearchResults, SpotifyPlaylist,
};
use crate::model::{ArtistRef, EntryId, PlaylistId, Source, TrackRef};

/// How long rows just added light up in the slim column.
pub const FLASH: Duration = Duration::from_millis(1200);
const RECENT_LIMIT: usize = 8;
const SEARCHES_LIMIT: usize = 8;

#[derive(Clone, Debug, PartialEq)]
pub enum Page {
    Home,
    Search(String),
    /// An artist's discography, by URI.
    Artist(String),
}

/// Browse's tabs: the catalogue, searched, the account's own Spotify
/// playlists, and the songs loved on Last.fm.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BrowseTab {
    #[default]
    Search,
    Spotify,
    LastFm,
}

/// Songs taken out of a Spotify playlist or Liked Songs, to put back.
#[derive(Clone, Debug, PartialEq)]
pub struct Removal {
    pub playlist: String,
    pub name: String,
    /// Each song's place and URI.
    pub songs: Vec<(Place, String)>,
    /// What the status bar said once they were gone, with UNDO beside it.
    pub notice: String,
}

/// A Spotify playlist taken out of the account's list (deleted, if its
/// own, else unfollowed), to put back.
#[derive(Clone, Debug, PartialEq)]
pub struct Unlisted {
    pub playlist: String,
    pub name: String,
    /// Where it was in the list, folders counted, and how it was listed
    /// (Spotify's own record of it).
    pub at: usize,
    pub item: Vec<u8>,
    pub notice: String,
}

/// What UNDO, or Ctrl+Z, puts back on Spotify.
#[derive(Clone, Debug, PartialEq)]
pub enum Undo {
    Songs(Removal),
    Playlist(Unlisted),
}

impl Undo {
    /// What the status bar said once it was done.
    pub fn notice(&self) -> &str {
        match self {
            Undo::Songs(removal) => &removal.notice,
            Undo::Playlist(unlisted) => &unlisted.notice,
        }
    }
}

/// What ADD ALL adds on an artist's page: what the page shows.
#[derive(Clone, Debug, PartialEq)]
pub enum AddAll {
    /// The tab's releases, by URI, in the order shown.
    Releases(Vec<String>),
    /// The popular songs, or what a find found: the songs found, and whole
    /// the releases found by their title.
    Songs(Vec<TrackRef>),
    /// A release found by its title is still being read.
    Reading,
}

/// What a find narrows a page to: names with every word typed in them,
/// whatever their case and accents.
#[derive(Clone, Debug, PartialEq)]
pub struct Find(Vec<String>);

impl Find {
    pub fn new(text: &str) -> Self {
        Self(fold(text).split_whitespace().map(String::from).collect())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Every word is in one of these names, or across them.
    pub fn matches(&self, names: &[&str]) -> bool {
        let names = fold(&names.join(" "));
        self.0.iter().all(|word| names.contains(word.as_str()))
    }

    /// Every word is in the song's name, its artists' or its album's.
    pub fn track(&self, track: &TrackRef) -> bool {
        let mut names = song_names(track);
        names.push(&track.album.name);
        self.matches(&names)
    }

    /// Every word is in the song's name or its artists': for songs shown
    /// under their album.
    pub fn song(&self, track: &TrackRef) -> bool {
        self.matches(&song_names(track))
    }
}

fn song_names(track: &TrackRef) -> Vec<&str> {
    let mut names = vec![track.title.as_str()];
    names.extend(track.artists.iter().map(|artist| artist.name.as_str()));
    names
}

/// Lowercase, without accents.
fn fold(text: &str) -> String {
    text.nfkd()
        .filter(|&c| !is_combining_mark(c))
        .flat_map(char::to_lowercase)
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub enum Loadable<T> {
    Loading,
    Ready(T),
    Failed(String),
}

pub struct Browse {
    pub open: bool,
    pub shown: BrowseTab,
    /// The Spotify playlist YOUR SPOTIFY shows, or Liked Songs, by URI.
    pub spotify_open: Option<String>,
    /// The current page is the last.
    history: Vec<Page>,
    pub searches: HashMap<String, Loadable<SearchResults>>,
    pub artists: HashMap<String, Loadable<Discography>>,
    /// Artists like an artist, by its URI: names from Last.fm.
    pub similar_artists: HashMap<String, Loadable<Vec<String>>>,
    /// The songs of albums and of Spotify playlists, by URI.
    pub albums: HashMap<String, Loadable<Vec<TrackRef>>>,
    /// Where the songs read of Spotify playlists and Liked Songs are
    /// there, by URI: the revision read, and each song's place, in the
    /// order of `albums`' songs.
    pub places: HashMap<String, (Vec<u8>, Vec<Place>)>,
    /// The account's own Spotify playlists, Liked Songs first; None until
    /// asked for.
    pub spotify_playlists: Option<Loadable<Vec<SpotifyPlaylist>>>,
    /// The songs or playlist removed last, while they can be put back.
    pub undo: Option<Undo>,
    /// Albums and Spotify playlists shown with their songs.
    pub expanded: HashSet<String>,
    pub tab: ReleaseTab,
    /// The artist page shows similar artists instead of the releases.
    pub similar: bool,
    /// The artist page shows its popular songs instead of the releases.
    pub popular: bool,
    /// A similar artist being looked for on Spotify, and the page it was
    /// opened from.
    pub finding: Option<(String, Page)>,
    pub newest_first: bool,
    /// Artists opened lately, newest first.
    pub recent: Vec<ArtistRef>,
    /// Searches made lately, newest first.
    pub recent_searches: Vec<String>,
    /// The playlist + ADD adds to.
    pub target: PlaylistId,
    /// Entries just added, and when.
    pub flash: Vec<(EntryId, Instant)>,
    /// Covers show next to albums.
    pub show_covers: bool,
    /// The page's find is open, with what is typed in it so far. A new
    /// page closes it.
    pub find: Option<String>,
}

impl Browse {
    pub fn new(target: PlaylistId, show_covers: bool) -> Self {
        Self {
            open: false,
            shown: BrowseTab::default(),
            spotify_open: None,
            history: vec![Page::Home],
            searches: HashMap::new(),
            artists: HashMap::new(),
            similar_artists: HashMap::new(),
            albums: HashMap::new(),
            places: HashMap::new(),
            spotify_playlists: None,
            undo: None,
            expanded: HashSet::new(),
            tab: ReleaseTab::default(),
            similar: false,
            popular: false,
            finding: None,
            newest_first: false,
            recent: Vec::new(),
            recent_searches: Vec::new(),
            target,
            flash: Vec::new(),
            show_covers,
            find: None,
        }
    }

    /// The discography of the artist whose page is shown, once read.
    pub fn discography(&self) -> Option<&Discography> {
        let Page::Artist(uri) = self.page() else {
            return None;
        };
        match self.artists.get(uri) {
            Some(Loadable::Ready(discography)) => Some(discography),
            _ => None,
        }
    }

    /// The artist's page shows its popular songs, in place of the releases.
    pub fn popular_shown(&self) -> bool {
        self.popular && !self.similar && self.discography().is_some_and(|d| !d.popular.is_empty())
    }

    /// The artist page's releases on its tab, in the order shown.
    pub fn releases_shown(&self) -> Vec<&Release> {
        let Some(discography) = self.discography() else {
            return Vec::new();
        };
        let mut releases: Vec<&Release> = (discography.releases.iter())
            .filter(|release| self.tab.includes(release.group))
            .collect();
        if self.newest_first {
            releases.reverse();
        }
        releases
    }

    /// What a find shows of a release: None when nothing of it has the
    /// find; else the songs that have it, by their place on it, none when
    /// only its title does. The release is the row: its songs go by their
    /// own names.
    pub fn found_in(&self, release: &Release, find: &Find) -> Option<Vec<(usize, &TrackRef)>> {
        let found: Vec<_> = match self.albums.get(&release.uri) {
            Some(Loadable::Ready(tracks)) => (tracks.iter().enumerate())
                .filter(|(_, track)| find.song(track))
                .collect(),
            _ => Vec::new(),
        };
        (!found.is_empty() || find.matches(&[&release.title])).then_some(found)
    }

    /// What ADD ALL adds on the artist's page shown; None off one, with its
    /// similar artists shown, or with nothing shown to add.
    pub fn add_all(&self) -> Option<AddAll> {
        let discography = self.discography()?;
        if self.similar {
            return None;
        }
        let find = self.finder();
        // A song of an album's comes from that album, as when added alone.
        let from_album = |track: &TrackRef| {
            let mut track = track.clone();
            track.source = Some(Source::album(&track.album));
            track
        };
        let all = if self.popular_shown() {
            AddAll::Songs(
                (discography.popular.iter())
                    .filter(|track| find.as_ref().is_none_or(|find| find.track(track)))
                    .cloned()
                    .collect(),
            )
        } else if let Some(find) = &find {
            let mut songs = Vec::new();
            for release in self.releases_shown() {
                match self.found_in(release, find) {
                    None => {}
                    Some(found) if !found.is_empty() => {
                        songs.extend(found.into_iter().map(|(_, track)| from_album(track)));
                    }
                    Some(_) => match self.albums.get(&release.uri) {
                        Some(Loadable::Ready(tracks)) => {
                            songs.extend(tracks.iter().map(from_album))
                        }
                        Some(Loadable::Failed(_)) => {}
                        None | Some(Loadable::Loading) => return Some(AddAll::Reading),
                    },
                }
            }
            AddAll::Songs(songs)
        } else {
            AddAll::Releases(
                (self.releases_shown().into_iter())
                    .map(|release| release.uri.clone())
                    .collect(),
            )
        };
        match &all {
            AddAll::Releases(none) if none.is_empty() => None,
            AddAll::Songs(none) if none.is_empty() => None,
            _ => Some(all),
        }
    }

    /// What the page shown is narrowed to, while something is typed in
    /// its find.
    pub fn finder(&self) -> Option<Find> {
        self.find
            .as_deref()
            .map(Find::new)
            .filter(|find| !find.is_empty())
    }

    pub fn page(&self) -> &Page {
        self.history.last().unwrap_or(&Page::Home)
    }

    pub fn can_go_back(&self) -> bool {
        self.history.len() > 1
    }

    pub fn back(&mut self) {
        if self.can_go_back() {
            self.history.pop();
            self.find = None;
        }
    }

    /// Opens `page` on top of the history. An artist page starts on its
    /// Albums tab, oldest first.
    pub fn go(&mut self, page: Page) {
        if *self.page() == page {
            return;
        }
        if matches!(page, Page::Artist(_)) {
            self.tab = ReleaseTab::default();
            self.similar = false;
            self.popular = false;
            self.newest_first = false;
        }
        self.history.push(page);
        self.find = None;
    }

    /// Puts an artist first among the recent ones, keeping the picture
    /// it had if this visit came without one.
    pub fn visited(&mut self, mut artist: ArtistRef) {
        if let Some(at) = self.recent.iter().position(|a| a.uri == artist.uri) {
            let before = self.recent.remove(at);
            artist.image = artist.image.or(before.image);
        }
        self.recent.insert(0, artist);
        self.recent.truncate(RECENT_LIMIT);
    }

    pub fn searched(&mut self, query: &str) {
        self.recent_searches.retain(|q| q != query);
        self.recent_searches.insert(0, query.to_string());
        self.recent_searches.truncate(SEARCHES_LIMIT);
    }

    pub fn forget_search(&mut self, query: &str) {
        self.recent_searches.retain(|q| q != query);
    }

    /// One of the account's Spotify playlists, or Liked Songs, by URI.
    pub fn spotify_playlist(&self, uri: &str) -> Option<&SpotifyPlaylist> {
        match &self.spotify_playlists {
            Some(Loadable::Ready(playlists)) => playlists.iter().find(|p| p.uri == uri),
            _ => None,
        }
    }

    /// Whether songs can be removed from this Spotify playlist: Liked
    /// Songs and the account's own can, others' it follows can't.
    pub fn editable(&self, uri: &str) -> bool {
        self.spotify_playlist(uri)
            .is_some_and(|playlist| playlist.owner.is_none())
    }

    /// Whether this Spotify playlist can be taken out of the account's
    /// list: any but Liked Songs.
    pub fn unlistable(&self, uri: &str) -> bool {
        uri != crate::spotify::catalog::LIKED_SONGS && self.spotify_playlist(uri).is_some()
    }

    pub fn flash(&mut self, entries: impl IntoIterator<Item = EntryId>) {
        let now = Instant::now();
        self.flash.retain(|(_, at)| at.elapsed() < FLASH);
        self.flash.extend(entries.into_iter().map(|id| (id, now)));
    }

    /// How lit an entry is, 1 when just added, fading to 0.
    pub fn glow(&self, entry: EntryId) -> f32 {
        self.flash
            .iter()
            .find(|(id, _)| *id == entry)
            .map_or(0.0, |(_, at)| {
                1.0 - (at.elapsed().as_secs_f32() / FLASH.as_secs_f32()).min(1.0)
            })
    }

    pub fn flashing(&self) -> bool {
        self.flash.iter().any(|(_, at)| at.elapsed() < FLASH)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_all_adds_what_the_artist_page_shows() {
        let mut browse = Browse::new(PlaylistId::DEFAULT, false);
        let discography = crate::demo::discography();
        let uri = discography.artist.uri.clone();
        browse
            .artists
            .insert(uri.clone(), Loadable::Ready(discography.clone()));
        browse.go(Page::Artist(uri));
        let of_tab: Vec<String> = (discography.releases.iter())
            .filter(|r| browse.tab.includes(r.group))
            .map(|r| r.uri.clone())
            .collect();
        assert_eq!(browse.add_all(), Some(AddAll::Releases(of_tab.clone())));
        browse.newest_first = true;
        let newest: Vec<String> = of_tab.into_iter().rev().collect();
        assert_eq!(browse.add_all(), Some(AddAll::Releases(newest)));

        browse.popular = true;
        assert_eq!(
            browse.add_all(),
            Some(AddAll::Songs(discography.popular.clone()))
        );
        browse.popular = false;

        // A find: a release found by its title waits for its songs, then
        // goes whole; one found by a song gives that song.
        browse.find = Some("static".into());
        assert_eq!(browse.add_all(), Some(AddAll::Reading));
        for (album, songs) in crate::demo::release_songs() {
            browse.albums.insert(album, Loadable::Ready(songs));
        }
        let Some(AddAll::Songs(songs)) = browse.add_all() else {
            panic!("the songs found");
        };
        let titles: Vec<&str> = songs.iter().map(|s| s.title.as_str()).collect();
        // Newest first: Everything Is Static has no songs in the demo.
        assert_eq!(titles, ["Static Rain"]);
        assert!(songs.iter().all(|s| s.source.is_some()));
        browse.find = Some("nothing like it".into());
        assert_eq!(browse.add_all(), None);
    }

    #[test]
    fn a_find_wants_every_word_in_the_names_whatever_the_case_and_accents() {
        let find = Find::new("  beyonce   HALO ");
        assert!(find.matches(&["Halo", "Beyoncé"]));
        assert!(!find.matches(&["Halo", "Leona Lewis"]));
        assert!(Find::new("bey").matches(&["BEYONCÉ"]));
        assert!(Find::new(" ").is_empty());
    }

    #[test]
    fn history_goes_back_to_home_and_no_further() {
        let mut browse = Browse::new(PlaylistId::DEFAULT, false);
        browse.go(Page::Search("boc".into()));
        browse.go(Page::Artist("spotify:artist:a".into()));
        browse.go(Page::Artist("spotify:artist:a".into()));
        browse.back();
        assert_eq!(*browse.page(), Page::Search("boc".into()));
        browse.back();
        browse.back();
        assert_eq!(*browse.page(), Page::Home);
    }

    #[test]
    fn an_artist_page_starts_on_albums_oldest_first() {
        let mut browse = Browse::new(PlaylistId::DEFAULT, false);
        browse.tab = ReleaseTab::All;
        browse.similar = true;
        browse.popular = true;
        browse.newest_first = true;
        browse.go(Page::Artist("spotify:artist:a".into()));
        assert_eq!(browse.tab, ReleaseTab::Albums);
        assert!(!browse.similar && !browse.popular);
        assert!(!browse.newest_first);
    }

    #[test]
    fn a_revisited_artist_keeps_its_picture() {
        let mut browse = Browse::new(PlaylistId::DEFAULT, false);
        let artist = |uri: &str, image: Option<&str>| ArtistRef {
            uri: uri.into(),
            name: uri.into(),
            image: image.map(String::from),
        };
        browse.visited(artist("a", Some("https://i.scdn.co/image/a")));
        browse.visited(artist("b", None));
        browse.visited(artist("a", None));
        assert_eq!(browse.recent.len(), 2);
        assert_eq!(browse.recent[0].uri, "a");
        assert_eq!(
            browse.recent[0].image.as_deref(),
            Some("https://i.scdn.co/image/a")
        );
    }

    #[test]
    fn searches_are_kept_newest_first_once_each() {
        let mut browse = Browse::new(PlaylistId::DEFAULT, false);
        for query in ["a", "b", "a"] {
            browse.searched(query);
        }
        assert_eq!(browse.recent_searches, ["a", "b"]);
        for i in 0..SEARCHES_LIMIT {
            browse.searched(&i.to_string());
        }
        assert_eq!(browse.recent_searches.len(), SEARCHES_LIMIT);
        assert_eq!(browse.recent_searches[0], (SEARCHES_LIMIT - 1).to_string());
        browse.forget_search("7");
        assert!(!browse.recent_searches.contains(&"7".to_string()));
    }
}
