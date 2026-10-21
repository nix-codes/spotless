//! When a song counts as listened to, by Last.fm's rules: it is longer than
//! 30 s and has played for half its length or 4 minutes, whichever comes
//! first. Time spent paused or skipped over by a seek doesn't count.
//!
//! The player's snapshots are fed in as they come; what Last.fm should hear
//! comes out. Time is passed in, so the rules are tested without waiting.

use std::time::Instant;

use crate::playback::engine::{PlayerSnapshot, Status};

/// Songs this short are never scrobbled.
const MIN_LENGTH_MS: u32 = 30_000;
/// Played this long, any longer song counts.
const ENOUGH_MS: u64 = 240_000;

/// A song as Last.fm takes it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Song {
    pub artist: String,
    pub title: String,
    pub album: String,
    pub duration_s: u32,
}

/// A listen to send: the song, and when it started (Unix seconds).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Scrobble {
    pub song: Song,
    pub timestamp: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Heard {
    NowPlaying(Song),
    Scrobble(Scrobble),
}

/// The listen in progress.
struct Listen {
    uri: String,
    song: Song,
    duration_ms: u32,
    /// When it first played, Unix seconds; None until it has.
    started: Option<u64>,
    played_ms: u64,
    /// While playing: since when, to add to `played_ms`.
    playing_since: Option<Instant>,
    /// The position last seen, to notice a jump back to the start.
    position_ms: u32,
    scrobbled: bool,
}

#[derive(Default)]
pub struct Listens {
    current: Option<Listen>,
}

impl Listens {
    /// Takes the player's latest snapshot, at `now` (`unix_s` on the wall
    /// clock), and tells what Last.fm should hear of it.
    pub fn observe(&mut self, snapshot: &PlayerSnapshot, now: Instant, unix_s: u64) -> Vec<Heard> {
        let mut heard = Vec::new();
        if let Some(listen) = &mut self.current {
            listen.add_played(now);
            heard.extend(listen.check());
        }
        let Some(track) = snapshot.track.as_ref().filter(|t| is_song(&t.uri)) else {
            self.current = None;
            return heard;
        };
        let same = self.current.as_ref().is_some_and(|listen| {
            listen.uri == track.uri
                && snapshot.status != Status::Loading
                // Back at the start after a good while: played again.
                && !(snapshot.position_ms < 3_000 && listen.position_ms > 30_000)
        });
        if !same {
            self.current = Some(Listen {
                uri: track.uri.clone(),
                song: Song {
                    artist: track.main_artist.clone(),
                    title: track.title.clone(),
                    album: track.album.clone(),
                    duration_s: track.duration_ms / 1000,
                },
                duration_ms: track.duration_ms,
                started: None,
                played_ms: 0,
                playing_since: None,
                position_ms: snapshot.position_ms,
                scrobbled: false,
            });
        }
        let listen = self.current.as_mut().expect("set above");
        listen.position_ms = snapshot.position_ms;
        if snapshot.status == Status::Playing {
            if listen.started.is_none() {
                listen.started = Some(unix_s);
                heard.push(Heard::NowPlaying(listen.song.clone()));
            }
            listen.playing_since.get_or_insert(now);
        } else {
            listen.playing_since = None;
        }
        heard
    }

    /// Counts the time played up to `now`, for when no snapshot comes
    /// for a while.
    pub fn tick(&mut self, now: Instant) -> Option<Heard> {
        let listen = self.current.as_mut()?;
        listen.add_played(now);
        listen.check()
    }
}

impl Listen {
    fn add_played(&mut self, now: Instant) {
        // Only while playing: stopped or paused, there is no clock to move.
        if let Some(since) = self.playing_since.as_mut() {
            self.played_ms += now.saturating_duration_since(*since).as_millis() as u64;
            *since = now;
        }
    }

    fn check(&mut self) -> Option<Heard> {
        let enough = (self.duration_ms as u64 / 2).min(ENOUGH_MS);
        if self.scrobbled || self.duration_ms <= MIN_LENGTH_MS || self.played_ms < enough {
            return None;
        }
        self.scrobbled = true;
        Some(Heard::Scrobble(Scrobble {
            song: self.song.clone(),
            timestamp: self.started?,
        }))
    }
}

/// Songs only: podcast episodes aren't scrobbled.
fn is_song(uri: &str) -> bool {
    uri.starts_with("spotify:track:")
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::playback::engine::TrackInfo;

    fn snapshot(uri: &str, length_s: u32, status: Status, position_s: u32) -> PlayerSnapshot {
        PlayerSnapshot {
            status,
            track: Some(TrackInfo {
                uri: uri.into(),
                title: "Roygbiv".into(),
                artists: "Boards of Canada, Someone".into(),
                main_artist: "Boards of Canada".into(),
                album: "Music Has the Right to Children".into(),
                duration_ms: length_s * 1000,
                cover_url: None,
            }),
            position_ms: position_s * 1000,
            reported_at: None,
            error: None,
        }
    }

    /// Plays a song from the start, reporting every second for `seconds`.
    fn play(
        listens: &mut Listens,
        start: Instant,
        uri: &str,
        length_s: u32,
        seconds: u32,
    ) -> Vec<Heard> {
        let mut heard = Vec::new();
        for s in 0..=seconds {
            let at = start + Duration::from_secs(s.into());
            heard.extend(listens.observe(
                &snapshot(uri, length_s, Status::Playing, s),
                at,
                1_000 + s as u64,
            ));
        }
        heard
    }

    fn scrobbles(heard: &[Heard]) -> usize {
        heard
            .iter()
            .filter(|h| matches!(h, Heard::Scrobble(_)))
            .count()
    }

    #[test]
    fn a_song_counts_at_half_its_length() {
        let mut listens = Listens::default();
        let start = Instant::now();
        let heard = play(&mut listens, start, "spotify:track:a", 100, 49);
        assert!(matches!(&heard[0], Heard::NowPlaying(song) if song.artist == "Boards of Canada"));
        assert_eq!(scrobbles(&heard), 0);
        let heard = listens.tick(start + Duration::from_secs(50));
        assert_eq!(
            heard,
            Some(Heard::Scrobble(Scrobble {
                song: Song {
                    artist: "Boards of Canada".into(),
                    title: "Roygbiv".into(),
                    album: "Music Has the Right to Children".into(),
                    duration_s: 100,
                },
                timestamp: 1_000,
            }))
        );
        // Once only.
        assert_eq!(listens.tick(start + Duration::from_secs(90)), None);
    }

    #[test]
    fn a_long_song_counts_at_four_minutes() {
        let mut listens = Listens::default();
        let heard = play(&mut listens, Instant::now(), "spotify:track:a", 1200, 240);
        assert_eq!(scrobbles(&heard), 1);
    }

    #[test]
    fn short_songs_and_episodes_never_count() {
        let mut listens = Listens::default();
        assert_eq!(
            scrobbles(&play(
                &mut listens,
                Instant::now(),
                "spotify:track:a",
                30,
                30
            )),
            0
        );
        let heard = play(&mut listens, Instant::now(), "spotify:episode:b", 600, 300);
        assert!(heard.is_empty());
    }

    #[test]
    fn paused_time_and_seeks_do_not_count() {
        let mut listens = Listens::default();
        let start = Instant::now();
        let uri = "spotify:track:a";
        let at = |s: u64| start + Duration::from_secs(s);
        listens.observe(&snapshot(uri, 100, Status::Playing, 0), at(0), 1_000);
        listens.observe(&snapshot(uri, 100, Status::Paused, 20), at(20), 1_020);
        // An hour paused, then a seek near the end and a little play.
        let heard = listens.observe(&snapshot(uri, 100, Status::Playing, 90), at(3600), 4_600);
        assert!(heard.is_empty());
        let heard = listens.tick(at(3609));
        assert_eq!(heard, None);
        assert!(listens.tick(at(3630)).is_some());
    }

    #[test]
    fn a_stopped_song_does_not_count_while_it_waits() {
        let mut listens = Listens::default();
        let start = Instant::now();
        let uri = "spotify:track:a";
        let at = |s: u64| start + Duration::from_secs(s);
        assert_eq!(scrobbles(&play(&mut listens, start, uri, 100, 8)), 0);
        listens.observe(&snapshot(uri, 100, Status::Stopped, 0), at(8), 1_008);
        // The scrobbler ticks every few seconds while nothing plays.
        for s in (10..600).step_by(5) {
            assert_eq!(listens.tick(at(s)), None, "at {s} s");
        }
    }

    #[test]
    fn a_song_played_again_counts_again() {
        let mut listens = Listens::default();
        let start = Instant::now();
        let uri = "spotify:track:a";
        assert_eq!(scrobbles(&play(&mut listens, start, uri, 60, 59)), 1);
        // Loaded afresh, as repeat or a double-click does.
        let later = start + Duration::from_secs(60);
        listens.observe(&snapshot(uri, 60, Status::Loading, 0), later, 2_000);
        assert_eq!(scrobbles(&play(&mut listens, later, uri, 60, 59)), 1);
    }

    #[test]
    fn the_next_song_starts_its_own_listen() {
        let mut listens = Listens::default();
        let start = Instant::now();
        let heard = play(&mut listens, start, "spotify:track:a", 100, 10);
        assert_eq!(scrobbles(&heard), 0);
        let heard = play(
            &mut listens,
            start + Duration::from_secs(11),
            "spotify:track:b",
            100,
            10,
        );
        assert!(matches!(&heard[0], Heard::NowPlaying(_)));
        assert_eq!(scrobbles(&heard), 0);
    }
}
