//! Spotless's player: opens songs through a [`Source`], decodes them, levels
//! them and writes them to an [`AudioSink`], on a thread of its own, and
//! tells what happens through events.
//!
//! A song is opened on a thread of its own too, as opening reads the
//! file's start, which waits on the network. The next song can be opened
//! ahead (`preload`): loading it then starts it at once, and the audio
//! still queued in the output plays on meanwhile, so songs follow each
//! other without a gap.
//!
//! Every load is a request, numbered: the events of a song carry its
//! request's number, so a late event of a song already replaced can be
//! told apart.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use super::audio_sink::AudioSink;
use super::decoder::DecodeError;
use super::engine::TrackInfo;
use super::levelling::{Levelling, attenuation};
use super::source::{OpenError, Opened, Source};
use crate::model::SongUri;

/// How often the position is told while a song plays.
const POSITION_EVERY: Duration = Duration::from_secs(1);

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A song is opening, to start at `position_ms`.
    Loading { request: u64, position_ms: u32 },
    /// The song's details, once it's open.
    Details {
        request: u64,
        details: Box<TrackInfo>,
    },
    Playing { request: u64, position_ms: u32 },
    Paused { request: u64, position_ms: u32 },
    /// Where the song is, every second while it plays.
    Position { request: u64, position_ms: u32 },
    Seeked { request: u64, position_ms: u32 },
    /// Time to preload the next song: the playing one is near its end, and
    /// all downloaded.
    PreloadDue { request: u64 },
    /// The song ended: `stalled` when its audio stopped coming, short of
    /// its end, rather than ran out.
    Ended { request: u64, stalled: bool },
    /// The song didn't open.
    Failed { request: u64, error: OpenError },
    Stopped { request: u64 },
}

pub type OnEvent = Box<dyn Fn(Event) + Send>;
/// Makes the output, on the player's thread.
pub type MakeSink = Box<dyn FnOnce() -> Box<dyn AudioSink> + Send>;

pub struct Player {
    inputs: Sender<Input>,
    volume: Arc<AtomicU64>,
    thread: Option<thread::JoinHandle<()>>,
}

enum Input {
    Load {
        request: u64,
        uri: SongUri,
        play: bool,
        position_ms: u32,
    },
    Preload { uri: SongUri, again: bool },
    Play,
    Pause,
    Stop,
    Seek(u32),
    SetLevelling(bool),
    /// The player was let go.
    Shutdown,
    /// A song opened, by the opening's number.
    Opened(u64, Box<Result<Opened, OpenError>>),
}

impl Player {
    pub fn new(
        source: Arc<dyn Source>,
        make_sink: MakeSink,
        volume: u16,
        levelling: bool,
        preload_before_end: Duration,
        on_event: OnEvent,
    ) -> Self {
        let (inputs, received) = mpsc::channel();
        let volume = Arc::new(AtomicU64::new(attenuation(volume).to_bits()));
        let thread = {
            let inputs = inputs.clone();
            let volume = Arc::clone(&volume);
            thread::Builder::new()
                .name("player".into())
                .spawn(move || {
                    Inner {
                        source,
                        sink: make_sink(),
                        sink_running: false,
                        inputs,
                        received,
                        on_event,
                        volume,
                        levelling: Levelling::new(levelling),
                        preload_before_end,
                        state: State::Stopped,
                        preload: Preload::None,
                        openings: 0,
                        last_position: Instant::now(),
                    }
                    .run()
                })
                .expect("the player's thread starts")
        };
        Self {
            inputs,
            volume,
            thread: Some(thread),
        }
    }

    /// Plays `uri` from `position_ms`, or holds it paused there, as
    /// request `request`, which its events carry.
    pub fn load(&self, request: u64, uri: &SongUri, play: bool, position_ms: u32) {
        self.send(Input::Load {
            request,
            uri: uri.clone(),
            play,
            position_ms,
        });
    }

    /// Opens `uri` ahead, for a load of it to start at once. `again`: even
    /// if it's opened ahead already.
    pub fn preload(&self, uri: &SongUri, again: bool) {
        self.send(Input::Preload {
            uri: uri.clone(),
            again,
        });
    }

    pub fn play(&self) {
        self.send(Input::Play);
    }

    pub fn pause(&self) {
        self.send(Input::Pause);
    }

    pub fn stop(&self) {
        self.send(Input::Stop);
    }

    pub fn seek(&self, position_ms: u32) {
        self.send(Input::Seek(position_ms));
    }

    /// Heard from the next piece of audio on.
    pub fn set_volume(&self, volume: u16) {
        self.volume
            .store(attenuation(volume).to_bits(), Ordering::Relaxed);
    }

    pub fn set_levelling(&self, on: bool) {
        self.send(Input::SetLevelling(on));
    }

    fn send(&self, input: Input) {
        // The thread ends only when the player is dropped.
        let _ = self.inputs.send(input);
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        // The thread ends once it's done with the piece of audio it's on.
        self.send(Input::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A song open to play.
struct Song {
    request: u64,
    opened: Opened,
    position_ms: u32,
    /// Whether the preload was told due for it.
    preload_told: bool,
}

enum State {
    Stopped,
    /// Opening a song: the opening's number, and whether it plays once open.
    Loading {
        request: u64,
        uri: SongUri,
        opening: u64,
        play: bool,
        position_ms: u32,
    },
    Paused(Box<Song>),
    Playing(Box<Song>),
    /// The song played to its end, or its audio stopped coming.
    Ended { request: u64 },
}

/// The next song, opened ahead.
enum Preload {
    None,
    Opening { uri: SongUri, opening: u64 },
    Ready { uri: SongUri, opened: Box<Opened> },
}

struct Inner {
    source: Arc<dyn Source>,
    sink: Box<dyn AudioSink>,
    sink_running: bool,
    /// For openings to hand their song back.
    inputs: Sender<Input>,
    received: Receiver<Input>,
    on_event: OnEvent,
    volume: Arc<AtomicU64>,
    levelling: Levelling,
    preload_before_end: Duration,
    state: State,
    preload: Preload,
    /// Openings started so far, to number them.
    openings: u64,
    last_position: Instant,
}

impl Inner {
    fn run(mut self) {
        loop {
            // While playing, inputs are taken between pieces of audio;
            // otherwise the thread waits for one.
            let input = if matches!(self.state, State::Playing(_)) {
                match self.received.try_recv() {
                    Ok(input) => Some(input),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => break,
                }
            } else {
                match self.received.recv() {
                    Ok(input) => Some(input),
                    Err(_) => break,
                }
            };
            match input {
                Some(Input::Shutdown) => break,
                Some(input) => {
                    self.handle(input);
                    continue;
                }
                None => {}
            }
            self.play_piece();
        }
        self.stop_sink();
    }

    fn tell(&self, event: Event) {
        (self.on_event)(event);
    }

    fn handle(&mut self, input: Input) {
        match input {
            Input::Load {
                request,
                uri,
                play,
                position_ms,
            } => self.load(request, uri, play, position_ms),
            Input::Preload { uri, again } => self.preload(uri, again),
            Input::Play => self.play(),
            Input::Pause => self.pause(),
            Input::Stop => self.stop(),
            Input::Seek(position_ms) => self.seek(position_ms),
            Input::SetLevelling(on) => self.levelling.set_on(on),
            Input::Opened(opening, opened) => self.opened(opening, *opened),
            Input::Shutdown => {}
        }
    }

    fn load(&mut self, request: u64, uri: SongUri, play: bool, position_ms: u32) {
        self.tell(Event::Loading {
            request,
            position_ms,
        });
        // Opened ahead: started at once.
        if let Preload::Ready { uri: ready, .. } = &self.preload
            && *ready == uri
            && let Preload::Ready { opened, .. } = std::mem::replace(&mut self.preload, Preload::None)
        {
            self.start(request, uri, *opened, play, position_ms);
            return;
        }
        // Opening ahead, from the start: that opening is the load's.
        let opening = match &self.preload {
            Preload::Opening {
                uri: opening_uri,
                opening,
            } if *opening_uri == uri && position_ms == 0 => *opening,
            _ => self.open(&uri),
        };
        self.preload = Preload::None;
        self.state = State::Loading {
            request,
            uri,
            opening,
            play,
            position_ms,
        };
    }

    fn preload(&mut self, uri: SongUri, again: bool) {
        let opened_ahead = match &self.preload {
            Preload::Opening { uri: other, .. } | Preload::Ready { uri: other, .. } => {
                *other == uri
            }
            Preload::None => false,
        };
        if opened_ahead && !again {
            return;
        }
        if let Preload::Ready { opened, .. } = std::mem::replace(&mut self.preload, Preload::None) {
            opened.download.close();
        }
        let opening = self.open(&uri);
        self.preload = Preload::Opening { uri, opening };
    }

    /// Opens `uri` on a thread of its own. Returns the opening's number.
    fn open(&mut self, uri: &SongUri) -> u64 {
        self.openings += 1;
        let opening = self.openings;
        let source = Arc::clone(&self.source);
        let inputs = self.inputs.clone();
        let uri = uri.clone();
        let spawned = thread::Builder::new()
            .name("opening".into())
            .spawn(move || {
                let opened = source.open(&uri);
                let _ = inputs.send(Input::Opened(opening, Box::new(opened)));
            });
        if let Err(e) = spawned {
            log::error!("unable to start opening a song: {e}");
            let _ = self.inputs.send(Input::Opened(
                opening,
                Box::new(Err(OpenError::Unavailable)),
            ));
        }
        opening
    }

    fn opened(&mut self, opening: u64, opened: Result<Opened, OpenError>) {
        if let State::Loading {
            opening: loading, ..
        } = &self.state
            && *loading == opening
        {
            let State::Loading {
                request,
                uri,
                play,
                position_ms,
                ..
            } = std::mem::replace(&mut self.state, State::Stopped)
            else {
                unreachable!()
            };
            match opened {
                Ok(opened) => self.start(request, uri, opened, play, position_ms),
                Err(error) => {
                    log::error!("{uri} didn't open: {error:?}");
                    self.stop_sink();
                    self.tell(Event::Failed { request, error });
                }
            }
            return;
        }
        if let Preload::Opening {
            opening: preloading,
            ..
        } = &self.preload
            && *preloading == opening
        {
            let Preload::Opening { uri, .. } = std::mem::replace(&mut self.preload, Preload::None)
            else {
                unreachable!()
            };
            match opened {
                Ok(opened) => {
                    log::info!("{uri} opened ahead");
                    self.preload = Preload::Ready {
                        uri,
                        opened: Box::new(opened),
                    };
                }
                // Opened again when it's loaded, which tells a failure then.
                Err(error) => log::info!("{uri} didn't open ahead: {error:?}"),
            }
            return;
        }
        // An opening no longer wanted.
        if let Ok(opened) = opened {
            opened.download.close();
        }
    }

    /// Starts an opened song, playing or paused, at `position_ms`.
    fn start(&mut self, request: u64, uri: SongUri, mut opened: Opened, play: bool, position_ms: u32) {
        let mut at = 0;
        if position_ms > 0 {
            let position_ms = if position_ms > opened.details.duration_ms {
                log::warn!("{position_ms} ms is past the song's end; starting it over");
                0
            } else {
                position_ms
            };
            match opened.decoder.seek(position_ms) {
                Ok(position_ms) => at = position_ms,
                Err(e) => {
                    log::error!("unable to go to {position_ms} ms in {uri}: {e}");
                    let error = if opened.download.stalled() {
                        OpenError::Unreachable
                    } else {
                        OpenError::Unavailable
                    };
                    self.stop_sink();
                    self.tell(Event::Failed { request, error });
                    self.state = State::Stopped;
                    return;
                }
            }
        }
        self.levelling.set_gains(opened.gains);
        self.tell(Event::Details {
            request,
            details: Box::new(opened.details.clone()),
        });
        log::info!("{uri} starts at {at} ms");
        let song = Box::new(Song {
            request,
            opened,
            position_ms: at,
            preload_told: false,
        });
        if play {
            self.state = State::Playing(song);
            self.last_position = Instant::now();
            self.tell(Event::Playing {
                request,
                position_ms: at,
            });
            self.start_sink();
        } else {
            self.stop_sink();
            self.state = State::Paused(song);
            self.tell(Event::Paused {
                request,
                position_ms: at,
            });
        }
    }

    fn play(&mut self) {
        match std::mem::replace(&mut self.state, State::Stopped) {
            State::Paused(song) => {
                let (request, position_ms) = (song.request, song.position_ms);
                self.state = State::Playing(song);
                self.last_position = Instant::now();
                self.tell(Event::Playing {
                    request,
                    position_ms,
                });
                self.start_sink();
            }
            State::Loading {
                request,
                uri,
                opening,
                position_ms,
                ..
            } => {
                self.state = State::Loading {
                    request,
                    uri,
                    opening,
                    play: true,
                    position_ms,
                }
            }
            other => self.state = other,
        }
    }

    fn pause(&mut self) {
        match std::mem::replace(&mut self.state, State::Stopped) {
            State::Playing(song) => {
                let (request, position_ms) = (song.request, song.position_ms);
                self.state = State::Paused(song);
                self.stop_sink();
                self.tell(Event::Paused {
                    request,
                    position_ms,
                });
            }
            State::Loading {
                request,
                uri,
                opening,
                position_ms,
                ..
            } => {
                self.state = State::Loading {
                    request,
                    uri,
                    opening,
                    play: false,
                    position_ms,
                }
            }
            other => self.state = other,
        }
    }

    fn stop(&mut self) {
        let request = match std::mem::replace(&mut self.state, State::Stopped) {
            State::Stopped => return,
            State::Loading { request, .. } | State::Ended { request } => request,
            State::Paused(song) | State::Playing(song) => {
                song.opened.download.close();
                song.request
            }
        };
        self.stop_sink();
        self.tell(Event::Stopped { request });
    }

    fn seek(&mut self, position_ms: u32) {
        match &mut self.state {
            // Opened again from there.
            State::Loading {
                request,
                uri,
                play,
                ..
            } => {
                let (request, uri, play) = (*request, uri.clone(), *play);
                self.load(request, uri, play, position_ms);
            }
            State::Playing(song) | State::Paused(song) => {
                match song.opened.decoder.seek(position_ms) {
                    Ok(at) => {
                        song.position_ms = at;
                        song.opened.download.read_ahead();
                        let request = song.request;
                        self.last_position = Instant::now();
                        self.tell(Event::Seeked {
                            request,
                            position_ms: at,
                        });
                    }
                    Err(e) => log::error!("unable to seek to {position_ms} ms: {e}"),
                }
            }
            State::Stopped | State::Ended { .. } => {
                log::warn!("a seek with no song open");
            }
        }
    }

    /// Decodes the next piece of the playing song and writes it out.
    fn play_piece(&mut self) {
        let State::Playing(song) = &mut self.state else {
            return;
        };
        let request = song.request;
        let next = song.opened.decoder.next_piece();
        match next {
            Ok(Some(mut piece)) => {
                song.position_ms = piece.position_ms;
                let duration_ms = song.opened.details.duration_ms;
                let preload_due = !song.preload_told
                    && duration_ms.saturating_sub(piece.position_ms)
                        < self.preload_before_end.as_millis() as u32
                    && song.opened.download.whole();
                if preload_due {
                    song.preload_told = true;
                }
                let volume = f64::from_bits(self.volume.load(Ordering::Relaxed));
                self.levelling.apply(&mut piece.samples, volume);
                let samples = piece.samples.iter().map(|&s| s as f32).collect();
                // A skipped piece puts the position out: told at once.
                if piece.skipped || self.last_position.elapsed() >= POSITION_EVERY {
                    self.last_position = Instant::now();
                    self.tell(Event::Position {
                        request,
                        position_ms: piece.position_ms,
                    });
                }
                if preload_due {
                    self.tell(Event::PreloadDue { request });
                }
                if self.sink.write(samples, volume as f32).is_err() {
                    // The output said why; play tries it again.
                    self.sink_running = false;
                    self.pause();
                }
            }
            Ok(None) => self.end(request, false),
            Err(e) => {
                log::error!("the song's audio failed: {e}");
                self.end(request, matches!(e, DecodeError::Stalled(_)));
            }
        }
    }

    fn end(&mut self, request: u64, stalled: bool) {
        if let State::Playing(song) = std::mem::replace(&mut self.state, State::Ended { request }) {
            song.opened.download.close();
        }
        self.tell(Event::Ended { request, stalled });
    }

    fn start_sink(&mut self) {
        if self.sink_running {
            return;
        }
        match self.sink.start() {
            Ok(()) => self.sink_running = true,
            // The output said why; play tries it again.
            Err(_) => self.pause(),
        }
    }

    fn stop_sink(&mut self) {
        if self.sink_running {
            self.sink.stop();
            self.sink_running = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::super::source::fake::{FakeSink, FakeSource, Heard};
    use super::*;

    const WAIT: Duration = Duration::from_secs(5);

    struct Test {
        player: Player,
        requests: std::cell::Cell<u64>,
        events: Receiver<Event>,
        heard: Arc<Mutex<Heard>>,
        source: Arc<FakeSource>,
    }

    fn player(source: FakeSource, preload_before_end: Duration) -> Test {
        let heard = Arc::new(Mutex::new(Heard::default()));
        let source = Arc::new(source);
        let (told, events) = mpsc::channel();
        let told = Mutex::new(told);
        let sink = FakeSink(Arc::clone(&heard));
        let player = Player::new(
            Arc::clone(&source) as Arc<dyn Source>,
            Box::new(move || Box::new(sink)),
            u16::MAX,
            false,
            preload_before_end,
            Box::new(move |event| {
                let _ = told.lock().unwrap().send(event);
            }),
        );
        Test {
            player,
            requests: std::cell::Cell::new(0),
            events,
            heard,
            source,
        }
    }

    impl Test {
        /// Loads `uri` as the next request. Returns its number.
        fn load(&self, uri: &str, play: bool, position_ms: u32) -> u64 {
            let request = self.requests.get() + 1;
            self.requests.set(request);
            self.player.load(request, &uri.into(), play, position_ms);
            request
        }

        /// The events up to the first that `until` takes, which is last.
        fn until(&self, until: impl Fn(&Event) -> bool) -> Vec<Event> {
            let mut seen = Vec::new();
            loop {
                let event = self
                    .events
                    .recv_timeout(WAIT)
                    .unwrap_or_else(|_| panic!("no such event; saw {seen:?}"));
                let done = until(&event);
                seen.push(event);
                if done {
                    return seen;
                }
            }
        }
    }

    fn ended(event: &Event) -> bool {
        matches!(event, Event::Ended { .. })
    }

    #[test]
    fn plays_a_song_to_its_end() {
        let test = player(FakeSource::default().with("a", 300, i16::MAX / 2), Duration::ZERO);
        let request = test.load("a", true, 0);
        let events = test.until(ended);
        assert!(matches!(events[0], Event::Loading { request: r, position_ms: 0 } if r == request));
        assert!(matches!(&events[1], Event::Details { details, .. } if details.duration_ms == 300));
        assert!(matches!(events[2], Event::Playing { position_ms: 0, .. }));
        assert_eq!(
            events.last(),
            Some(&Event::Ended {
                request,
                stalled: false
            })
        );
        let heard = test.heard.lock().unwrap();
        assert_eq!(heard.samples.len(), 44_100 * 3 / 10 * 2);
        assert!(heard.samples.iter().all(|s| (s - 0.5).abs() < 0.01));
    }

    #[test]
    fn a_song_opened_ahead_follows_without_opening_again_or_stopping() {
        let source = FakeSource::default()
            .with("a", 300, 1000)
            .with("b", 200, 2000);
        let test = player(source, Duration::from_secs(10));
        test.load("a", true, 0);
        test.until(|e| matches!(e, Event::PreloadDue { .. }));
        test.player.preload(&"b".into(), false);
        test.until(ended);
        let request = test.load("b", true, 0);
        test.until(|e| matches!(e, Event::Ended { request: r, .. } if *r == request));
        assert_eq!(*test.source.opened.lock().unwrap(), ["a", "b"]);
        let heard = test.heard.lock().unwrap();
        assert_eq!((heard.starts, heard.stops), (1, 0));
        assert_eq!(heard.samples.len(), 44_100 / 2 * 2);
    }

    #[test]
    fn the_preload_is_told_only_near_the_end() {
        let test = player(FakeSource::default().with("a", 2000, 0), Duration::from_millis(500));
        test.load("a", true, 0);
        let events = test.until(ended);
        let at = events
            .iter()
            .take_while(|e| !matches!(e, Event::PreloadDue { .. }))
            .filter_map(|e| match e {
                Event::Position { position_ms, .. } => Some(*position_ms),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        assert!(events.iter().any(|e| matches!(e, Event::PreloadDue { .. })));
        assert!(at < 2000);
    }

    #[test]
    fn a_song_loaded_paused_starts_where_it_was_asked() {
        let test = player(FakeSource::default().with("a", 2000, 1000), Duration::ZERO);
        let request = test.load("a", false, 1500);
        let events = test.until(|e| matches!(e, Event::Paused { .. }));
        let Some(Event::Paused { position_ms, .. }) = events.last() else {
            unreachable!()
        };
        assert!((1450..=1500).contains(position_ms), "{position_ms}");
        assert!(test.heard.lock().unwrap().samples.is_empty());
        test.player.play();
        let events = test.until(ended);
        assert!(matches!(events[0], Event::Playing { request: r, .. } if r == request));
        // Only the last half second or so.
        let heard = test.heard.lock().unwrap().samples.len();
        assert!(heard <= 44_100 * 2 * 6 / 10, "{heard}");
    }

    #[test]
    fn a_seek_goes_on_from_there() {
        let test = player(FakeSource::default().with("a", 3000, 1000), Duration::ZERO);
        test.load("a", false, 0);
        test.until(|e| matches!(e, Event::Paused { .. }));
        test.player.seek(2500);
        let events = test.until(|e| matches!(e, Event::Seeked { .. }));
        assert!(matches!(events.last(), Some(Event::Seeked { position_ms, .. }) if *position_ms >= 2450));
        test.player.play();
        test.until(ended);
        let heard = test.heard.lock().unwrap().samples.len();
        assert!(heard <= 44_100 * 2 * 6 / 10, "{heard}");
    }

    #[test]
    fn a_song_that_doesnt_open_fails() {
        let test = player(FakeSource::default(), Duration::ZERO);
        let request = test.load("missing", true, 0);
        let events = test.until(|e| matches!(e, Event::Failed { .. }));
        assert_eq!(
            events.last(),
            Some(&Event::Failed {
                request,
                error: OpenError::Unavailable
            })
        );
    }

    #[test]
    fn the_volume_turns_the_audio_down() {
        let test = player(FakeSource::default().with("a", 200, i16::MAX / 2), Duration::ZERO);
        test.player.set_volume(0);
        test.load("a", true, 0);
        test.until(ended);
        assert!(test.heard.lock().unwrap().samples.iter().all(|s| *s == 0.0));
    }

    #[test]
    fn stop_stops_the_output_and_is_told() {
        let test = player(FakeSource::default().with("a", 5000, 0), Duration::ZERO);
        let request = test.load("a", true, 0);
        test.until(|e| matches!(e, Event::Playing { .. }));
        test.player.stop();
        test.until(|e| *e == Event::Stopped { request });
        assert!(!test.heard.lock().unwrap().started);
    }
}
