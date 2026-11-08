//! Browse: search, artists, albums and adding from them.

use super::*;

#[test]
fn an_album_dropped_before_it_is_read_lands_where_it_was_dropped() {
    let mut rig = Rig::new("drop");
    rig.controller
        .apply(BrowseAction::AddTracks(tracks([1, 2])));
    rig.controller
        .apply(BrowseAction::DropAlbum("spotify:album:a".into(), 1));
    assert!(
        rig.sent()
            .iter()
            .any(|c| matches!(c, Command::Album(a) if a == "spotify:album:a"))
    );
    assert_eq!(rig.songs(), ["Song 1", "Song 2"]);
    rig.tell(Event::Album {
        album: "spotify:album:a".into(),
        result: Ok(songs(tracks([7, 8]))),
    });
    assert_eq!(rig.songs(), ["Song 1", "Song 7", "Song 8", "Song 2"]);
}

#[test]
fn songs_added_from_an_album_or_a_spotify_playlist_keep_where_from() {
    let mut rig = Rig::new("sources");
    rig.tell(Event::Session(SessionState::Ready { user: "u".into() }));
    rig.tell(Event::SpotifyPlaylists(Ok(vec![SpotifyPlaylist {
        uri: "spotify:playlist:late".into(),
        name: "Late Trains".into(),
        tracks: 2,
        owner: None,
        private: false,
    }])));
    let on_album = |ids: [u64; 2]| {
        let mut tracks = tracks(ids);
        for track in &mut tracks {
            track.album = AlbumRef {
                uri: "spotify:album:a".into(),
                name: "Afterhours".into(),
            };
        }
        tracks
    };
    for (uri, read) in [
        ("spotify:album:a", on_album([1, 2])),
        ("spotify:playlist:late", tracks([3, 4])),
    ] {
        rig.controller.apply(BrowseAction::AddAlbum(uri.into()));
        rig.tell(Event::Album {
            album: uri.into(),
            result: Ok(songs(read)),
        });
    }
    let state = rig.controller.state();
    let sources: Vec<_> = state
        .library
        .get(state.viewed)
        .unwrap()
        .entries()
        .iter()
        .map(|e| {
            e.track
                .source
                .as_ref()
                .map(|s| (s.uri.as_str(), s.name.as_str()))
        })
        .collect();
    let album = Some(("spotify:album:a", "Afterhours"));
    let late = Some(("spotify:playlist:late", "Late Trains"));
    assert_eq!(sources, [album, album, late, late]);
    // Kept when copied to another playlist.
    let copied: Vec<_> = state.library.get(state.viewed).unwrap().entries()[2..]
        .iter()
        .map(|e| e.track.clone())
        .collect();
    rig.controller
        .apply(PlaylistsAction::NewPlaylistWith(copied));
    let state = rig.controller.state();
    let list = state.library.get(state.viewed).unwrap();
    assert!(
        list.entries()
            .iter()
            .all(|e| e.track.source.as_ref().map(|s| s.name.as_str()) == Some("Late Trains"))
    );
}

#[test]
fn albums_added_together_arrive_together_in_their_order_whenever_read() {
    let mut rig = Rig::new("add-albums");
    let viewed = rig.controller.state().viewed;
    let uris = |rig: &Rig| -> Vec<String> {
        let list = rig.controller.state().library.get(viewed).unwrap();
        list.entries().iter().map(|e| e.track.uri.clone()).collect()
    };
    let before = uris(&rig);
    let albums = ["spotify:album:a", "spotify:album:b", "spotify:album:c"];
    rig.controller
        .apply(BrowseAction::AddAlbums(albums.map(String::from).to_vec()));
    // Read last first, one failing: nothing until all are answered.
    rig.tell(Event::Album {
        album: albums[2].into(),
        result: Ok(songs(tracks([5, 6]))),
    });
    rig.tell(Event::Album {
        album: albums[1].into(),
        result: Err("gone".into()),
    });
    assert_eq!(uris(&rig), before);
    rig.tell(Event::Album {
        album: albums[0].into(),
        result: Ok(songs(tracks([1, 2]))),
    });
    let added: Vec<String> = tracks([1, 2, 5, 6]).into_iter().map(|t| t.uri).collect();
    assert_eq!(uris(&rig)[before.len()..], added);
    assert_eq!(
        rig.controller.state().error.as_deref(),
        Some("One of the releases couldn't be read.")
    );
}

#[test]
fn a_find_on_an_artist_page_reads_its_tabs_songs_and_a_new_page_closes_it() {
    use crate::model::catalog::{ReleaseGroup, ReleaseTab};
    let mut rig = Rig::new("find");
    let discography = crate::demo::discography();
    let uri = discography.artist.uri.clone();
    let albums = |rig: &mut Rig| -> Vec<String> {
        rig.sent()
            .into_iter()
            .filter_map(|c| match c {
                Command::Album(album) => Some(album),
                _ => None,
            })
            .collect()
    };
    rig.controller
        .apply(BrowseAction::OpenArtist(discography.artist.clone()));
    // Opened before the discography is read: read once it is.
    rig.controller
        .apply(BrowseAction::Find(Some(String::new())));
    assert!(albums(&mut rig).is_empty());
    rig.tell(Event::Discography {
        artist: uri.clone(),
        result: Ok(discography.clone()),
    });
    let tab = rig.controller.state().browse.tab;
    let of_tab: Vec<String> = discography
        .releases
        .iter()
        .filter(|r| tab.includes(r.group))
        .map(|r| r.uri.clone())
        .collect();
    assert_eq!(albums(&mut rig), of_tab);
    // Another tab reads its own, those not read already.
    rig.controller
        .apply(BrowseAction::SetTab(ReleaseTab::SinglesAndEps));
    let singles = albums(&mut rig);
    assert!(!singles.is_empty());
    assert!(singles.iter().all(|album| {
        discography
            .releases
            .iter()
            .any(|r| r.uri == *album && r.group == ReleaseGroup::Single)
    }));
    rig.controller.apply(BrowseAction::Search("x".into()));
    assert_eq!(rig.controller.state().browse.find, None);
}

#[test]
fn a_similar_artist_opens_once_found_unless_the_user_moved_on() {
    let mut rig = Rig::new("similar");
    let artist = |name: &str| ArtistRef {
        uri: format!("spotify:artist:{name}"),
        name: name.into(),
        image: None,
    };
    let page = |rig: &Rig| rig.controller.state().browse.page().clone();
    rig.controller
        .apply(BrowseAction::OpenArtist(artist("boc")));
    rig.controller
        .apply(BrowseAction::OpenSimilar("Tycho".into()));
    assert!(
        rig.sent()
            .iter()
            .any(|c| matches!(c, Command::FindArtist(n) if n == "Tycho"))
    );
    rig.tell(Event::ArtistFound {
        name: "Tycho".into(),
        result: Ok(Some(artist("tycho"))),
    });
    assert_eq!(page(&rig), Page::Artist("spotify:artist:tycho".into()));

    rig.controller
        .apply(BrowseAction::OpenSimilar("Bibio".into()));
    rig.controller.apply(BrowseAction::Back);
    rig.tell(Event::ArtistFound {
        name: "Bibio".into(),
        result: Ok(Some(artist("bibio"))),
    });
    assert_eq!(page(&rig), Page::Artist("spotify:artist:boc".into()));

    // Spotify has nobody by that name: search for it instead.
    rig.controller
        .apply(BrowseAction::OpenSimilar("Ulrich Schnauss".into()));
    rig.tell(Event::ArtistFound {
        name: "Ulrich Schnauss".into(),
        result: Ok(None),
    });
    assert_eq!(page(&rig), Page::Search("Ulrich Schnauss".into()));
}

#[test]
fn more_songs_reads_the_next_page_once_and_adds_the_new_ones() {
    use crate::model::catalog::SearchResults;
    use crate::spotify::webapi::SearchPage;
    let mut rig = Rig::new("more-songs");
    rig.controller.apply(BrowseAction::Search("boards".into()));
    rig.sent();
    rig.tell(Event::SearchResults {
        query: "boards".into(),
        results: Ok(SearchResults {
            songs: tracks([1, 2]),
            songs_read: 10,
            more_songs: true,
            ..SearchResults::default()
        }),
    });
    rig.controller
        .apply(BrowseAction::MoreSongs("boards".into()));
    // Pressed again while it reads: asked once.
    rig.controller
        .apply(BrowseAction::MoreSongs("boards".into()));
    let more: Vec<_> = rig
        .sent()
        .into_iter()
        .filter(|c| matches!(c, Command::MoreSongs { .. }))
        .collect();
    assert!(matches!(&more[..], [Command::MoreSongs { query, offset: 10 }] if query == "boards"));
    rig.tell(Event::MoreSongs {
        query: "boards".into(),
        result: Ok(SearchPage {
            items: tracks([2, 3]),
            read: 10,
            more: false,
        }),
    });
    let Some(Loadable::Ready(results)) = rig.controller.state().browse.searches.get("boards")
    else {
        panic!("the search's results");
    };
    assert_eq!(results.songs, tracks([1, 2, 3]));
    assert_eq!(results.songs_read, 20);
    assert!(!results.more_songs && !results.reading_songs);
    rig.controller
        .apply(BrowseAction::MoreSongs("boards".into()));
    assert!(rig.sent().is_empty());
}

#[test]
fn more_artists_shows_those_read_then_reads_the_next_page_once() {
    use crate::model::catalog::SearchResults;
    use crate::spotify::webapi::SearchPage;
    let named = |names: &[&str]| -> Vec<ArtistRef> {
        (names.iter())
            .map(|name| ArtistRef {
                uri: format!("spotify:artist:{name}"),
                name: (*name).into(),
                image: None,
            })
            .collect()
    };
    let shown = |rig: &Rig| {
        let Some(Loadable::Ready(results)) = rig.controller.state().browse.searches.get("boards")
        else {
            panic!("the search's results");
        };
        (results.shown_artists().to_vec(), results.has_more_artists())
    };
    let mut rig = Rig::new("more-artists");
    rig.controller.apply(BrowseAction::Search("boards".into()));
    rig.sent();
    rig.tell(Event::SearchResults {
        query: "boards".into(),
        results: Ok(SearchResults {
            artists: named(&["a", "b", "c"]),
            artists_shown: 2,
            artists_read: 10,
            more_artists: true,
            ..SearchResults::default()
        }),
    });
    assert_eq!(shown(&rig), (named(&["a", "b"]), true));

    // The ones already read show without asking Spotify.
    rig.controller
        .apply(BrowseAction::MoreArtists("boards".into()));
    assert!(rig.sent().is_empty());
    assert_eq!(shown(&rig), (named(&["a", "b", "c"]), true));

    // Then Spotify's next page, asked for once while it reads.
    rig.controller
        .apply(BrowseAction::MoreArtists("boards".into()));
    rig.controller
        .apply(BrowseAction::MoreArtists("boards".into()));
    let more: Vec<_> = rig
        .sent()
        .into_iter()
        .filter(|c| matches!(c, Command::MoreArtists { .. }))
        .collect();
    assert!(matches!(&more[..], [Command::MoreArtists { query, offset: 10 }] if query == "boards"));
    rig.tell(Event::MoreArtists {
        query: "boards".into(),
        result: Ok(SearchPage {
            items: named(&["c", "d"]),
            read: 10,
            more: false,
        }),
    });
    assert_eq!(shown(&rig), (named(&["a", "b", "c", "d"]), false));
    rig.controller
        .apply(BrowseAction::MoreArtists("boards".into()));
    assert!(rig.sent().is_empty());
}

#[test]
fn searches_are_remembered_across_starts_but_links_are_not() {
    let mut rig = Rig::new("history");
    rig.controller
        .apply(BrowseAction::Search(" boards ".into()));
    rig.controller
        .apply(BrowseAction::Search("autechre".into()));
    rig.controller.apply(BrowseAction::Search(
        "spotify:track:4uLU6hMCjMI75M1A2tKUQC".into(),
    ));
    assert!(
        rig.sent()
            .iter()
            .any(|c| matches!(c, Command::Search(q) if q == "boards"))
    );
    rig.controller
        .apply(BrowseAction::ForgetSearch(Some("autechre".into())));
    rig.controller.apply(BrowseAction::Search("aphex".into()));
    rig.controller.save();
    let history = store::load_history(&rig.controller.paths);
    assert_eq!(history.searches, ["aphex", "boards"]);
    let (backend, _, _) = Backend::detached();
    let paths = rig.controller.paths.clone();
    let settings = Settings::load(&paths);
    let again = Controller::with_backend(paths, settings, Arc::default(), Ok(backend));
    assert_eq!(again.state().browse.recent_searches, ["aphex", "boards"]);
}

#[test]
fn a_spotify_code_opens_what_it_points_to_in_browse() {
    use crate::spotify::code::CodeTarget;
    let mut rig = Rig::new("code");
    let artist = ArtistRef {
        uri: "spotify:artist:boc".into(),
        name: "Boards of Canada".into(),
        image: None,
    };
    rig.controller
        .apply(BrowseAction::ReadCode(crate::spotify::CodeImage::File(vec![1])));
    assert!(
        (rig.sent().iter()).any(|c| matches!(c, Command::ReadCode(_))),
        "the picture goes to the worker"
    );

    // A song: its artist's page, on every release, its album open.
    let bars = crate::spotify::code::encode(57268659651);
    rig.tell(Event::CodeRead(Ok((
        bars,
        CodeTarget::Track {
            artist: artist.clone(),
            album: "spotify:album:geogaddi".into(),
            track: "spotify:track:dawn".into(),
        },
    ))));
    let browse = &rig.controller.state().browse;
    assert!(browse.open && browse.shown == crate::browse::BrowseTab::Search);
    assert_eq!(*browse.page(), Page::Artist("spotify:artist:boc".into()));
    assert_eq!(browse.tab, crate::model::catalog::ReleaseTab::All);
    assert!(browse.expanded.contains("spotify:album:geogaddi"));
    assert_eq!(browse.reveal.as_deref(), Some("spotify:track:dawn"));
    // The code is drawn there, with what it points to.
    let code = browse.code_here().expect("the code shows on the page it opened");
    assert_eq!(code.bars, bars);
    assert_eq!(code.uri, "spotify:track:dawn");
    let sent = rig.sent();
    assert!(sent.iter().any(|c| matches!(c, Command::Discography(a) if a == "spotify:artist:boc")));
    assert!(sent.iter().any(|c| matches!(c, Command::Album(a) if a == "spotify:album:geogaddi")));
    rig.controller.apply(BrowseAction::Revealed);
    assert_eq!(rig.controller.state().browse.reveal, None);

    // A playlist: its own page, its songs read, added under its name.
    let playlist = SpotifyPlaylist {
        uri: "spotify:playlist:p".into(),
        name: "Late Night Drive".into(),
        tracks: 2,
        owner: Some("Max".into()),
        private: false,
    };
    rig.tell(Event::CodeRead(Ok((bars, CodeTarget::Playlist(playlist)))));
    let browse = &rig.controller.state().browse;
    assert_eq!(*browse.page(), Page::Playlist("spotify:playlist:p".into()));
    assert_eq!(
        browse.code_here().map(|c| c.uri.as_str()),
        Some("spotify:playlist:p")
    );
    assert!(
        (rig.sent().iter()).any(|c| matches!(c, Command::Album(a) if a == "spotify:playlist:p"))
    );
    rig.tell(Event::Album {
        album: "spotify:playlist:p".into(),
        result: Ok(songs(tracks([1, 2]))),
    });
    rig.controller
        .apply(BrowseAction::AddAlbum("spotify:playlist:p".into()));
    let state = rig.controller.state();
    let list = state.library.get(state.browse.target).unwrap();
    assert!(list.entries().iter().all(|e| {
        e.track.source.as_ref().map(|s| s.name.as_str()) == Some("Late Night Drive")
    }));

    // One of the account's own: its songs stay while its page shows,
    // though the account's playlists are read again.
    let own = SpotifyPlaylist {
        uri: "spotify:playlist:p".into(),
        name: "Late Night Drive".into(),
        tracks: 2,
        owner: None,
        private: false,
    };
    rig.tell(Event::SpotifyPlaylists(Ok(vec![own.clone()])));
    let albums = &rig.controller.state().browse.albums;
    assert!(matches!(albums.get("spotify:playlist:p"), Some(Loadable::Ready(_))));
    // Let go once it's out of sight, they're read again on coming back.
    rig.controller.apply(BrowseAction::OpenArtist(artist.clone()));
    rig.tell(Event::SpotifyPlaylists(Ok(vec![own])));
    assert_eq!(rig.controller.state().browse.albums.get("spotify:playlist:p"), None);
    rig.sent();
    rig.controller.apply(BrowseAction::Back);
    assert!(
        (rig.sent().iter()).any(|c| matches!(c, Command::Album(a) if a == "spotify:playlist:p"))
    );

    // Elsewhere, it isn't.
    rig.controller.apply(BrowseAction::Back);
    let browse = &rig.controller.state().browse;
    assert_eq!(*browse.page(), Page::Artist("spotify:artist:boc".into()));
    assert_eq!(browse.code_here(), None);

    // A code the camera read goes straight to the lookup.
    rig.controller.apply(BrowseAction::LookUpCode(16862907074));
    assert!((rig.sent().iter()).any(|c| matches!(c, Command::LookUpCode(16862907074))));

    // A link opens the same way, with no code drawn.
    rig.controller.apply(BrowseAction::OpenLink(
        " https://open.spotify.com/artist/boc ".into(),
    ));
    assert!(
        (rig.sent().iter())
            .any(|c| matches!(c, Command::ReadLink(l) if l == "https://open.spotify.com/artist/boc"))
    );
    rig.tell(Event::LinkRead(Ok(CodeTarget::Artist(artist.clone()))));
    let browse = &rig.controller.state().browse;
    assert_eq!(*browse.page(), Page::Artist("spotify:artist:boc".into()));
    assert_eq!(browse.code_here(), None);

    // Not a code: said, nothing opened.
    rig.tell(Event::CodeRead(Err("No Spotify Code found".into())));
    assert_eq!(
        rig.controller.state().error.as_deref(),
        Some("No Spotify Code found")
    );
}
