//! Audio output through rodio, without librespot's crashes.
//!
//! librespot's own rodio sink opens the device when the player is made and
//! panics if there is none, which kills the player thread. This one opens
//! the device when playback starts. If there is no device, or the device
//! stops taking audio, it reports why and returns an error, on which
//! librespot pauses. Play tries the device again.
//!
//! librespot exits the process if `stop` fails, so `stop` never does.

use std::thread;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait};
use librespot_playback::audio_backend::{Sink, SinkError, SinkResult};
use librespot_playback::convert::Converter;
use librespot_playback::decoder::AudioPacket;
use librespot_playback::{NUM_CHANNELS, SAMPLE_RATE};

/// Told why the output failed, in words for the status bar.
pub type ErrorHook = Box<dyn Fn(String) + Send>;

pub const NO_DEVICE: &str = "No audio output was found. Connect or enable one, then press Play.";
const STALLED: &str = "The audio output stopped working. Press Play to try again.";

/// Queued chunks before `write` waits, about half a second of audio.
const QUEUE_LIMIT: usize = 26;
/// A device that takes no audio for this long has failed.
const STALL_LIMIT: Duration = Duration::from_secs(2);
/// How long `stop` lets queued audio finish, so a song's end isn't cut.
const DRAIN_LIMIT: Duration = Duration::from_secs(1);

pub struct RodioSink {
    /// A device name, else the system's default output.
    device: Option<String>,
    output: Option<Output>,
    on_error: ErrorHook,
}

struct Output {
    sink: rodio::Sink,
    _stream: rodio::OutputStream,
}

impl RodioSink {
    /// Opens nothing yet: the device is opened by `start`.
    pub fn new(device: Option<String>, on_error: ErrorHook) -> Self {
        Self {
            device,
            output: None,
            on_error,
        }
    }

    fn fail(&mut self, message: &str, detail: impl std::fmt::Display) -> SinkError {
        log::error!("audio output: {detail}");
        self.output = None;
        (self.on_error)(message.to_string());
        SinkError::ConnectionRefused(detail.to_string())
    }
}

impl Sink for RodioSink {
    fn start(&mut self) -> SinkResult<()> {
        if self.output.is_none() {
            match open(self.device.as_deref()) {
                Ok(output) => self.output = Some(output),
                Err(error) => return Err(self.fail(NO_DEVICE, error)),
            }
        }
        if let Some(output) = &self.output {
            output.sink.play();
        }
        Ok(())
    }

    fn stop(&mut self) -> SinkResult<()> {
        if let Some(output) = &self.output {
            let began = Instant::now();
            while !output.sink.empty() && began.elapsed() < DRAIN_LIMIT {
                thread::sleep(Duration::from_millis(10));
            }
            output.sink.pause();
        }
        Ok(())
    }

    fn write(&mut self, packet: AudioPacket, converter: &mut Converter) -> SinkResult<()> {
        let Some(output) = &self.output else {
            return Err(SinkError::NotConnected("no audio output".into()));
        };
        let samples = packet
            .samples()
            .map_err(|e| SinkError::OnWrite(e.to_string()))?;
        let samples: &[f32] = &converter.f64_to_f32(samples);
        output.sink.append(rodio::buffer::SamplesBuffer::new(
            NUM_CHANNELS as cpal::ChannelCount,
            SAMPLE_RATE,
            samples,
        ));
        // Wait for room; a device that never drains has gone.
        let began = Instant::now();
        while output.sink.len() > QUEUE_LIMIT {
            if began.elapsed() > STALL_LIMIT {
                return Err(self.fail(STALLED, "the device stopped taking audio"));
            }
            thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
enum OpenError {
    #[error("no output device")]
    NoDevice,
    #[error("no output device named {0:?}")]
    NotFound(String),
    #[error("{0}")]
    Devices(#[from] cpal::DevicesError),
    #[error("{0}")]
    Config(#[from] cpal::DefaultStreamConfigError),
    #[error("{0}")]
    Stream(#[from] rodio::StreamError),
}

/// As librespot opens it: stereo at 44.1 kHz if the device can, else its
/// own rate (rodio resamples), else whatever it defaults to.
fn open(device: Option<&str>) -> Result<Output, OpenError> {
    let host = cpal::default_host();
    let device = match device {
        Some(name) => host
            .output_devices()?
            .find(|d| d.name().is_ok_and(|n| n == name))
            .ok_or_else(|| OpenError::NotFound(name.into()))?,
        None => host.default_output_device().ok_or(OpenError::NoDevice)?,
    };
    log::info!(
        "audio output: {}",
        device.name().as_deref().unwrap_or("[unnamed]")
    );
    let default = device.default_output_config()?;
    let config = device
        .supported_output_configs()
        .ok()
        .and_then(|mut configs| configs.find(|c| c.channels() == NUM_CHANNELS as u16))
        .and_then(|c| {
            c.try_with_sample_rate(cpal::SampleRate(SAMPLE_RATE))
                .or_else(|| c.try_with_sample_rate(default.sample_rate()))
        })
        .unwrap_or(default);
    let mut stream = match rodio::OutputStreamBuilder::default()
        .with_device(device.clone())
        .with_config(&config.config())
        .with_sample_format(cpal::SampleFormat::F32)
        .open_stream()
    {
        Ok(stream) => stream,
        Err(error) => {
            log::warn!("audio output: {error}; trying the device's defaults");
            rodio::OutputStreamBuilder::from_device(device)?.open_stream_or_fallback()?
        }
    };
    stream.log_on_drop(false);
    let sink = rodio::Sink::connect_new(stream.mixer());
    Ok(Output {
        sink,
        _stream: stream,
    })
}
