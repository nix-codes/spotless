//! Spotify as a source of audio for the player: a song's details and its
//! file in the format played, through librespot, and the file downloaded
//! from Spotify's servers (or kept in the cache), decrypted with its key
//! and read past Spotify's own header.
//!
//! A song kept is recorded with what opening it takes (its file, format and
//! details), so it opens again asking Spotify only for its key, which
//! librespot keeps in memory once given: one played before while the app
//! runs opens with no connection at all.

use std::io::{self, Read, Seek, SeekFrom};
use std::ops::Range;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use futures_util::future::BoxFuture;
use futures_util::{FutureExt, StreamExt, TryStreamExt};
use http_body_util::BodyExt;
use hyper::StatusCode;
use hyper::header::CONTENT_RANGE;
use librespot_core::audio_key::AudioKeyError;
use librespot_core::cdn_url::CdnUrl;
use librespot_core::error::ErrorKind;
use librespot_core::http_client::HttpClient;
use librespot_core::session::Session;
use librespot_core::{Error, FileId, SpotifyId, SpotifyUri};
use librespot_metadata::audio::{AudioFileFormat, AudioFiles, AudioItem, UniqueFields};
use serde::{Deserialize, Serialize};
use symphonia::core::io::MediaSource;
use symphonia::core::probe::Hint;

use crate::playback::cache::SongCache;
use crate::playback::decoder::Decoder;
use crate::playback::decrypt::Decrypt;
use crate::playback::download::plan::MINIMUM_PART;
use crate::playback::download::{Body, Downloading, Fault, Progress, Remote};
use crate::playback::engine::TrackInfo;
use crate::playback::levelling::Gains;
use crate::playback::source::{Download, Kept, OpenError, Opened, Source};

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

/// The counter's start for every Spotify file.
const AUDIO_IV: [u8; 16] = [
    0x72, 0xe0, 0x67, 0xfb, 0xdd, 0xcb, 0xcf, 0x77, 0xeb, 0xe8, 0xbc, 0x64, 0x3f, 0x63, 0x0d, 0x93,
];

/// How long a server has to give a file's first part.
const FIRST_PART_TIME: Duration = Duration::from_secs(10);

/// Opens songs on the session, which a reconnect replaces.
pub struct SpotifyAudio {
    session: Mutex<Session>,
    runtime: tokio::runtime::Handle,
    cache: Arc<SongCache>,
    /// The servers' hosts that stopped sending, or sent too slowly, on this
    /// session: asked only after the others.
    given_out: Arc<Mutex<Vec<String>>>,
}

impl SpotifyAudio {
    /// Must be called on the tokio runtime, which opening runs on.
    pub fn new(session: &Session, cache: Arc<SongCache>) -> Self {
        Self {
            session: Mutex::new(session.clone()),
            runtime: tokio::runtime::Handle::current(),
            cache,
            given_out: Arc::default(),
        }
    }

    /// Songs open on `session` from now on.
    pub fn set_session(&self, session: &Session) {
        *lock(&self.session) = session.clone();
        lock(&self.given_out).clear();
    }
}

impl Source for SpotifyAudio {
    fn open(&self, uri: &str) -> Result<Opened, OpenError> {
        let session = lock(&self.session).clone();
        self.runtime.block_on(self.open_on(&session, uri))
    }
}

/// A song's file to play, as its details name it.
#[derive(Clone)]
struct Resolved {
    details: TrackInfo,
    format: AudioFileFormat,
    file_id: FileId,
}

/// What the cache keeps of a song beside its file, to open it again
/// without its details from Spotify. The file's ID is its key.
#[derive(Serialize, Deserialize)]
struct Record {
    /// As librespot names it (`OGG_VORBIS_320`).
    format: String,
    /// The song played: the one asked for, or its alternative.
    uri: String,
    title: String,
    artists: String,
    main_artist: String,
    album: String,
    duration_ms: u32,
    cover_url: Option<String>,
}

impl Resolved {
    fn key(&self) -> String {
        format!("spotify/{}", self.file_id.to_base16())
    }

    fn record(&self) -> serde_json::Value {
        let d = &self.details;
        let record = Record {
            format: format!("{:?}", self.format),
            uri: d.uri.clone(),
            title: d.title.clone(),
            artists: d.artists.clone(),
            main_artist: d.main_artist.clone(),
            album: d.album.clone(),
            duration_ms: d.duration_ms,
            cover_url: d.cover_url.clone(),
        };
        serde_json::to_value(record).unwrap_or_default()
    }

    /// The song a kept record describes, if it reads as one.
    fn from_record(key: &str, details: serde_json::Value) -> Option<Self> {
        let record: Record = serde_json::from_value(details).ok()?;
        let format = *FORMATS.iter().find(|f| format!("{f:?}") == record.format)?;
        let file_id = FileId::from_raw(&from_base16(key.strip_prefix("spotify/")?)?);
        Some(Self {
            details: TrackInfo {
                uri: record.uri,
                title: record.title,
                artists: record.artists,
                main_artist: record.main_artist,
                album: record.album,
                duration_ms: record.duration_ms,
                cover_url: record.cover_url,
            },
            format,
            file_id,
        })
    }
}

/// The bytes a file ID's hexadecimal stands for.
fn from_base16(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
        .collect()
}

/// A file to read, from wherever it comes.
trait File: Read + Seek + Send + Sync {}
impl<T: Read + Seek + Send + Sync> File for T {}

impl SpotifyAudio {
    /// Opens the song `asked` for, by its URI.
    async fn open_on(&self, session: &Session, asked: &str) -> Result<Opened, OpenError> {
        let uri = SpotifyUri::from_uri(asked).map_err(|_| OpenError::Unavailable)?;
        let id = SpotifyId::try_from(&uri).map_err(|_| OpenError::Unavailable)?;
        // A song kept with its record needs no details from Spotify.
        let kept = self
            .cache
            .find(asked)
            .and_then(|found| Some((Resolved::from_record(&found.key, found.details)?, found.file)));
        let (song, mut kept) = match kept {
            Some((song, file)) => (song, Some(file)),
            None => {
                let song = resolve(session, uri).await?;
                // Kept already, as another song's or before it had a record.
                let file = self.cache.get(&song.key());
                if file.is_some() {
                    self.cache.name(&song.key(), asked, &song.record());
                }
                (song, file)
            }
        };
        let Resolved {
            details,
            format,
            file_id,
        } = &song;
        let (format, file_id) = (*format, *file_id);
        let name = &details.title;
        let key = song.key();
        // A loop only to download a cached file again that didn't decode.
        loop {
            let (file, download, cached, length): (Box<dyn File>, Box<dyn Download>, _, _) =
                match kept.take() {
                    Some(file) => {
                        let length = file.metadata().map_err(|_| OpenError::Unavailable)?.len();
                        (Box::new(file), Box::new(Kept), true, length)
                    }
                    None => {
                        let (file, progress) = self.download(session, &song, asked).await?;
                        let length = file.size() as u64;
                        (Box::new(file), Box::new(progress), false, length)
                    }
                };
            // Some files aren't encrypted: without a key, the file is read
            // as it is, and the decoder fails if it was encrypted after all.
            // A key that didn't come for want of a connection makes that
            // failure the connection's; one Spotify refused ends the opening
            // here.
            let (audio_key, mut unreachable) = match session.audio_key().request(id, file_id).await {
                Ok(key) => (Some(key.0), false),
                Err(e) if matches!(e.error.downcast_ref(), Some(AudioKeyError::AesKey)) => {
                    log::warn!("Spotify refused the audio key of {}", name);
                    download.close();
                    return Err(OpenError::KeyRefused);
                }
                Err(e) => {
                    log::warn!("no audio key, reading the file as it is: {e}");
                    (None, is_unreachable(&e))
                }
            };
            let mut decrypted = Decrypt::new(audio_key, AUDIO_IV, file);
            let ogg = AudioFiles::is_ogg_vorbis(format);
            let (offset, mut gains) = if ogg {
                (OGG_HEADER_END, spotify_gains(&mut decrypted).ok())
            } else {
                (0, None)
            };
            let audio = match Audio::new(decrypted, offset, length) {
                Ok(audio) => audio,
                Err(e) => {
                    log::error!("unable to read the file of {}: {e}", name);
                    return Err(if download.stalled() {
                        OpenError::Unreachable
                    } else {
                        OpenError::Unavailable
                    });
                }
            };
            let mut hint = Hint::new();
            if let Some(mime) = AudioFiles::mime_type(format) {
                hint.mime_type(mime);
            }
            let mut decoder = match Decoder::new(Box::new(audio), &hint, MINIMUM_PART) {
                Ok(decoder) => decoder,
                // Without its key, a kept file can't decode: that's no fault
                // of the file.
                Err(e) if cached && audio_key.is_none() => {
                    log::warn!("{name} is kept but its key didn't come: {e}");
                    return Err(after_failure(unreachable));
                }
                Err(e) if cached => {
                    log::warn!("the cached file of {} doesn't decode ({e}); downloading it", name);
                    self.cache.remove(&key);
                    continue;
                }
                Err(e) => {
                    log::error!("unable to decode {}: {e}", name);
                    unreachable |= download.stalled();
                    download.close();
                    return Err(after_failure(unreachable));
                }
            };
            // Files other than Ogg have their gains as tags, if at all.
            if gains.is_none() {
                gains = decoder.tag_gains();
            }
            // Found its place: parts are asked for ahead from now on.
            download.stream();
            log::info!(
                "{} ({} ms) opened{}",
                name,
                details.duration_ms,
                if cached { " from the cache" } else { "" }
            );
            return Ok(Opened {
                details: details.clone(),
                decoder,
                gains: gains.unwrap_or_default(),
                download,
            });
        }
    }

    /// Starts downloading the song's file from Spotify's servers: its first
    /// part tells its size. Once whole, it's kept with its record, found by
    /// the song `asked` for and the one played.
    async fn download(
        &self,
        session: &Session,
        song: &Resolved,
        asked: &str,
    ) -> Result<(Downloading, Progress), OpenError> {
        let (file_id, name) = (song.file_id, &song.details.title);
        let cdn = CdnUrl::new(file_id).resolve_audio(session).await.map_err(|e| {
            log::error!("unable to find the servers of {}: {e:?}", name);
            from_error(&e)
        })?;
        let mut urls: Vec<String> = cdn
            .try_get_urls()
            .map_err(|e| from_error(&e))?
            .into_iter()
            .map(String::from)
            .collect();
        {
            let given_out = lock(&self.given_out);
            urls.sort_by_key(|url| given_out.iter().any(|h| h == host(url)));
        }
        let remote = Arc::new(Servers {
            session: session.clone(),
            urls,
            given_out: Arc::clone(&self.given_out),
        });
        let (server, size, head) = remote.first_part().await.inspect_err(|_| {
            log::error!("no server gave the start of {}", name);
        })?;
        let cache = Arc::clone(&self.cache);
        let kept = song.key();
        let record = song.record();
        let mut names = vec![asked.to_string()];
        if song.details.uri != asked {
            names.push(song.details.uri.clone());
        }
        let (file, progress) = Downloading::start(
            remote,
            server,
            size,
            bytes_per_second(song.format),
            head,
            &self.cache.temporary_dir(),
            Box::new(move |file| {
                let kept_file = file.seek(SeekFrom::Start(0)).and_then(|_| cache.put(&kept, &names, &record, file));
                match kept_file {
                    Ok(()) => log::debug!("{kept} kept"),
                    Err(e) => log::error!("unable to keep {kept}: {e}"),
                }
            }),
        )
        .map_err(|e| {
            log::error!("unable to start downloading {}: {e}", name);
            OpenError::Unavailable
        })?;
        Ok((file, progress))
    }
}

/// The servers a Spotify file can be downloaded from, through the session's
/// HTTP client.
struct Servers {
    session: Session,
    urls: Vec<String>,
    given_out: Arc<Mutex<Vec<String>>>,
}

impl Servers {
    /// The file's first part and size, from the first server to give them.
    /// Returns which server that was.
    async fn first_part(&self) -> Result<(usize, usize, Bytes), OpenError> {
        for server in 0..self.urls.len() {
            let first = async {
                let response = self.request(server, 0..MINIMUM_PART).await?;
                let size = content_size(&response)?;
                let mut body = response.into_body().into_data_stream();
                let mut data = BytesMut::new();
                while let Some(chunk) = body.next().await {
                    data.extend_from_slice(&chunk.map_err(|e| Fault(e.to_string()))?);
                }
                if data.is_empty() || data.len() > size {
                    return Err(Fault("no data".into()));
                }
                Ok::<_, Fault>((size, data.freeze()))
            };
            match tokio::time::timeout(FIRST_PART_TIME, first).await {
                Ok(Ok((size, head))) => return Ok((server, size, head)),
                Ok(Err(e)) => log::warn!("{} didn't give the start: {e}; trying the next", self.name(server)),
                Err(_) => log::warn!("{} didn't give the start in time; trying the next", self.name(server)),
            }
            self.gave_out(server);
        }
        Err(OpenError::Unreachable)
    }

    /// Asks `server` for `range`; answered with the part, or an error.
    fn request(
        &self,
        server: usize,
        range: Range<usize>,
    ) -> BoxFuture<'static, Result<hyper::Response<hyper::body::Incoming>, Fault>> {
        let stream = self
            .session
            .spclient()
            .stream_from_cdn(self.urls[server].as_str(), range.start, range.len());
        async move {
            let mut stream = stream.map_err(|e| Fault(e.to_string()))?;
            let response = stream
                .next()
                .await
                .ok_or_else(|| Fault("no answer".into()))?
                .map_err(|e| Fault(e.to_string()))?;
            match response.status() {
                StatusCode::PARTIAL_CONTENT => Ok(response),
                StatusCode::TOO_MANY_REQUESTS => {
                    if let Some(wait) = HttpClient::get_retry_after(response.headers()) {
                        log::warn!("asked to wait {} s before asking again", wait.as_secs());
                        tokio::time::sleep(wait).await;
                    }
                    Err(Fault("too many requests".into()))
                }
                status => Err(Fault(format!("answered {status}"))),
            }
        }
        .boxed()
    }
}

impl Remote for Servers {
    fn servers(&self) -> usize {
        self.urls.len()
    }

    fn name(&self, server: usize) -> String {
        host(&self.urls[server]).to_string()
    }

    fn get(&self, server: usize, range: Range<usize>) -> BoxFuture<'static, Result<Body, Fault>> {
        let response = self.request(server, range);
        async move {
            let body: Body = response
                .await?
                .into_body()
                .into_data_stream()
                .map_err(|e| Fault(e.to_string()))
                .boxed();
            Ok(body)
        }
        .boxed()
    }

    fn gave_out(&self, server: usize) {
        let host = host(&self.urls[server]).to_string();
        let mut given_out = lock(&self.given_out);
        if !given_out.contains(&host) {
            given_out.push(host);
        }
    }
}

/// The whole file's size, from a part's `Content-Range` header.
fn content_size(response: &hyper::Response<hyper::body::Incoming>) -> Result<usize, Fault> {
    let range = response
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| Fault("no Content-Range".into()))?;
    range
        .rsplit_once('/')
        .and_then(|(_, size)| size.parse().ok())
        .ok_or_else(|| Fault(format!("a Content-Range of {range:?}")))
}

/// The host of a server's URL; the rest holds a token, so it isn't logged.
fn host(url: &str) -> &str {
    url.split('/').nth(2).unwrap_or_default()
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
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
        details: track_info(&item),
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
