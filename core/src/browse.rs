//! The catalog view's state: where it is, its back history, and what has
//! been read from Spotify so far.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crate::model::catalog::{Discography, ReleaseTab, SearchResults};
use crate::model::{ArtistRef, EntryId, PlaylistId, TrackRef};

/// How long rows just added light up in the slim column.
pub const FLASH: Duration = Duration::from_millis(1200);
const RECENT_LIMIT: usize = 8;

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
    pub albums: HashMap<String, Loadable<Vec<TrackRef>>>,
    /// Albums shown with their songs.
    pub expanded: HashSet<String>,
    pub tab: ReleaseTab,
    pub newest_first: bool,
    /// Artists opened lately, newest first.
    pub recent: Vec<ArtistRef>,
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
            albums: HashMap::new(),
            expanded: HashSet::new(),
            tab: ReleaseTab::default(),
            newest_first: false,
            recent: Vec::new(),
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
            self.newest_first = false;
        }
        self.history.push(page);
    }

    pub fn visited(&mut self, artist: ArtistRef) {
        self.recent.retain(|a| a.uri != artist.uri);
        self.recent.insert(0, artist);
        self.recent.truncate(RECENT_LIMIT);
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
        browse.newest_first = true;
        browse.go(Page::Artist("spotify:artist:a".into()));
        assert_eq!(browse.tab, ReleaseTab::Albums);
        assert!(!browse.newest_first);
    }
}
