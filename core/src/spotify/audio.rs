//! Spotify as a source of audio for the player: a song's details, its file
//! in the format played, its key, and the file decrypted and read past
//! Spotify's own header, through librespot.

use std::io::{self, Read, Seek, SeekFrom};
use std::sync::Mutex;

use librespot_audio::{AudioDecrypt, AudioFetchParams, AudioFile, StreamLoaderController};
use librespot_core::audio_key::AudioKeyError;
use librespot_core::error::ErrorKind;
use librespot_core::session::Session;
use librespot_core::{Error, FileId, SpotifyId, SpotifyUri};
use librespot_metadata::audio::{AudioFileFormat, AudioFiles, AudioItem, UniqueFields};
use symphonia::core::io::MediaSource;
use symphonia::core::probe::Hint;

use crate::playback::decoder::Decoder;
use crate::playback::engine::TrackInfo;
use crate::playback::levelling::Gains;
use crate::playback::source::{Download, OpenError, Opened, Source};

/// The formats played, best first: 320 kbps Ogg Vorbis, then what's
/// nearest.
const FORMATS: [AudioFileFormat; 7] = [
    AudioFileFormat::OGG_VORBIS_320,
    AudioFileFormat::MP3_320,
    AudioFileFormat::MP3_256,
    AudioFileFormat::OGG_VORBIS_160,
    AudioFileFormat::MP3_160,
    AudioFileFormat::OGG_VORBIS_96,
    AudioFileFormat::MP3_96,
];

/// Where the audio starts in a Spotify Ogg file: before it, Spotify's own
/// packet, which holds the gains and which decoders balk at.
const OGG_HEADER_END: u64 = 0xa7;

/// Opens songs on the session, which a reconnect replaces.
pub struct SpotifyAudio {
    session: Mutex<Session>,
    runtime: tokio::runtime::Handle,
}

impl SpotifyAudio {
    /// Must be called on the tokio runtime, which opening runs on.
    pub fn new(session: &Session) -> Self {
        Self {
            session: Mutex::new(session.clone()),
            runtime: tokio::runtime::Handle::current(),
        }
    }

    /// Songs open on `session` from now on.
    pub fn set_session(&self, session: &Session) {
        *self.session.lock().unwrap_or_else(|p| p.into_inner()) = session.clone();
    }
}

impl Source for SpotifyAudio {
    fn open(&self, uri: &str) -> Result<Opened, OpenError> {
        let session = self
            .session
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        let uri = SpotifyUri::from_uri(uri).map_err(|_| OpenError::Unavailable)?;
        self.runtime.block_on(open(&session, uri))
    }
}

/// A song's file to play, as its details name it.
struct Resolved {
    item: AudioItem,
    format: AudioFileFormat,
    file_id: FileId,
    bytes_per_second: usize,
}

async fn open(session: &Session, uri: SpotifyUri) -> Result<Opened, OpenError> {
    let id = SpotifyId::try_from(&uri).map_err(|_| OpenError::Unavailable)?;
    let Resolved {
        item,
        format,
        file_id,
        bytes_per_second,
    } = resolve(session, uri).await?;
    // A loop only to download a cached file again that didn't decode.
    loop {
        let file = AudioFile::open(session, file_id, bytes_per_second)
            .await
            .map_err(|e| {
                log::error!("unable to open the file of {}: {e:?}", item.name);
                from_error(&e)
            })?;
        let cached = file.is_cached();
        let controller = file
            .get_stream_loader_controller()
            .map_err(|e| from_error(&e))?;
        // Some files aren't encrypted: without a key, the file is read as
        // it is, and the decoder fails if it was encrypted after all. A key
        // that didn't come for want of a connection makes that failure the
        // connection's; one Spotify refused ends the opening here.
        let (key, mut unreachable) = match session.audio_key().request(id, file_id).await {
            Ok(key) => (Some(key), false),
            Err(e) if matches!(e.error.downcast_ref(), Some(AudioKeyError::AesKey)) => {
                log::warn!("Spotify refused the audio key of {}", item.name);
                controller.close();
                return Err(OpenError::KeyRefused);
            }
            Err(e) => {
                log::warn!("no audio key, reading the file as it is: {e}");
                (None, is_unreachable(&e))
            }
        };
        let mut decrypted = AudioDecrypt::new(key, file);
        let ogg = AudioFiles::is_ogg_vorbis(format);
        let (offset, mut gains) = if ogg {
            (OGG_HEADER_END, spotify_gains(&mut decrypted).ok())
        } else {
            (0, None)
        };
        let stalled = |controller: &StreamLoaderController| {
            if controller.stalled() {
                OpenError::Unreachable
            } else {
                OpenError::Unavailable
            }
        };
        let audio = match Audio::new(decrypted, offset, controller.len() as u64) {
            Ok(audio) => audio,
            Err(e) => {
                log::error!("unable to read the file of {}: {e}", item.name);
                return Err(stalled(&controller));
            }
        };
        let mut hint = Hint::new();
        if let Some(mime) = AudioFiles::mime_type(format) {
            hint.mime_type(mime);
        }
        let buffer_len = AudioFetchParams::get().minimum_download_size;
        let mut decoder = match Decoder::new(Box::new(audio), &hint, buffer_len) {
            Ok(decoder) => decoder,
            Err(e) if cached => {
                log::warn!("the cached file of {} doesn't decode ({e}); downloading it", item.name);
                if session
                    .cache()
                    .is_none_or(|cache| cache.remove_file(file_id).is_err())
                {
                    return Err(after_failure(unreachable));
                }
                continue;
            }
            Err(e) => {
                log::error!("unable to decode {}: {e}", item.name);
                unreachable |= controller.stalled();
                return Err(after_failure(unreachable));
            }
        };
        // Files other than Ogg have their gains as tags, if at all.
        if gains.is_none() {
            gains = decoder.tag_gains();
        }
        controller.set_stream_mode();
        log::info!("{} ({} ms) opened", item.name, item.duration_ms);
        return Ok(Opened {
            details: track_info(&item),
            decoder,
            gains: gains.unwrap_or_default(),
            download: Box::new(SpotifyDownload {
                controller,
                bytes_per_second,
            }),
        });
    }
}

/// The song's details and its file in the best format played: its own,
/// or an alternative's where it has none.
async fn resolve(session: &Session, uri: SpotifyUri) -> Result<Resolved, OpenError> {
    let item = AudioItem::get_file(session, uri).await.map_err(|e| {
        log::error!("unable to read the song's details: {e:?}");
        from_error(&e)
    })?;
    let item = available(session, item).await.ok_or(OpenError::Unavailable)?;
    let (format, file_id) = FORMATS
        .iter()
        .find_map(|format| Some((*format, *item.files.get(format)?)))
        .ok_or_else(|| {
            log::warn!("{} is in no format played", item.name);
            OpenError::Unavailable
        })?;
    Ok(Resolved {
        bytes_per_second: bytes_per_second(format),
        item,
        format,
        file_id,
    })
}

/// The song if it has files, else the first of its alternatives that's
/// available.
async fn available(session: &Session, item: AudioItem) -> Option<AudioItem> {
    if let Err(e) = item.availability {
        log::error!("{} is unavailable: {e}", item.name);
        return None;
    }
    if !item.files.is_empty() {
        return Some(item);
    }
    for alternative in item.alternatives.into_iter().flat_map(|a| a.0) {
        if let Ok(other) = AudioItem::get_file(session, alternative).await
            && other.availability.is_ok()
            && !other.files.is_empty()
        {
            return Some(other);
        }
    }
    log::error!("{} has no files and no alternative that has", item.name);
    None
}

/// The format's nominal rate, which the download plans by.
fn bytes_per_second(format: AudioFileFormat) -> usize {
    let kbps = match format {
        AudioFileFormat::OGG_VORBIS_96 | AudioFileFormat::MP3_96 => 12,
        AudioFileFormat::OGG_VORBIS_160 | AudioFileFormat::MP3_160 => 20,
        AudioFileFormat::MP3_256 => 32,
        _ => 40,
    };
    kbps * 1024
}

/// The gains in Spotify's header.
fn spotify_gains(file: &mut impl ReadSeek) -> io::Result<Gains> {
    file.seek(SeekFrom::Start(Gains::SPOTIFY_OFFSET))?;
    let mut bytes = [0; Gains::SPOTIFY_LEN];
    file.read_exact(&mut bytes)?;
    Ok(Gains::from_spotify(bytes))
}

trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

/// The decrypted file from where its audio starts, as the decoder reads it.
struct Audio<R> {
    file: R,
    offset: u64,
    /// The whole file's length, as librespot gives it.
    length: u64,
}

impl<R: Read + Seek> Audio<R> {
    fn new(mut file: R, offset: u64, length: u64) -> io::Result<Self> {
        file.seek(SeekFrom::Start(offset))?;
        Ok(Self {
            file,
            offset,
            length,
        })
    }
}

impl<R: Read> Read for Audio<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.file.read(buf)
    }
}

impl<R: Seek> Seek for Audio<R> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let position = match position {
            SeekFrom::Start(at) => SeekFrom::Start(at + self.offset),
            SeekFrom::End(back) if (self.length as i64 - back) < self.offset as i64 => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "before the start of the audio",
                ));
            }
            other => other,
        };
        Ok(self.file.seek(position)? - self.offset)
    }
}

impl<R: Read + Seek + Send + Sync> MediaSource for Audio<R> {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.length)
    }
}

/// A Spotify file's download, as librespot fetches it.
struct SpotifyDownload {
    controller: StreamLoaderController,
    bytes_per_second: usize,
}

impl Download for SpotifyDownload {
    fn whole(&self) -> bool {
        self.controller.range_to_end_available()
    }

    fn stalled(&self) -> bool {
        self.controller.stalled()
    }

    fn read_ahead(&self) {
        let ahead = AudioFetchParams::get().read_ahead_during_playback;
        let length = (ahead.as_secs_f32() * self.bytes_per_second as f32) as usize;
        if let Err(e) = self.controller.fetch_next_and_wait(length, length) {
            log::warn!("reading ahead after a seek: {e}");
        }
    }

    fn close(&self) {
        self.controller.close();
    }
}

/// Unreachable when the failure came of a missing connection.
fn after_failure(unreachable: bool) -> OpenError {
    if unreachable {
        OpenError::Unreachable
    } else {
        OpenError::Unavailable
    }
}

fn from_error(error: &Error) -> OpenError {
    after_failure(is_unreachable(error))
}

/// Whether `error` came of not reaching Spotify (no connection, a timeout,
/// a dropped connection, an overloaded server) rather than of its answer.
fn is_unreachable(error: &Error) -> bool {
    match error.kind {
        ErrorKind::Unavailable
        | ErrorKind::DeadlineExceeded
        | ErrorKind::Aborted
        | ErrorKind::Cancelled
        | ErrorKind::ResourceExhausted => true,
        ErrorKind::Unknown => error.is_transport(),
        _ => false,
    }
}

/// A song's details as the player shows them.
pub(crate) fn track_info(item: &AudioItem) -> TrackInfo {
    let (artists, main_artist, album) = match &item.unique_fields {
        UniqueFields::Track { artists, album, .. } => (
            artists
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            artists.first().map(|a| a.name.clone()).unwrap_or_default(),
            album.clone(),
        ),
        UniqueFields::Local { artists, album, .. } => {
            let artists = artists.clone().unwrap_or_default();
            (artists.clone(), artists, album.clone().unwrap_or_default())
        }
        UniqueFields::Episode { show_name, .. } => {
            (show_name.clone(), show_name.clone(), String::new())
        }
    };
    TrackInfo {
        uri: item.uri.clone(),
        title: item.name.clone(),
        artists,
        main_artist,
        album,
        duration_ms: item.duration_ms,
        cover_url: item
            .covers
            .iter()
            .find(|cover| cover.size == librespot_metadata::image::ImageSize::DEFAULT)
            .or_else(|| item.covers.first())
            .map(|cover| cover.url.clone()),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn the_audio_starts_past_spotifys_header() {
        let file: Vec<u8> = (0..=255).collect();
        let mut audio = Audio::new(Cursor::new(file), OGG_HEADER_END, 256).unwrap();
        let mut first = [0];
        audio.read_exact(&mut first).unwrap();
        assert_eq!(first[0], OGG_HEADER_END as u8);
        assert_eq!(audio.seek(SeekFrom::Start(2)).unwrap(), 2);
        audio.read_exact(&mut first).unwrap();
        assert_eq!(first[0], OGG_HEADER_END as u8 + 2);
    }

    #[test]
    fn gains_are_read_from_the_header() {
        let mut file = vec![0u8; 200];
        file[144..148].copy_from_slice(&(-4.5f32).to_le_bytes());
        let gains = spotify_gains(&mut Cursor::new(file)).unwrap();
        assert_eq!(gains.track_gain_db, -4.5);
    }

    #[test]
    fn a_refusal_is_not_a_missing_connection() {
        assert!(is_unreachable(&Error::unavailable("no network")));
        assert!(!is_unreachable(&Error::not_found("no such song")));
        assert_eq!(after_failure(true), OpenError::Unreachable);
    }
}
