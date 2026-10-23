//! Audio output on Android, written straight to AAudio.
//!
//! Through cpal (under rodio), AAudio pulled the audio with a callback
//! and a buffer of AAudio's default size, and over Bluetooth earphones it
//! played with a constant stream of tiny silences. This output asks for
//! the power-saving path that music players use, uses the whole buffer it
//! is given, and writes to it, waiting for room, as librespot's other
//! blocking outputs do.
//!
//! As `super::sink` does, it opens the output when playback starts,
//! reports a missing or stalled output and returns an error, on which
//! librespot pauses, and never fails `stop`. When the device goes (the
//! earphones are disconnected), it opens the output again on whatever
//! Android plays to now and goes on.

use std::thread;
use std::time::{Duration, Instant};

use librespot_playback::audio_backend::{Sink, SinkError, SinkResult};
use librespot_playback::convert::Converter;
use librespot_playback::decoder::AudioPacket;
use librespot_playback::{NUM_CHANNELS, SAMPLE_RATE};
use ndk::audio::{
    AudioContentType, AudioDirection, AudioError, AudioFormat, AudioPerformanceMode, AudioStream,
    AudioStreamBuilder, AudioUsage,
};

use super::sink::{ErrorHook, NO_DEVICE, STALLED};

/// The buffer asked for, about half a second of audio, as `super::sink`
/// queues.
const BUFFER_FRAMES: i32 = SAMPLE_RATE as i32 / 2;
/// A device that takes no audio for this long has failed.
const STALL_LIMIT: Duration = Duration::from_secs(2);
/// How long `stop` lets queued audio finish, so a song's end isn't cut.
const DRAIN_LIMIT: Duration = Duration::from_secs(1);

pub struct AAudioSink {
    stream: Option<AudioStream>,
    on_error: ErrorHook,
}

impl AAudioSink {
    /// Opens nothing yet: the output is opened by `start`.
    pub fn new(on_error: ErrorHook) -> Self {
        Self {
            stream: None,
            on_error,
        }
    }

    fn fail(&mut self, message: &str, detail: impl std::fmt::Display) -> SinkError {
        log::error!("audio output: {detail}");
        self.stream = None;
        (self.on_error)(message.to_string());
        SinkError::ConnectionRefused(detail.to_string())
    }

    /// Opens the output and starts it.
    fn open(&mut self) -> Result<(), AudioError> {
        self.stream = None;
        let stream = open()?;
        stream.request_start()?;
        self.stream = Some(stream);
        Ok(())
    }

    /// Writes all of `samples`, waiting for room. A device that went is
    /// replaced once.
    fn write_all(&mut self, samples: &[f32]) -> SinkResult<()> {
        let channels = NUM_CHANNELS as usize;
        let mut left = samples;
        let mut reopened = false;
        let mut last_progress = Instant::now();
        while !left.is_empty() {
            let Some(stream) = &self.stream else {
                return Err(SinkError::NotConnected("no audio output".into()));
            };
            let frames = (left.len() / channels) as i32;
            // Safety: `left` holds `frames` whole frames of f32 samples, the
            // format the stream was opened with.
            let written = unsafe {
                stream.write(left.as_ptr().cast(), frames, STALL_LIMIT.as_nanos() as i64)
            };
            match written {
                Ok(0) if last_progress.elapsed() >= STALL_LIMIT => {
                    return Err(self.fail(STALLED, "the device stopped taking audio"));
                }
                Ok(0) => {}
                Ok(written) => {
                    left = &left[written as usize * channels..];
                    last_progress = Instant::now();
                }
                Err(AudioError::Disconnected) if !reopened => {
                    log::info!("audio output: the device went; opening the output again");
                    reopened = true;
                    if let Err(error) = self.open() {
                        return Err(self.fail(NO_DEVICE, error));
                    }
                }
                Err(error) => return Err(self.fail(STALLED, error)),
            }
        }
        Ok(())
    }
}

impl Sink for AAudioSink {
    fn start(&mut self) -> SinkResult<()> {
        let started = match &self.stream {
            Some(stream) => stream.request_start(),
            None => Err(AudioError::Disconnected),
        };
        // Not open yet, or no longer usable (its device went while paused).
        if started.is_err()
            && let Err(error) = self.open()
        {
            return Err(self.fail(NO_DEVICE, error));
        }
        Ok(())
    }

    fn stop(&mut self) -> SinkResult<()> {
        if let Some(stream) = &self.stream {
            let began = Instant::now();
            while stream.frames_written() > stream.frames_read() && began.elapsed() < DRAIN_LIMIT {
                thread::sleep(Duration::from_millis(10));
            }
            if let Err(error) = stream.request_pause() {
                log::warn!("audio output: unable to pause: {error}");
            }
            log::info!("audio output: {} underruns so far", stream.x_run_count());
        }
        Ok(())
    }

    fn write(&mut self, packet: AudioPacket, converter: &mut Converter) -> SinkResult<()> {
        let samples = packet
            .samples()
            .map_err(|e| SinkError::OnWrite(e.to_string()))?;
        let samples = converter.f64_to_f32(samples);
        self.write_all(&samples)
    }
}

/// A stereo float stream at librespot's rate (AAudio converts it to the
/// device's), for music, on the power-saving path, with all of its buffer
/// in use.
fn open() -> Result<AudioStream, AudioError> {
    let stream = AudioStreamBuilder::new()?
        .direction(AudioDirection::Output)
        .format(AudioFormat::PCM_Float)
        .channel_count(NUM_CHANNELS as i32)
        .sample_rate(SAMPLE_RATE as i32)
        .usage(AudioUsage::Media)
        .content_type(AudioContentType::Music)
        .performance_mode(AudioPerformanceMode::PowerSaving)
        .buffer_capacity_in_frames(BUFFER_FRAMES)
        .open_stream()?;
    let capacity = stream.buffer_capacity_in_frames();
    let size = stream
        .set_buffer_size_in_frames(capacity)
        .unwrap_or_else(|_| stream.buffer_size_in_frames());
    log::info!(
        "audio output: device {}, {} Hz, {:?}, buffer {size} of {capacity} frames, bursts of {}",
        stream.device_id(),
        stream.sample_rate(),
        stream.performance_mode(),
        stream.frames_per_burst(),
    );
    Ok(stream)
}
