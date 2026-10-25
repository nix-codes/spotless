//! Moving through the playing playlist: Next, Previous, Repeat, Shuffle and
//! the play-next marks.
//!
//! `peek_next` and `next` share one rule, so the song the transport preloads
//! is the one `next` then picks. Call `edited` after changing the playing
//! playlist, so the preload follows the edited list.

use std::collections::HashSet;

use super::playlist::{EntryId, Playlist, PlaylistId};

/// Previous within this many ms of a song's start goes to the song before;
/// later, it restarts the song.
pub const RESTART_AFTER_MS: u32 = 3_000;

const HISTORY_LIMIT: usize = 1_000;

/// What the transport does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Load this song from its start. The cursor is already on it.
    Play(EntryId),
    /// Seek the current song to its start.
    Restart,
    /// Stop. The cursor is back on the first song.
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Successor {
    Mark(EntryId),
    Next(EntryId),
    /// Repeat: back to the start of the list or of a new shuffle round.
    Wrap(EntryId),
    End,
}

/// One round of shuffle: every entry once.
#[derive(Clone, Debug, Default)]
struct Shuffle {
    /// Empty until a playlist needs it.
    order: Vec<EntryId>,
    /// The current song's slot. When the playlist is detached, the slot of
    /// the song that replaced it, not yet played.
    pos: usize,
}

impl Shuffle {
    fn next_slot(&self, detached: bool) -> usize {
        if detached { self.pos } else { self.pos + 1 }
    }

    /// Makes `id` the current song: the next slot, moved there if it is
    /// elsewhere in the round.
    fn advance_to(&mut self, id: EntryId, detached: bool) {
        let mut slot = self.next_slot(detached).min(self.order.len());
        if self.order.get(slot) != Some(&id) {
            if let Some(i) = self.order.iter().position(|&o| o == id) {
                self.order.remove(i);
                if i < slot {
                    slot -= 1;
                }
            }
            self.order.insert(slot, id);
        }
        self.pos = slot;
    }
}

#[derive(Debug)]
pub struct Playback {
    playing: Option<PlaylistId>,
    pub repeat: bool,
    shuffle: Option<Shuffle>,
    /// The songs played before the current one, for Previous in shuffle.
    history: Vec<EntryId>,
    rng: fastrand::Rng,
}

impl Playback {
    pub fn new(seed: u64) -> Self {
        Self {
            playing: None,
            repeat: false,
            shuffle: None,
            history: Vec::new(),
            rng: fastrand::Rng::with_seed(seed),
        }
    }

    /// The playlist Next and Previous act on.
    pub fn playing(&self) -> Option<PlaylistId> {
        self.playing
    }

    pub fn shuffle(&self) -> bool {
        self.shuffle.is_some()
    }

    /// Turning shuffle on starts a round from the current song.
    pub fn set_shuffle(&mut self, on: bool) {
        self.shuffle = on.then(Shuffle::default);
    }

    /// Starts `list` at `entry`, or at its cursor. `None` if it is empty.
    pub fn start(&mut self, list: &mut Playlist, entry: Option<EntryId>) -> Option<EntryId> {
        self.adopt(list);
        let id = entry
            .filter(|&id| list.index_of(id).is_some())
            .or(list.cursor())
            .or(list.first())?;
        if list.cursor() != Some(id) || list.detached() {
            self.go_to(list, id);
        }
        Some(id)
    }

    /// The song after the current one. At the end of the list, playback
    /// stops and the cursor goes back to the first song, unless Repeat.
    pub fn next(&mut self, list: &mut Playlist) -> Step {
        self.adopt(list);
        match self.successor(list) {
            Successor::Mark(id) => {
                list.unmark(id);
                self.go_to(list, id);
                Step::Play(id)
            }
            Successor::Next(id) => {
                self.go_to(list, id);
                Step::Play(id)
            }
            Successor::Wrap(id) => {
                self.remember(list);
                if let Some(shuffle) = &mut self.shuffle {
                    *shuffle = Self::round(&mut self.rng, list, Some(id));
                }
                list.set_cursor(id);
                Step::Play(id)
            }
            Successor::End => {
                list.reset_cursor();
                self.history.clear();
                if let Some(shuffle) = &mut self.shuffle {
                    *shuffle = Shuffle::default();
                }
                Step::End
            }
        }
    }

    /// The song before the current one, or the current one from its start.
    pub fn prev(&mut self, list: &mut Playlist, position_ms: u32) -> Step {
        self.adopt(list);
        if position_ms > RESTART_AFTER_MS {
            return Step::Restart;
        }
        let previous = if self.shuffle.is_some() {
            std::iter::from_fn(|| self.history.pop()).find(|&id| list.index_of(id).is_some())
        } else {
            // A detached cursor stands on the removed song's successor, so
            // the song before it is the one before the removed song too.
            match list.cursor() {
                Some(cursor) => list.index_of(cursor).and_then(|i| i.checked_sub(1)),
                None => list.len().checked_sub(1),
            }
            .map(|i| list.entries()[i].id)
        };
        let Some(id) = previous else {
            return Step::Restart;
        };
        list.set_cursor(id);
        if let Some(shuffle) = &mut self.shuffle {
            match shuffle.order.iter().position(|&o| o == id) {
                Some(i) => shuffle.pos = i,
                None => *shuffle = Self::round(&mut self.rng, list, Some(id)),
            }
        }
        Step::Play(id)
    }

    /// The song `next` would pick, for preloading. `None` at the end.
    pub fn peek_next(&mut self, list: &mut Playlist) -> Option<EntryId> {
        self.adopt(list);
        match self.successor(list) {
            Successor::Mark(id) | Successor::Next(id) | Successor::Wrap(id) => Some(id),
            Successor::End => None,
        }
    }

    /// Brings the shuffle round up to date after `list` was edited.
    pub fn edited(&mut self, list: &mut Playlist) {
        if self.playing == Some(list.id) {
            self.sync(list);
        }
    }

    /// Picks up where the app left off: `list` played last.
    pub fn restore(&mut self, list: &mut Playlist) {
        self.adopt(list);
    }

    /// A different playlist starts afresh.
    fn adopt(&mut self, list: &mut Playlist) {
        if self.playing != Some(list.id) {
            self.playing = Some(list.id);
            self.history.clear();
            if let Some(shuffle) = &mut self.shuffle {
                *shuffle = Shuffle::default();
            }
        }
        self.sync(list);
    }

    fn successor(&self, list: &Playlist) -> Successor {
        if let Some(&id) = list.play_next().first() {
            return Successor::Mark(id);
        }
        let Some(first) = list.first() else {
            return Successor::End;
        };
        let next = match (&self.shuffle, list.cursor()) {
            (Some(shuffle), _) => shuffle
                .order
                .get(shuffle.next_slot(list.detached()))
                .copied(),
            (None, cursor) if list.detached() => cursor,
            (None, cursor) => cursor
                .and_then(|cursor| list.index_of(cursor))
                .and_then(|i| list.entries().get(i + 1))
                .map(|entry| entry.id),
        };
        match next {
            Some(id) => Successor::Next(id),
            None if !self.repeat => Successor::End,
            None => match &self.shuffle {
                // The new round starts with a song of this one.
                Some(shuffle) => Successor::Wrap(shuffle.order.first().copied().unwrap_or(first)),
                None => Successor::Wrap(first),
            },
        }
    }

    fn go_to(&mut self, list: &mut Playlist, id: EntryId) {
        self.remember(list);
        if let Some(shuffle) = &mut self.shuffle {
            shuffle.advance_to(id, list.detached());
        }
        list.set_cursor(id);
    }

    /// Keeps the current song for Previous. A removed one is gone.
    fn remember(&mut self, list: &Playlist) {
        if let Some(cursor) = list.cursor().filter(|_| !list.detached()) {
            if self.history.len() == HISTORY_LIMIT {
                self.history.remove(0);
            }
            self.history.push(cursor);
        }
    }

    /// A shuffled round of `list`, `first` in front.
    fn round(rng: &mut fastrand::Rng, list: &Playlist, first: Option<EntryId>) -> Shuffle {
        let mut order: Vec<EntryId> = list.entries().iter().map(|entry| entry.id).collect();
        rng.shuffle(&mut order);
        if let Some(i) = first.and_then(|first| order.iter().position(|&o| o == first)) {
            order.swap(0, i);
        }
        Shuffle { order, pos: 0 }
    }

    /// Drops removed songs from the round and deals added ones into the part
    /// not yet played.
    fn sync(&mut self, list: &mut Playlist) {
        let Some(shuffle) = &mut self.shuffle else {
            return;
        };
        if shuffle.order.is_empty() {
            let first = list.cursor().filter(|_| !list.detached());
            *shuffle = Self::round(&mut self.rng, list, first);
        } else {
            let present: HashSet<EntryId> = list.entries().iter().map(|entry| entry.id).collect();
            let pos = shuffle.pos.min(shuffle.order.len());
            shuffle.pos = shuffle.order[..pos]
                .iter()
                .filter(|id| present.contains(id))
                .count();
            shuffle.order.retain(|id| present.contains(id));
            let known: HashSet<EntryId> = shuffle.order.iter().copied().collect();
            for entry in list.entries() {
                if !known.contains(&entry.id) {
                    let lowest = shuffle.next_slot(list.detached()).min(shuffle.order.len());
                    let at = self.rng.usize(lowest..=shuffle.order.len());
                    shuffle.order.insert(at, entry.id);
                }
            }
        }
        if list.detached() {
            list.detach_to(shuffle.order.get(shuffle.pos).copied());
        } else if let Some(i) = list
            .cursor()
            .and_then(|cursor| shuffle.order.iter().position(|&o| o == cursor))
        {
            shuffle.pos = i;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::playlist::tests::{entry, list};

    fn id(n: u64) -> EntryId {
        EntryId(n)
    }

    /// Plays `list` from its cursor until it stops, peeking before each step.
    fn play_out(playback: &mut Playback, list: &mut Playlist) -> Vec<u64> {
        let mut played = vec![playback.start(list, None).unwrap().0];
        for _ in 0..1_000 {
            let peeked = playback.peek_next(list);
            match playback.next(list) {
                Step::Play(next) => {
                    assert_eq!(peeked, Some(next));
                    played.push(next.0);
                }
                Step::End => {
                    assert_eq!(peeked, None);
                    return played;
                }
                Step::Restart => unreachable!(),
            }
        }
        panic!("never ended");
    }

    fn sorted(mut v: Vec<u64>) -> Vec<u64> {
        v.sort();
        v
    }

    #[test]
    fn plays_in_order_then_stops_on_the_first_song() {
        let mut list = list(3);
        let mut playback = Playback::new(1);
        assert_eq!(play_out(&mut playback, &mut list), [1, 2, 3]);
        assert_eq!(list.cursor(), Some(id(1)));
    }

    #[test]
    fn repeat_loops() {
        let mut list = list(2);
        let mut playback = Playback::new(1);
        playback.repeat = true;
        playback.start(&mut list, Some(id(2)));
        assert_eq!(playback.peek_next(&mut list), Some(id(1)));
        assert_eq!(playback.next(&mut list), Step::Play(id(1)));
    }

    #[test]
    fn marks_play_first_then_the_list_continues_after_the_last() {
        let mut list = list(6);
        let mut playback = Playback::new(1);
        list.toggle_mark(id(5));
        list.toggle_mark(id(3));
        assert_eq!(play_out(&mut playback, &mut list), [1, 5, 3, 4, 5, 6]);
        assert!(list.play_next().is_empty());
    }

    #[test]
    fn previous_restarts_late_in_a_song_and_at_the_first() {
        let mut list = list(3);
        let mut playback = Playback::new(1);
        playback.start(&mut list, Some(id(2)));
        assert_eq!(playback.prev(&mut list, 10_000), Step::Restart);
        assert_eq!(playback.prev(&mut list, 1_000), Step::Play(id(1)));
        assert_eq!(playback.prev(&mut list, 0), Step::Restart);
    }

    #[test]
    fn removing_the_playing_song_plays_the_one_after_it_next() {
        let mut list = list(4);
        let mut playback = Playback::new(1);
        playback.start(&mut list, Some(id(2)));
        list.remove(&[id(2)]);
        playback.edited(&mut list);
        assert_eq!(playback.peek_next(&mut list), Some(id(3)));
        assert_eq!(playback.next(&mut list), Step::Play(id(3)));
    }

    #[test]
    fn previous_after_removing_the_playing_song_goes_to_the_one_before_it() {
        let mut list = list(4);
        let mut playback = Playback::new(1);
        playback.start(&mut list, Some(id(3)));
        list.remove(&[id(3)]);
        assert_eq!(playback.prev(&mut list, 0), Step::Play(id(2)));
    }

    #[test]
    fn removing_the_playing_last_song_ends_unless_more_is_added() {
        let mut list = list(3);
        let mut playback = Playback::new(1);
        playback.start(&mut list, Some(id(3)));
        list.remove(&[id(3)]);
        assert_eq!(playback.peek_next(&mut list), None);
        list.append(vec![entry(9)]);
        assert_eq!(playback.next(&mut list), Step::Play(id(9)));
    }

    #[test]
    fn next_follows_moves() {
        let mut list = list(4);
        let mut playback = Playback::new(1);
        playback.start(&mut list, None);
        list.move_to(&[id(4)], 1);
        assert_eq!(playback.peek_next(&mut list), Some(id(4)));
    }

    #[test]
    fn an_empty_list_ends() {
        let mut list = list(0);
        let mut playback = Playback::new(1);
        assert_eq!(playback.start(&mut list, None), None);
        assert_eq!(playback.next(&mut list), Step::End);
    }

    #[test]
    fn shuffle_plays_every_song_once_then_stops() {
        for seed in 0..20 {
            let mut list = list(8);
            let mut playback = Playback::new(seed);
            playback.set_shuffle(true);
            let played = play_out(&mut playback, &mut list);
            assert_eq!(played[0], 1, "starts from the cursor");
            assert_eq!(sorted(played), (1..=8).collect::<Vec<_>>());
        }
    }

    #[test]
    fn shuffle_with_repeat_starts_a_new_round() {
        let mut list = list(5);
        let mut playback = Playback::new(3);
        playback.set_shuffle(true);
        playback.repeat = true;
        playback.start(&mut list, None);
        let mut played = Vec::new();
        for _ in 0..10 {
            let Step::Play(next) = playback.next(&mut list) else {
                panic!()
            };
            played.push(next.0);
        }
        assert_eq!(sorted(played[4..9].to_vec()), [1, 2, 3, 4, 5]);
    }

    #[test]
    fn shuffle_previous_walks_back_through_what_played() {
        let mut list = list(6);
        let mut playback = Playback::new(5);
        playback.set_shuffle(true);
        let mut played = vec![playback.start(&mut list, None).unwrap()];
        for _ in 0..3 {
            let Step::Play(next) = playback.next(&mut list) else {
                panic!()
            };
            played.push(next);
        }
        for &expected in played[..3].iter().rev() {
            assert_eq!(playback.prev(&mut list, 0), Step::Play(expected));
        }
        assert_eq!(playback.prev(&mut list, 0), Step::Restart);
    }

    #[test]
    fn shuffle_survives_edits_in_the_middle_of_a_round() {
        for seed in 0..20 {
            let mut list = list(8);
            let mut playback = Playback::new(seed);
            playback.set_shuffle(true);
            let mut played = vec![playback.start(&mut list, None).unwrap().0];
            for _ in 0..2 {
                let Step::Play(next) = playback.next(&mut list) else {
                    panic!()
                };
                played.push(next.0);
            }
            // Remove the playing song and one not played, add two.
            let playing = *played.last().unwrap();
            let unplayed = (1..=8).find(|n| !played.contains(n)).unwrap();
            list.remove(&[id(playing), id(unplayed)]);
            list.append(vec![entry(20), entry(21)]);
            playback.edited(&mut list);
            loop {
                let peeked = playback.peek_next(&mut list);
                match playback.next(&mut list) {
                    Step::Play(next) => {
                        assert_eq!(peeked, Some(next));
                        played.push(next.0);
                    }
                    _ => break,
                }
            }
            let mut expected: Vec<u64> = (1..=8).filter(|&n| n != unplayed).collect();
            expected.extend([20, 21]);
            assert_eq!(sorted(played), expected, "seed {seed}");
        }
    }

    #[test]
    fn shuffle_counts_a_chosen_song_in_the_round() {
        let mut list = list(6);
        let mut playback = Playback::new(9);
        playback.set_shuffle(true);
        playback.start(&mut list, None);
        playback.start(&mut list, Some(id(4)));
        list.toggle_mark(id(2));
        let mut played = vec![1, 4];
        while let Step::Play(next) = playback.next(&mut list) {
            played.push(next.0);
        }
        assert_eq!(played[2], 2, "the mark plays first");
        assert_eq!(sorted(played), [1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn another_playlist_starts_afresh() {
        let mut first = list(3);
        let mut second = list(3);
        second.id = PlaylistId(2);
        let mut playback = Playback::new(1);
        playback.set_shuffle(true);
        playback.start(&mut first, None);
        playback.next(&mut first);
        playback.start(&mut second, None);
        assert_eq!(playback.playing(), Some(PlaylistId(2)));
        assert_eq!(playback.prev(&mut second, 0), Step::Restart);
    }
}
