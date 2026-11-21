//! How songs flow on the player: the rules the engine carries out.
//!
//! The engine hands a song to load and the song to follow it; the player
//! tells what happens. From these, the flow decides: when to preload the
//! song to follow, what to do when a song ends (follow it, stop, or stop
//! where it was cut short), how a song that didn't open is told (offline
//! or unavailable), when to let a session go that can't reach Spotify, and
//! what a new session picks up. It also folds the player's events into the
//! [`PlayerSnapshot`] the interface shows.
//!
//! It never talks to the player or the network: every method changes the
//! flow and returns the [`Do`]s that carry the decision out, in order.
//! Whether the session is up is handed in, as is the time.

use std::time::{Duration, Instant};

use super::engine::{EngineEvent, PlayerSnapshot, Status};
use super::player::Event;
use super::source::OpenError;
use crate::model::{EntryId, SongUri};

/// Position reports come every second while a song plays; this one's
/// beyond that.
pub const REPORTS_DUE: Duration = Duration::from_millis(1_500);
/// A song that ends further than this from its end was cut short.
const CUT_MARGIN_MS: u32 = 5_000;

const UNAVAILABLE: &str = "This song can't be played.";
const KEY_REFUSED: &str = "Spotify didn't give this song's key just now. Try again in a moment.";

/// What the engine does for the flow.
#[derive(Debug, PartialEq)]
pub enum Do {
    /// Loads `uri` on the player as request `request`.
    Load {
        request: u64,
        uri: SongUri,
        play: bool,
        position_ms: u32,
    },
    /// Opens `uri` ahead on the player. `again`: even if it's opened ahead
    /// already, as that was on a session gone.
    Preload { uri: SongUri, again: bool },
    Stop,
    /// Lets the session go: Spotify couldn't be reached on it, though it
    /// has yet to notice (its socket can take a minute and a half). A new
    /// one is made once Spotify answers.
    LetSessionGo,
    /// Tells the engine's owner.
    Tell(EngineEvent),
}

#[derive(Debug, Default)]
pub struct Flow {
    /// The request the player's events count for: the last load's.
    request: u64,
    snapshot: PlayerSnapshot,
    /// The song loading or loaded, as the snapshot only has its details
    /// once open.
    uri: Option<SongUri>,
    /// The song to start when the playing one ends.
    follow: Option<(EntryId, SongUri)>,
    /// The player said it's time to preload the song to follow, and the
    /// playing song hasn't ended since.
    preload_due: bool,
}

impl Flow {
    /// The player's state, as last told.
    #[cfg(test)]
    pub fn snapshot(&self) -> &PlayerSnapshot {
        &self.snapshot
    }

    /// Whether a song is playing, or loading to play.
    pub fn playing(&self) -> bool {
        matches!(self.snapshot.status, Status::Playing | Status::Loading)
    }

    /// Plays `uri` now, following it with nothing until told otherwise.
    pub fn load(&mut self, uri: &SongUri, position_ms: u32) -> Vec<Do> {
        self.follow = None;
        self.preload_due = false;
        vec![self.start(uri, true, position_ms)]
    }

    /// The song to start when the playing one ends; None: playback stops.
    /// Preloaded at once if the playing song is near its end.
    pub fn follow(&mut self, next: Option<(EntryId, SongUri)>) -> Vec<Do> {
        let changed = self.follow.as_ref().map(|(_, uri)| uri) != next.as_ref().map(|(_, uri)| uri);
        self.follow = next;
        if changed {
            self.preload(false)
        } else {
            Vec::new()
        }
    }

    pub fn stop(&mut self) -> Vec<Do> {
        self.follow = None;
        self.preload_due = false;
        vec![Do::Stop]
    }

    /// A new session, for one that dropped, now opens songs. A song playing
    /// on with the audio it has goes on until it ends; one whose audio
    /// stopped coming, or a paused or loading one, is opened again on the
    /// new session where it was. The song to follow, if preloaded, was
    /// preloaded on the session gone.
    pub fn session_changed(&mut self, now: Instant) -> Vec<Do> {
        let state = &self.snapshot;
        let flowing = state.status == Status::Playing
            && state
                .reported_at
                .is_some_and(|at| now.duration_since(at) < REPORTS_DUE);
        if flowing {
            log::info!("the playing song goes on with the audio it has");
            return self.preload(true);
        }
        let playing = matches!(state.status, Status::Playing | Status::Loading);
        let position_ms = state.position_at(now);
        let (Some(uri), Status::Playing | Status::Paused | Status::Loading) =
            (self.uri.clone(), state.status)
        else {
            return Vec::new();
        };
        log::info!("{uri} opened again on the new session at {position_ms} ms");
        let mut done = vec![self.start(&uri, playing, position_ms)];
        done.extend(self.preload(true));
        done
    }

    /// Handles what the player told. `online`: whether the session is up.
    pub fn event(&mut self, event: Event, online: bool, now: Instant) -> Vec<Do> {
        // A late event of a song already replaced.
        if request(&event) != self.request {
            return Vec::new();
        }
        let mut done = Vec::new();
        match &event {
            Event::PreloadDue { .. } => {
                self.preload_due = true;
                done.extend(self.preload(false));
            }
            Event::Ended { stalled, .. } => {
                self.preload_due = false;
                // The audio stopped coming for want of a connection: the
                // session is let go, so a new one is made once Spotify
                // answers, and the song goes on then.
                let offline = !online || *stalled;
                if *stalled && online {
                    done.push(Do::LetSessionGo);
                }
                // Offline, a song that ended short of its end was cut
                // short: the next would fail too. Near its end, it's over.
                let cut = offline
                    && self.snapshot.track.as_ref().is_some_and(|track| {
                        self.snapshot.position_ms.saturating_add(CUT_MARGIN_MS) < track.duration_ms
                    });
                log::info!(
                    "song ended at {} of {} ms, {}{}",
                    self.snapshot.position_ms,
                    self.snapshot.track.as_ref().map_or(0, |t| t.duration_ms),
                    if offline { "offline" } else { "online" },
                    if cut { ": cut short" } else { "" },
                );
                if cut {
                    done.push(Do::Stop);
                    done.push(Do::Tell(EngineEvent::Cut {
                        position_ms: self.snapshot.position_ms,
                    }));
                } else {
                    match self.follow.take() {
                        Some((entry, uri)) => {
                            done.push(self.start(&uri, true, 0));
                            done.push(Do::Tell(EngineEvent::Followed(entry)));
                        }
                        None => {
                            done.push(Do::Stop);
                            done.push(Do::Tell(EngineEvent::Ended));
                        }
                    }
                }
            }
            Event::Failed { error, .. } => {
                let unreachable = *error == OpenError::Unreachable;
                if unreachable && online {
                    log::info!("Spotify couldn't be reached for the song; the session is let go");
                    done.push(Do::LetSessionGo);
                }
                if *error != OpenError::KeyRefused {
                    let offline = unreachable || !online;
                    log::info!("song unavailable, {}", if offline { "offline" } else { "online" });
                    done.push(Do::Tell(EngineEvent::Unavailable { offline }));
                }
            }
            _ => {}
        }
        if self.apply(event, now) {
            done.push(Do::Tell(EngineEvent::Snapshot(self.snapshot.clone())));
        }
        done
    }

    /// Loads `uri` as a new request, which the player's events then count
    /// for.
    fn start(&mut self, uri: &SongUri, play: bool, position_ms: u32) -> Do {
        self.request += 1;
        self.uri = Some(uri.clone());
        Do::Load {
            request: self.request,
            uri: uri.clone(),
            play,
            position_ms,
        }
    }

    /// Preloads the song to follow, if it's time to.
    fn preload(&self, again: bool) -> Vec<Do> {
        match &self.follow {
            Some((_, uri)) if self.preload_due => vec![Do::Preload {
                uri: uri.clone(),
                again,
            }],
            _ => Vec::new(),
        }
    }

    /// Folds one player event into the snapshot. Returns whether it changed.
    fn apply(&mut self, event: Event, now: Instant) -> bool {
        let state = &mut self.snapshot;
        let before = state.clone();
        let now = Some(now);
        match event {
            Event::Loading { position_ms, .. } => {
                state.status = Status::Loading;
                state.position_ms = position_ms;
                state.reported_at = now;
                state.error = None;
            }
            Event::Details { details, .. } => {
                state.track = Some(*details);
                if state.status == Status::Stopped {
                    state.status = Status::Loading;
                }
            }
            Event::Playing { position_ms, .. } => {
                state.status = Status::Playing;
                state.position_ms = position_ms;
                state.reported_at = now;
            }
            Event::Paused { position_ms, .. } => {
                state.status = Status::Paused;
                state.position_ms = position_ms;
                state.reported_at = now;
            }
            Event::Position { position_ms, .. } | Event::Seeked { position_ms, .. } => {
                state.position_ms = position_ms;
                state.reported_at = now;
            }
            Event::Stopped { .. } => {
                state.status = Status::Stopped;
                state.position_ms = 0;
                state.reported_at = now;
            }
            Event::Failed { error, .. } => {
                state.status = Status::Stopped;
                state.error = Some(
                    match error {
                        OpenError::KeyRefused => KEY_REFUSED,
                        OpenError::Unavailable | OpenError::Unreachable => UNAVAILABLE,
                    }
                    .into(),
                );
            }
            // The end of a song is followed by a load or a stop.
            Event::PreloadDue { .. } | Event::Ended { .. } => return false,
        }
        // Position reports alone change only the timestamp; skip those
        // repaints unless the position really moved.
        *state != before
    }
}

fn request(event: &Event) -> u64 {
    match *event {
        Event::Loading { request, .. }
        | Event::Details { request, .. }
        | Event::Playing { request, .. }
        | Event::Paused { request, .. }
        | Event::Position { request, .. }
        | Event::Seeked { request, .. }
        | Event::PreloadDue { request }
        | Event::Ended { request, .. }
        | Event::Failed { request, .. }
        | Event::Stopped { request } => request,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playback::engine::TrackInfo;

    fn details(uri: &str, duration_ms: u32) -> Box<TrackInfo> {
        Box::new(TrackInfo {
            uri: uri.into(),
            title: uri.into(),
            artists: String::new(),
            main_artist: String::new(),
            album: String::new(),
            duration_ms,
            cover_url: None,
        })
    }

    fn load(request: u64, uri: &str, position_ms: u32) -> Do {
        Do::Load {
            request,
            uri: uri.into(),
            play: true,
            position_ms,
        }
    }

    fn entry(n: u64) -> EntryId {
        EntryId(n)
    }

    /// A flow playing "a" (`duration_ms` long) at `position_ms`, as request 1.
    fn playing(duration_ms: u32, position_ms: u32, now: Instant) -> Flow {
        let mut flow = Flow::default();
        flow.load(&"a".into(), 0);
        for event in [
            Event::Loading {
                request: 1,
                position_ms: 0,
            },
            Event::Details {
                request: 1,
                details: details("a", duration_ms),
            },
            Event::Playing {
                request: 1,
                position_ms: 0,
            },
            Event::Position {
                request: 1,
                position_ms,
            },
        ] {
            flow.event(event, true, now);
        }
        flow
    }

    fn tells(done: &[Do]) -> Vec<&EngineEvent> {
        done.iter()
            .filter_map(|d| match d {
                Do::Tell(EngineEvent::Snapshot(_)) => None,
                Do::Tell(event) => Some(event),
                _ => None,
            })
            .collect()
    }

    fn acts(done: &[Do]) -> Vec<&Do> {
        done.iter().filter(|d| !matches!(d, Do::Tell(_))).collect()
    }

    #[test]
    fn each_load_is_a_new_request() {
        let mut flow = Flow::default();
        assert_eq!(flow.load(&"a".into(), 0), [load(1, "a", 0)]);
        assert_eq!(flow.load(&"b".into(), 5000), [load(2, "b", 5000)]);
    }

    #[test]
    fn a_late_event_of_a_song_replaced_counts_for_nothing() {
        let now = Instant::now();
        let mut flow = playing(200_000, 10_000, now);
        flow.load(&"b".into(), 0);
        let done = flow.event(
            Event::Ended {
                request: 1,
                stalled: false,
            },
            true,
            now,
        );
        assert!(done.is_empty());
    }

    #[test]
    fn the_song_to_follow_is_preloaded_once_due() {
        let now = Instant::now();
        let mut flow = playing(200_000, 100_000, now);
        assert!(flow.follow(Some((entry(2), "b".into()))).is_empty());
        let done = flow.event(Event::PreloadDue { request: 1 }, true, now);
        assert_eq!(
            acts(&done),
            [&Do::Preload {
                uri: "b".into(),
                again: false
            }]
        );
        // Another song to follow, now that it's due: preloaded at once.
        let done = flow.follow(Some((entry(3), "c".into())));
        assert_eq!(
            done,
            [Do::Preload {
                uri: "c".into(),
                again: false
            }]
        );
        // The same one again: nothing to do.
        assert!(flow.follow(Some((entry(3), "c".into()))).is_empty());
    }

    #[test]
    fn a_song_that_ends_is_followed_or_playback_ends() {
        let now = Instant::now();
        let mut flow = playing(200_000, 199_000, now);
        flow.follow(Some((entry(2), "b".into())));
        let ended = |flow: &mut Flow, request| {
            flow.event(
                Event::Ended {
                    request,
                    stalled: false,
                },
                true,
                now,
            )
        };
        let done = ended(&mut flow, 1);
        assert_eq!(acts(&done), [&load(2, "b", 0)]);
        assert_eq!(tells(&done), [&EngineEvent::Followed(entry(2))]);
        // Nothing to follow "b".
        let done = ended(&mut flow, 2);
        assert_eq!(acts(&done), [&Do::Stop]);
        assert_eq!(tells(&done), [&EngineEvent::Ended]);
    }

    #[test]
    fn a_load_or_stop_forgets_the_song_to_follow() {
        let now = Instant::now();
        let mut flow = playing(200_000, 199_000, now);
        flow.follow(Some((entry(2), "b".into())));
        flow.load(&"c".into(), 0);
        let done = flow.event(
            Event::Ended {
                request: 2,
                stalled: false,
            },
            true,
            now,
        );
        assert_eq!(tells(&done), [&EngineEvent::Ended]);
    }

    #[test]
    fn a_song_whose_audio_stops_coming_lets_the_session_go_and_is_cut() {
        let now = Instant::now();
        let mut flow = playing(200_000, 60_000, now);
        flow.follow(Some((entry(2), "b".into())));
        let done = flow.event(
            Event::Ended {
                request: 1,
                stalled: true,
            },
            true,
            now,
        );
        assert_eq!(acts(&done), [&Do::LetSessionGo, &Do::Stop]);
        assert_eq!(
            tells(&done),
            [&EngineEvent::Cut {
                position_ms: 60_000
            }]
        );
    }

    #[test]
    fn offline_a_song_ending_short_is_cut_but_near_its_end_is_over() {
        let now = Instant::now();
        let ended = Event::Ended {
            request: 1,
            stalled: false,
        };
        let mut flow = playing(200_000, 60_000, now);
        let done = flow.event(ended.clone(), false, now);
        assert_eq!(
            tells(&done),
            [&EngineEvent::Cut {
                position_ms: 60_000
            }]
        );
        let mut flow = playing(200_000, 197_000, now);
        flow.follow(Some((entry(2), "b".into())));
        let done = flow.event(ended, false, now);
        assert_eq!(tells(&done), [&EngineEvent::Followed(entry(2))]);
    }

    #[test]
    fn a_song_that_doesnt_open_is_told_offline_or_unavailable() {
        let now = Instant::now();
        let failed = |error| Event::Failed { request: 1, error };
        let mut flow = Flow::default();
        flow.load(&"a".into(), 0);
        let done = flow.event(failed(OpenError::Unavailable), true, now);
        assert_eq!(acts(&done), Vec::<&Do>::new());
        assert_eq!(tells(&done), [&EngineEvent::Unavailable { offline: false }]);
        assert_eq!(flow.snapshot().error.as_deref(), Some(UNAVAILABLE));

        let mut flow = Flow::default();
        flow.load(&"a".into(), 0);
        let done = flow.event(failed(OpenError::Unreachable), true, now);
        assert_eq!(acts(&done), [&Do::LetSessionGo]);
        assert_eq!(tells(&done), [&EngineEvent::Unavailable { offline: true }]);

        let mut flow = Flow::default();
        flow.load(&"a".into(), 0);
        let done = flow.event(failed(OpenError::Unavailable), false, now);
        assert_eq!(tells(&done), [&EngineEvent::Unavailable { offline: true }]);
    }

    #[test]
    fn a_refused_key_stops_with_word_to_try_again() {
        let now = Instant::now();
        let mut flow = Flow::default();
        flow.load(&"a".into(), 0);
        let done = flow.event(
            Event::Failed {
                request: 1,
                error: OpenError::KeyRefused,
            },
            true,
            now,
        );
        assert!(tells(&done).is_empty());
        assert_eq!(flow.snapshot().status, Status::Stopped);
        assert_eq!(flow.snapshot().error.as_deref(), Some(KEY_REFUSED));
    }

    #[test]
    fn a_new_session_leaves_a_flowing_song_but_preloads_again() {
        let now = Instant::now();
        let mut flow = playing(200_000, 150_000, now);
        flow.follow(Some((entry(2), "b".into())));
        flow.event(Event::PreloadDue { request: 1 }, true, now);
        let done = flow.session_changed(now + Duration::from_millis(500));
        assert_eq!(
            done,
            [Do::Preload {
                uri: "b".into(),
                again: true
            }]
        );
    }

    #[test]
    fn a_new_session_opens_a_stalled_song_again_where_it_was() {
        let now = Instant::now();
        let mut flow = playing(200_000, 60_000, now);
        // Reports stopped coming: the position counts on only to their due.
        let done = flow.session_changed(now + Duration::from_secs(10));
        assert_eq!(done, [load(2, "a", 61_500)]);
    }

    #[test]
    fn a_new_session_opens_a_paused_song_again_paused() {
        let now = Instant::now();
        let mut flow = playing(200_000, 60_000, now);
        flow.event(
            Event::Paused {
                request: 1,
                position_ms: 61_000,
            },
            true,
            now,
        );
        let done = flow.session_changed(now + Duration::from_secs(30));
        assert_eq!(
            done,
            [Do::Load {
                request: 2,
                uri: "a".into(),
                play: false,
                position_ms: 61_000
            }]
        );
    }

    #[test]
    fn a_new_session_leaves_a_stopped_player_be() {
        let mut flow = Flow::default();
        assert!(flow.session_changed(Instant::now()).is_empty());
    }

    #[test]
    fn position_reports_that_dont_move_it_repaint_nothing() {
        let now = Instant::now();
        let mut flow = playing(200_000, 10_000, now);
        let report = Event::Position {
            request: 1,
            position_ms: 10_000,
        };
        assert!(flow.event(report, true, now).is_empty());
    }
}
