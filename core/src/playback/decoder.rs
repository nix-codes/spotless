//! A song's audio decoded with Symphonia, from any source that reads and
//! seeks: a piece at a time, as interleaved stereo samples at the player's
//! rate.

use std::io;
use std::time::Duration;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{self, DecoderOptions};
use symphonia::core::errors::Error;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::{MediaSource, MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::{MetadataOptions, StandardTagKey, Value};
use symphonia::core::probe::Hint;
use symphonia::core::units::TimeBase;

use super::levelling::Gains;
use super::{CHANNELS, SAMPLE_RATE};

#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    /// A read gave up waiting for the audio to download.
    #[error("the audio stopped coming: {0}")]
    Stalled(String),
    #[error("{0}")]
    Failed(String),
}

impl From<Error> for DecodeError {
    fn from(error: Error) -> Self {
        match error {
            Error::IoError(e) if e.kind() == io::ErrorKind::TimedOut => {
                Self::Stalled(e.to_string())
            }
            e => Self::Failed(e.to_string()),
        }
    }
}

/// A piece of decoded audio.
pub struct Piece {
    /// Where in the song it starts.
    pub position_ms: u32,
    /// Whether a piece before it was skipped, as it didn't decode.
    pub skipped: bool,
    pub samples: Vec<f64>,
}

pub struct Decoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn codecs::Decoder>,
    time_base: Option<TimeBase>,
    /// Made with the first piece, whose length suits the rest.
    buffer: Option<SampleBuffer<f64>>,
}

impl Decoder {
    /// Reads the start of `source` to find its format. `buffer_len` is how
    /// much is read at a time: a power of two over 32 KiB.
    pub fn new(
        source: Box<dyn MediaSource>,
        hint: &Hint,
        buffer_len: usize,
    ) -> Result<Self, DecodeError> {
        let stream = MediaSourceStream::new(source, MediaSourceStreamOptions { buffer_len });
        let format_options = FormatOptions {
            enable_gapless: true,
            ..FormatOptions::default()
        };
        let probed = symphonia::default::get_probe().format(
            hint,
            stream,
            &format_options,
            &MetadataOptions::default(),
        )?;
        let format = probed.format;
        let track = format
            .default_track()
            .ok_or_else(|| DecodeError::Failed("no audio track".into()))?;
        let decoder =
            symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;
        let params = decoder.codec_params();
        let rate = params
            .sample_rate
            .ok_or_else(|| DecodeError::Failed("no sample rate".into()))?;
        if rate != SAMPLE_RATE {
            return Err(DecodeError::Failed(format!("unsupported rate: {rate} Hz")));
        }
        let channels = params
            .channels
            .ok_or_else(|| DecodeError::Failed("no channel layout".into()))?;
        if channels.count() != usize::from(CHANNELS) {
            return Err(DecodeError::Failed(format!(
                "unsupported channels: {channels}"
            )));
        }
        let time_base = params.time_base;
        Ok(Self {
            format,
            decoder,
            time_base,
            buffer: None,
        })
    }

    /// ReplayGain tags, where the file has them.
    pub fn tag_gains(&mut self) -> Option<Gains> {
        let metadata = self.format.metadata();
        let tags = metadata.current()?.tags();
        let mut gains = Gains::default();
        let mut found = false;
        for tag in tags {
            let Value::Float(value) = tag.value else {
                continue;
            };
            let field = match tag.std_key {
                Some(StandardTagKey::ReplayGainTrackGain) => &mut gains.track_gain_db,
                Some(StandardTagKey::ReplayGainTrackPeak) => &mut gains.track_peak,
                Some(StandardTagKey::ReplayGainAlbumGain) => &mut gains.album_gain_db,
                Some(StandardTagKey::ReplayGainAlbumPeak) => &mut gains.album_peak,
                _ => continue,
            };
            *field = value;
            found = true;
        }
        found.then_some(gains)
    }

    /// Goes to `position_ms`, or the end if it's past it. Returns where it
    /// went.
    pub fn seek(&mut self, position_ms: u32) -> Result<u32, DecodeError> {
        let mut target = Duration::from_millis(position_ms.into());
        let params = self.decoder.codec_params();
        if let (Some(time_base), Some(frames)) = (params.time_base, params.n_frames) {
            target = target.min(time_base.calc_time(frames).into());
        }
        let seeked = self.format.seek(
            SeekMode::Accurate,
            SeekTo::Time {
                time: target.into(),
                track_id: None,
            },
        )?;
        // The decoder can't tell a seek happened: it starts over.
        self.decoder.reset();
        Ok(self.ms(seeked.actual_ts))
    }

    /// The next piece, or None at the song's end.
    pub fn next_piece(&mut self) -> Result<Option<Piece>, DecodeError> {
        let mut skipped = false;
        loop {
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(Error::IoError(e)) if e.kind() == io::ErrorKind::UnexpectedEof => {
                    return Ok(None);
                }
                Err(e) => return Err(e.into()),
            };
            let position_ms = self.ms(packet.ts());
            match self.decoder.decode(&packet) {
                Ok(decoded) => {
                    let buffer = self.buffer.get_or_insert_with(|| {
                        SampleBuffer::new(decoded.capacity() as u64, *decoded.spec())
                    });
                    buffer.copy_interleaved_ref(decoded);
                    return Ok(Some(Piece {
                        position_ms,
                        skipped,
                        samples: buffer.samples().to_vec(),
                    }));
                }
                Err(Error::DecodeError(_)) => {
                    log::warn!("skipping a piece of audio that doesn't decode, at {position_ms} ms");
                    skipped = true;
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    fn ms(&self, ts: u64) -> u32 {
        match self.time_base {
            Some(base) => Duration::from(base.calc_time(ts)).as_millis() as u32,
            None => (ts as f64 * 1000.0 / f64::from(SAMPLE_RATE)) as u32,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::super::source::fake::wav;
    use super::*;

    fn decoder(file: Vec<u8>) -> Decoder {
        let mut hint = Hint::new();
        hint.with_extension("wav");
        Decoder::new(Box::new(Cursor::new(file)), &hint, 64 * 1024).unwrap()
    }

    #[test]
    fn decodes_a_whole_song_in_pieces_to_its_end() {
        let mut decoder = decoder(wav(SAMPLE_RATE, i16::MAX / 2));
        let mut samples = 0;
        let mut last = 0;
        while let Some(piece) = decoder.next_piece().unwrap() {
            assert!(piece.position_ms >= last);
            last = piece.position_ms;
            assert!(piece.samples.iter().all(|s| (s - 0.5).abs() < 0.01));
            samples += piece.samples.len();
        }
        assert_eq!(samples, SAMPLE_RATE as usize * 2);
        assert!(last > 900, "{last}");
    }

    #[test]
    fn seeks_to_a_position_and_goes_on_from_there() {
        let mut decoder = decoder(wav(SAMPLE_RATE * 2, 1000));
        // To the start of the piece it falls in.
        let at = decoder.seek(1500).unwrap();
        assert!((1450..=1500).contains(&at), "{at}");
        let piece = decoder.next_piece().unwrap().unwrap();
        assert_eq!(piece.position_ms, at);
    }

    #[test]
    fn a_seek_past_the_end_goes_to_the_end() {
        let mut decoder = decoder(wav(SAMPLE_RATE, 1000));
        let at = decoder.seek(60_000).unwrap();
        assert!(at <= 1000, "{at}");
    }

    #[test]
    fn another_rate_is_refused() {
        let mut file = wav(100, 0);
        file[24..28].copy_from_slice(&48_000u32.to_le_bytes());
        let mut hint = Hint::new();
        hint.with_extension("wav");
        let result = Decoder::new(Box::new(Cursor::new(file)), &hint, 64 * 1024);
        assert!(matches!(result, Err(DecodeError::Failed(_))));
    }
}
