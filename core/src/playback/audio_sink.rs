//! Where the player's audio goes: the desktop's device through rodio
//! (`super::sink`) or Android's through AAudio (`super::aaudio`). Audio is
//! interleaved stereo at [`SAMPLE_RATE`](super::SAMPLE_RATE), already
//! levelled and turned down by the volume.
//!
//! The Spotify tab's device plays through librespot's own player, which
//! writes to an output through [`Librespot`].

use librespot_playback::audio_backend::{self, Sink, SinkResult};
use librespot_playback::convert::Converter;
use librespot_playback::decoder::AudioPacket;
use librespot_playback::mixer::VolumeGetter;

/// Why the output took no audio. The status bar has been told already,
/// through the output's error hook.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct SinkError(pub String);

/// Made and used on the player's thread: it needn't be sent between
/// threads (AAudio's stream can't be).
pub trait AudioSink {
    /// Opens the device if it isn't open, and starts it.
    fn start(&mut self) -> Result<(), SinkError>;

    /// Lets the queued audio finish, for up to a second, and pauses. Never
    /// fails: a device that's gone is found on the next start.
    fn stop(&mut self);

    /// Queues `samples`, waiting while the queue is full. `volume` is what
    /// they were turned down by, for the analyser to take out.
    fn write(&mut self, samples: Vec<f32>, volume: f32) -> Result<(), SinkError>;
}

/// An output as librespot's player writes to it.
pub struct Librespot<S> {
    sink: S,
    /// The volume the player turned the audio down by.
    volume: Box<dyn VolumeGetter + Send>,
}

impl<S> Librespot<S> {
    pub fn new(sink: S, volume: Box<dyn VolumeGetter + Send>) -> Self {
        Self { sink, volume }
    }
}

impl<S: AudioSink> Sink for Librespot<S> {
    fn start(&mut self) -> SinkResult<()> {
        self.sink.start().map_err(|e| {
            audio_backend::SinkError::ConnectionRefused(e.0)
        })
    }

    // librespot exits the process if `stop` fails.
    fn stop(&mut self) -> SinkResult<()> {
        self.sink.stop();
        Ok(())
    }

    fn write(&mut self, packet: AudioPacket, converter: &mut Converter) -> SinkResult<()> {
        let samples = packet.samples().map_err(|e| {
            audio_backend::SinkError::OnWrite(e.to_string())
        })?;
        let volume = self.volume.attenuation_factor() as f32;
        self.sink
            .write(converter.f64_to_f32(samples), volume)
            .map_err(|e| audio_backend::SinkError::ConnectionRefused(e.0))
    }
}
