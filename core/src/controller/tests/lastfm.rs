//! Songs loved on Last.fm.

use super::*;

#[test]
fn loved_songs_land_where_they_were_dropped_once_found() {
    let mut rig = Rig::new("loved");
    rig.controller.apply(Action::AddTracks(tracks([1, 2])));
    let loved = |title: &str| LovedSong {
        artist: "A".into(),
        title: title.into(),
        date: 0,
    };
    rig.controller.apply(Action::DropLoved(
        vec![loved("Song 7"), loved("Gone"), loved("Song 8")],
        1,
    ));
    let id = rig
        .sent()
        .into_iter()
        .find_map(|c| match c {
            Command::FindSongs { id, songs } if songs.len() == 3 => Some(id),
            _ => None,
        })
        .expect("asked for");
    // An answer to something else changes nothing.
    rig.tell(Event::SongsFound {
        id: id + 1,
        result: Ok(vec![Some(entry(9).track)]),
    });
    assert_eq!(rig.songs(), ["Song 1", "Song 2"]);
    let [seven, eight] = [7, 8].map(|id| Some(entry(id).track));
    rig.tell(Event::SongsFound {
        id,
        result: Ok(vec![seven, None, eight]),
    });
    assert_eq!(rig.songs(), ["Song 1", "Song 7", "Song 8", "Song 2"]);
    let notice = rig.controller.state().notice.clone().unwrap_or_default();
    assert!(
        notice.ends_with("1 of the 3 songs isn't on Spotify"),
        "{notice}"
    );
    // Told twice, it's added once.
    rig.tell(Event::SongsFound {
        id,
        result: Ok(vec![Some(entry(9).track)]),
    });
    assert_eq!(rig.songs().len(), 4);
}
