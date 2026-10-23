//! Plays the playlists on the engine.
//!
//! Each song is loaded on its own, and the song after it is handed to the
//! engine to follow it: the engine preloads that one and starts it the
//! moment the playing one ends, without a gap. Whenever what comes next
//! changes (an edit, a mark, shuffle), the transport hands over the new one.
//!
//! The transport never talks to the engine. Every method edits the model
//! and returns the [`Request`]s that make playback match, in order.

use crate::model::{EntryId, Library, Playback, Playlist, PlaylistId, Step};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// Plays this song now, from `position_ms`, following it with nothing
    /// until told otherwise.
    Load {
        entry: EntryId,
        uri: String,
        position_ms: u32,
    },
    /// The song to start when the playing one ends. None: playback stops.
    Follow(Option<(EntryId, String)>),
    Resume,
    Pause,
    /// The playing song from its start.
    Restart,
    Stop,
}

/// Unplayable songs skipped in a row before playback gives up.
const SKIP_LIMIT: usize = 10;

#[derive(Debug)]
pub struct Transport {
    pub playback: Playback,
    /// A song is loaded: playing, paused or loading.
    active: bool,
    /// The song handed to the engine to follow the playing one. `None`
    /// when unknown: right after the engine moved on by itself, a Follow
    /// sent meanwhile may have reached it late, so the next one is always
    /// sent.
    followed: Option<Option<EntryId>>,
    skips: usize,
}

impl Transport {
    pub fn new(playback: Playback) -> Self {
        Self {
            playback,
            active: false,
            followed: Some(None),
            skips: 0,
        }
    }

    pub fn active(&self) -> bool {
        self.active
    }

    /// The playlist Next and Previous act on: the playing one while a song
    /// is loaded, the viewed one when stopped.
    fn target(&self, viewed: PlaylistId) -> PlaylistId {
        match self.playback.playing() {
            Some(playing) if self.active => playing,
            _ => viewed,
        }
    }

    /// Double-click or Enter on a row.
    pub fn play_entry(
        &mut self,
        library: &mut Library,
        list: PlaylistId,
        entry: EntryId,
        position_ms: u32,
    ) -> Vec<Request> {
        self.remember_position(library, position_ms);
        let Some(playlist) = library.get_mut(list) else {
            return Vec::new();
        };
        match self.playback.start(playlist, Some(entry)) {
            Some(id) => {
                self.skips = 0;
                self.load(library, id, 0)
            }
            None => Vec::new(),
        }
    }

    /// The Play button: resumes a paused song, restarts a playing one, or
    /// starts the viewed playlist where its cursor is.
    pub fn play(
        &mut self,
        library: &mut Library,
        viewed: PlaylistId,
        paused: bool,
        position_ms: u32,
    ) -> Vec<Request> {
        if self.active && self.playback.playing() == Some(viewed) {
            return vec![if paused {
                Request::Resume
            } else {
                Request::Restart
            }];
        }
        self.remember_position(library, position_ms);
        let Some(list) = library.get_mut(viewed) else {
            return Vec::new();
        };
        let resume = list.resume_ms;
        match self.playback.start(list, None) {
            Some(id) => {
                self.skips = 0;
                let position = library
                    .get(viewed)
                    .filter(|list| list.cursor() == Some(id))
                    .map_or(0, |_| resume);
                self.load(library, id, position)
            }
            None => Vec::new(),
        }
    }

    pub fn toggle_pause(&self, paused: bool) -> Vec<Request> {
        match (self.active, paused) {
            (false, _) => Vec::new(),
            (true, true) => vec![Request::Resume],
            (true, false) => vec![Request::Pause],
        }
    }

    /// Stop: the cursor stays, and Play starts its song from the beginning.
    pub fn stop(&mut self, library: &mut Library) -> Vec<Request> {
        if let Some(list) = self.playing_mut(library) {
            list.resume_ms = 0;
        }
        self.halt()
    }

    /// Stop, but Play goes on where the song was.
    pub fn release(&mut self, library: &mut Library, position_ms: u32) -> Vec<Request> {
        self.remember_position(library, position_ms);
        self.halt()
    }

    pub fn next(&mut self, library: &mut Library, viewed: PlaylistId) -> Vec<Request> {
        let target = self.target(viewed);
        let Some(list) = library.get_mut(target) else {
            return Vec::new();
        };
        match self.playback.next(list) {
            Step::Play(id) if self.active => self.load(library, id, 0),
            Step::End if self.active => self.halt(),
            _ => Vec::new(),
        }
    }

    pub fn prev(
        &mut self,
        library: &mut Library,
        viewed: PlaylistId,
        position_ms: u32,
    ) -> Vec<Request> {
        let target = self.target(viewed);
        let Some(list) = library.get_mut(target) else {
            return Vec::new();
        };
        let position = if self.active { position_ms } else { 0 };
        match self.playback.prev(list, position) {
            Step::Play(id) if self.active => self.load(library, id, 0),
            Step::Restart if self.active => vec![Request::Restart],
            _ => Vec::new(),
        }
    }

    /// The engine started the followed song by itself.
    pub fn followed(&mut self, library: &mut Library, entry: EntryId) -> Vec<Request> {
        self.followed = None;
        self.advance(library, Some(entry))
    }

    /// The engine finished a song with nothing to follow it, and stopped.
    pub fn ended(&mut self, library: &mut Library) -> Vec<Request> {
        self.followed = None;
        self.advance(library, None)
    }

    /// The loaded song can't be played: skip it.
    pub fn unavailable(&mut self, library: &mut Library) -> Vec<Request> {
        if !self.active {
            return Vec::new();
        }
        self.skips += 1;
        if self.skips >= SKIP_LIMIT {
            return self.halt();
        }
        let Some(list) = self.playing_mut(library) else {
            return self.halt();
        };
        match self.playback.next(list) {
            Step::Play(id) => self.load(library, id, 0),
            _ => self.halt(),
        }
    }

    /// The loaded song didn't load, or stopped at `position_ms`, for want
    /// of a connection: stop on it, to go on from there with `resume`.
    pub fn offline(&mut self, library: &mut Library, position_ms: u32) -> Vec<Request> {
        if !self.active {
            return Vec::new();
        }
        if let Some(list) = self.playing_mut(library) {
            list.resume_ms = position_ms;
        }
        self.halt()
    }

    /// Plays the playlist that was playing, from where it stopped.
    pub fn resume(&mut self, library: &mut Library) -> Vec<Request> {
        match self.playback.playing() {
            Some(list) => self.play(library, list, false, 0),
            None => Vec::new(),
        }
    }

    /// Call after any change to playlist `list`, so what follows the
    /// playing song stays right.
    pub fn edited(&mut self, library: &mut Library, list: PlaylistId) -> Vec<Request> {
        if self.playback.playing() != Some(list) {
            return Vec::new();
        }
        if let Some(playlist) = library.get_mut(list) {
            self.playback.edited(playlist);
        }
        self.sync(library)
    }

    /// Call before deleting playlist `list`: playing from it stops.
    pub fn deleting(&mut self, list: PlaylistId) -> Vec<Request> {
        if self.playback.playing() == Some(list) {
            self.halt()
        } else {
            Vec::new()
        }
    }

    /// The playlist a song of is loaded, if any.
    pub fn playing_list(&self) -> Option<PlaylistId> {
        self.playback.playing().filter(|_| self.active)
    }

    pub fn set_shuffle(&mut self, library: &mut Library, on: bool) -> Vec<Request> {
        self.playback.set_shuffle(on);
        self.sync(library)
    }

    pub fn set_repeat(&mut self, library: &mut Library, on: bool) -> Vec<Request> {
        self.playback.repeat = on;
        self.sync(library)
    }

    /// Keeps where the playing song is, so Play resumes there (after a
    /// switch to another playlist, or a restart of the app).
    pub fn remember_position(&mut self, library: &mut Library, position_ms: u32) {
        if self.active
            && let Some(list) = self.playing_mut(library)
        {
            list.resume_ms = position_ms;
        }
    }

    /// The model moves on as the engine did. `started` is the song the
    /// engine started, if any; when the model disagrees (an edit raced the
    /// end of the song), the model wins.
    fn advance(&mut self, library: &mut Library, started: Option<EntryId>) -> Vec<Request> {
        if !self.active {
            return Vec::new();
        }
        let Some(list) = self.playing_mut(library) else {
            return self.halt();
        };
        match self.playback.next(list) {
            Step::Play(id) if Some(id) == started => {
                self.skips = 0;
                self.sync(library)
            }
            Step::Play(id) => self.load(library, id, 0),
            // The engine stopped by itself.
            _ if started.is_none() => {
                self.active = false;
                self.followed = Some(None);
                Vec::new()
            }
            _ => self.halt(),
        }
    }

    fn load(&mut self, library: &mut Library, entry: EntryId, position_ms: u32) -> Vec<Request> {
        let Some(uri) = self.uri(library, entry) else {
            return self.halt();
        };
        self.active = true;
        // Loading clears the engine's follow.
        self.followed = Some(None);
        let mut requests = vec![Request::Load {
            entry,
            uri,
            position_ms,
        }];
        requests.extend(self.sync(library));
        requests
    }

    fn halt(&mut self) -> Vec<Request> {
        let was_active = std::mem::replace(&mut self.active, false);
        self.followed = Some(None);
        if was_active {
            vec![Request::Stop]
        } else {
            Vec::new()
        }
    }

    /// Hands the engine the song that now comes next, if it changed.
    fn sync(&mut self, library: &mut Library) -> Vec<Request> {
        let next = match self.playing_mut(library) {
            Some(list) if self.active => self.playback.peek_next(list),
            _ => None,
        };
        if self.followed == Some(next) {
            return Vec::new();
        }
        self.followed = Some(next);
        let follow = next.and_then(|id| Some((id, self.uri(library, id)?)));
        vec![Request::Follow(follow)]
    }

    fn playing_mut<'a>(&self, library: &'a mut Library) -> Option<&'a mut Playlist> {
        library.get_mut(self.playback.playing()?)
    }

    fn uri(&self, library: &Library, entry: EntryId) -> Option<String> {
        let list = library.get(self.playback.playing()?)?;
        Some(list.get(entry)?.track.uri.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::playlist::tests::entry;

    const DEFAULT: PlaylistId = PlaylistId::DEFAULT;

    fn setup(n: u64) -> (Library, Transport) {
        let mut library = Library::new();
        library
            .get_mut(DEFAULT)
            .unwrap()
            .append((1..=n).map(entry).collect());
        (library, Transport::new(Playback::new(1)))
    }

    fn load(n: u64, position_ms: u32) -> Request {
        Request::Load {
            entry: EntryId(n),
            uri: format!("spotify:track:{n}"),
            position_ms,
        }
    }

    fn follow(n: u64) -> Request {
        Request::Follow(Some((EntryId(n), format!("spotify:track:{n}"))))
    }

    #[test]
    fn play_loads_the_cursor_and_hands_over_the_next_song() {
        let (mut library, mut transport) = setup(3);
        let requests = transport.play(&mut library, DEFAULT, false, 0);
        assert_eq!(requests, [load(1, 0), follow(2)]);
    }

    #[test]
    fn songs_follow_each_other_then_playback_stops_and_resets() {
        let (mut library, mut transport) = setup(2);
        transport.play(&mut library, DEFAULT, false, 0);
        // The engine started song 2 by itself; nothing follows it.
        assert_eq!(
            transport.followed(&mut library, EntryId(2)),
            [Request::Follow(None)]
        );
        assert!(transport.ended(&mut library).is_empty());
        assert!(!transport.active());
        assert_eq!(library.get(DEFAULT).unwrap().cursor(), Some(EntryId(1)));
    }

    #[test]
    fn an_edit_hands_over_the_new_next_song() {
        let (mut library, mut transport) = setup(3);
        transport.play(&mut library, DEFAULT, false, 0);
        library.get_mut(DEFAULT).unwrap().toggle_mark(EntryId(3));
        assert_eq!(transport.edited(&mut library, DEFAULT), [follow(3)]);
        assert!(transport.edited(&mut library, DEFAULT).is_empty());
    }

    #[test]
    fn an_edit_racing_the_end_of_a_song_loads_the_right_one() {
        let (mut library, mut transport) = setup(3);
        transport.play(&mut library, DEFAULT, false, 0);
        // Song 2 was removed but the engine had already started it.
        library.get_mut(DEFAULT).unwrap().remove(&[EntryId(2)]);
        let requests = transport.followed(&mut library, EntryId(2));
        assert_eq!(requests, [load(3, 0)]);
    }

    #[test]
    fn next_while_stopped_moves_the_cursor_only() {
        let (mut library, mut transport) = setup(3);
        assert!(transport.next(&mut library, DEFAULT).is_empty());
        assert_eq!(library.get(DEFAULT).unwrap().cursor(), Some(EntryId(2)));
    }

    #[test]
    fn play_goes_on_where_the_player_was_released() {
        let (mut library, mut transport) = setup(3);
        transport.play_entry(&mut library, DEFAULT, EntryId(2), 0);
        assert_eq!(transport.release(&mut library, 42_000), vec![Request::Stop]);
        let requests = transport.play(&mut library, DEFAULT, false, 0);
        assert_eq!(requests[0], load(2, 42_000));
    }

    #[test]
    fn play_resumes_where_the_playlist_was_left() {
        let (mut library, mut transport) = setup(3);
        transport.play_entry(&mut library, DEFAULT, EntryId(2), 0);
        transport.remember_position(&mut library, 61_000);
        let mix = library.create("Mix");
        library.get_mut(mix).unwrap().append(vec![entry(10)]);
        transport.play(&mut library, mix, false, 75_000);
        assert_eq!(library.get(DEFAULT).unwrap().resume_ms, 75_000);
        let requests = transport.play(&mut library, DEFAULT, false, 1_000);
        assert_eq!(requests[0], load(2, 75_000));
    }

    #[test]
    fn stop_then_play_starts_the_song_over() {
        let (mut library, mut transport) = setup(3);
        transport.play_entry(&mut library, DEFAULT, EntryId(2), 0);
        transport.remember_position(&mut library, 30_000);
        assert_eq!(transport.stop(&mut library), [Request::Stop]);
        let requests = transport.play(&mut library, DEFAULT, false, 0);
        assert_eq!(requests[0], load(2, 0));
    }

    #[test]
    fn unplayable_songs_are_skipped_up_to_a_limit() {
        let (mut library, mut transport) = setup(30);
        transport.play(&mut library, DEFAULT, false, 0);
        assert_eq!(transport.unavailable(&mut library)[0], load(2, 0));
        for _ in 2..SKIP_LIMIT {
            transport.unavailable(&mut library);
        }
        assert_eq!(transport.unavailable(&mut library), [Request::Stop]);
    }

    #[test]
    fn offline_stops_on_the_song_and_resume_plays_it() {
        let (mut library, mut transport) = setup(30);
        transport.play_entry(&mut library, DEFAULT, EntryId(3), 0);
        transport.remember_position(&mut library, 61_000);
        assert_eq!(transport.offline(&mut library, 0), [Request::Stop]);
        assert!(transport.offline(&mut library, 0).is_empty());
        assert_eq!(transport.resume(&mut library)[0], load(3, 0));
    }

    #[test]
    fn a_song_cut_offline_goes_on_where_it_stopped() {
        let (mut library, mut transport) = setup(30);
        transport.play_entry(&mut library, DEFAULT, EntryId(3), 0);
        assert_eq!(transport.offline(&mut library, 95_000), [Request::Stop]);
        assert_eq!(transport.resume(&mut library)[0], load(3, 95_000));
    }

    #[test]
    fn deleting_the_playing_playlist_stops() {
        let (mut library, mut transport) = setup(3);
        let mix = library.create("Mix");
        transport.play(&mut library, DEFAULT, false, 0);
        assert!(transport.deleting(mix).is_empty());
        assert_eq!(transport.playing_list(), Some(DEFAULT));
        library.get_mut(DEFAULT).unwrap();
        assert_eq!(transport.deleting(DEFAULT), [Request::Stop]);
        assert_eq!(transport.playing_list(), None);
    }

    #[test]
    fn edits_to_another_playlist_change_nothing() {
        let (mut library, mut transport) = setup(3);
        transport.play(&mut library, DEFAULT, false, 0);
        let mix = library.create("Mix");
        assert!(transport.edited(&mut library, mix).is_empty());
    }
}
