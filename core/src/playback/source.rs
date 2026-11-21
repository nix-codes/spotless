//! Where a song's audio comes from, as the player sees it: a source opens a
//! song by its URI and hands over its decoder, its details and gains, and
//! its download, whatever it comes from.

use super::decoder::Decoder;
use super::engine::TrackInfo;
use super::levelling::Gains;
use crate::model::SongUri;

/// Why a song didn't open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenError {
    /// It can't be played: not there, in no format played, or undecodable.
    Unavailable,
    /// Where it comes from couldn't be reached: the network is down or too
    /// slow. It may well play later.
    Unreachable,
    /// Spotify refused its key: it can't be decrypted just now.
    KeyRefused,
}

/// A song opened to play.
pub struct Opened {
    pub details: TrackInfo,
    pub decoder: Decoder,
    pub gains: Gains,
    pub download: Box<dyn Download>,
}

/// How a song's file is coming in.
pub trait Download: Send {
    /// The decoder found its place: the song plays on from there, so the
    /// file is fetched ahead.
    fn stream(&self) {}
    /// Whether the rest of the file, from where it's being read, is in.
    fn whole(&self) -> bool;
    /// Whether a read gave up waiting for it, as when the network went.
    fn stalled(&self) -> bool;
    /// Fetches the next few seconds from where it's read, and waits for
    /// them: after a seek, so playback goes on with some audio in hand.
    fn read_ahead(&self);
    /// Stops fetching it.
    fn close(&self);
}

/// A file all there: kept in the cache, or local.
pub struct Kept;

impl Download for Kept {
    fn whole(&self) -> bool {
        true
    }

    fn stalled(&self) -> bool {
        false
    }

    fn read_ahead(&self) {}

    fn close(&self) {}
}

/// Opens songs. Blocks while it reads their start: called on a thread of
/// its own.
pub trait Source: Send + Sync {
    fn open(&self, uri: &SongUri) -> Result<Opened, OpenError>;
}

/// A source and an output for tests: songs made up as WAV files, and an
/// output that keeps what it's given.
#[cfg(test)]
pub(crate) mod fake {
    use std::collections::HashMap;
    use std::io::Cursor;
    use std::sync::{Arc, Mutex};

    use symphonia::core::probe::Hint;

    use super::*;
    use crate::playback::audio_sink::{AudioSink, SinkError};
    use crate::playback::{CHANNELS, SAMPLE_RATE};

    /// A WAV file of `frames` stereo frames at the player's rate, each
    /// sample `value` as 16 bits.
    pub fn wav(frames: u32, value: i16) -> Vec<u8> {
        let data_len = frames * 4;
        let mut file = Vec::new();
        file.extend_from_slice(b"RIFF");
        file.extend_from_slice(&(36 + data_len).to_le_bytes());
        file.extend_from_slice(b"WAVEfmt ");
        file.extend_from_slice(&16u32.to_le_bytes());
        file.extend_from_slice(&1u16.to_le_bytes());
        file.extend_from_slice(&CHANNELS.to_le_bytes());
        file.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
        file.extend_from_slice(&(SAMPLE_RATE * 4).to_le_bytes());
        file.extend_from_slice(&4u16.to_le_bytes());
        file.extend_from_slice(&16u16.to_le_bytes());
        file.extend_from_slice(b"data");
        file.extend_from_slice(&data_len.to_le_bytes());
        for _ in 0..frames * 2 {
            file.extend_from_slice(&value.to_le_bytes());
        }
        file
    }

    /// Songs by URI: how long each is, and the level its samples are at.
    #[derive(Default)]
    pub struct FakeSource {
        songs: HashMap<String, (u32, i16)>,
        /// The URIs opened, in order.
        pub opened: Mutex<Vec<String>>,
    }

    impl FakeSource {
        pub fn with(mut self, uri: &str, duration_ms: u32, value: i16) -> Self {
            self.songs.insert(uri.into(), (duration_ms, value));
            self
        }
    }

    impl Source for FakeSource {
        fn open(&self, uri: &SongUri) -> Result<Opened, OpenError> {
            self.opened.lock().unwrap().push(uri.to_string());
            let &(duration_ms, value) = self.songs.get(uri.as_str()).ok_or(OpenError::Unavailable)?;
            let frames = (u64::from(SAMPLE_RATE) * u64::from(duration_ms) / 1000) as u32;
            let mut hint = Hint::new();
            hint.with_extension("wav");
            let decoder = Decoder::new(Box::new(Cursor::new(wav(frames, value))), &hint, 64 * 1024)
                .map_err(|_| OpenError::Unavailable)?;
            Ok(Opened {
                details: TrackInfo {
                    uri: uri.clone(),
                    title: uri.to_string(),
                    artists: String::new(),
                    main_artist: String::new(),
                    album: String::new(),
                    duration_ms,
                    cover_url: None,
                },
                decoder,
                gains: Gains::default(),
                download: Box::new(Kept),
            })
        }
    }

    /// What an output was given: its samples, and whether it's started.
    #[derive(Default)]
    pub struct Heard {
        pub samples: Vec<f32>,
        pub started: bool,
        pub starts: usize,
        pub stops: usize,
    }

    pub struct FakeSink(pub Arc<Mutex<Heard>>);

    impl AudioSink for FakeSink {
        fn start(&mut self) -> Result<(), SinkError> {
            let mut heard = self.0.lock().unwrap();
            heard.started = true;
            heard.starts += 1;
            Ok(())
        }

        fn stop(&mut self) {
            let mut heard = self.0.lock().unwrap();
            heard.started = false;
            heard.stops += 1;
        }

        fn write(&mut self, samples: Vec<f32>, _volume: f32) -> Result<(), SinkError> {
            self.0.lock().unwrap().samples.extend(samples);
            Ok(())
        }
    }
}
