//! The catalog view's state: where it is, its back history, and what has
//! been read from Spotify so far.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crate::model::catalog::{Discography, ReleaseTab, SearchResults};
use crate::model::{ArtistRef, EntryId, PlaylistId, TrackRef};

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

#[derive(Clone, Debug, PartialEq)]
pub enum Loadable<T> {
    Loading,
    Ready(T),
    Failed(String),
}

pub struct Browse {
    pub open: bool,
    /// The current page is the last.
    history: Vec<Page>,
    pub searches: HashMap<String, Loadable<SearchResults>>,
    pub artists: HashMap<String, Loadable<Discography>>,
    /// Artists like an artist, by its URI: names from Last.fm.
    pub similar_artists: HashMap<String, Loadable<Vec<String>>>,
    pub albums: HashMap<String, Loadable<Vec<TrackRef>>>,
    /// Albums shown with their songs.
    pub expanded: HashSet<String>,
    pub tab: ReleaseTab,
    /// The artist page shows similar artists instead of the releases.
    pub similar: bool,
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
}

impl Browse {
    pub fn new(target: PlaylistId, show_covers: bool) -> Self {
        Self {
            open: false,
            history: vec![Page::Home],
            searches: HashMap::new(),
            artists: HashMap::new(),
            similar_artists: HashMap::new(),
            albums: HashMap::new(),
            expanded: HashSet::new(),
            tab: ReleaseTab::default(),
            similar: false,
            finding: None,
            newest_first: false,
            recent: Vec::new(),
            recent_searches: Vec::new(),
            target,
            flash: Vec::new(),
            show_covers,
        }
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
            self.newest_first = false;
        }
        self.history.push(page);
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
        browse.newest_first = true;
        browse.go(Page::Artist("spotify:artist:a".into()));
        assert_eq!(browse.tab, ReleaseTab::Albums);
        assert!(!browse.similar);
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
