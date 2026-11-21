//! MEGA as a source of audio for the player: a song is a file of a public
//! folder (`mega:<folder>:<file>#<key>`), downloaded in parts from the
//! server MEGA names for it (or kept in the cache), decrypted with its key
//! and decoded as its name's extension says.
//!
//! The file's key comes with the folder's listing, encrypted with the
//! folder's key, which the song's URI holds. A song kept is recorded with
//! that key as given, so it opens again with no connection at all: the
//! record tells nothing that the URI doesn't already open.

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::ops::Range;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use futures_util::future::BoxFuture;
use futures_util::{FutureExt, StreamExt, stream};
use serde::{Deserialize, Serialize};
use symphonia::core::probe::Hint;

use super::api::{Api, ApiError, Node};
use super::link::{FileKey, folder_key};
use crate::model::{SongSource, SongUri};
use crate::playback::cache::SongCache;
use crate::playback::decoder::Decoder;
use crate::playback::decrypt::Decrypt;
use crate::playback::download::plan::MINIMUM_PART;
use crate::playback::download::{Body, Downloading, Fault, Remote};
use crate::playback::engine::TrackInfo;
use crate::playback::source::{Download, Kept, Media, OpenError, Opened, Source};

/// How long the server has to give a file's first part.
const FIRST_PART_TIME: Duration = Duration::from_secs(15);
/// How much of an Ogg file's end is asked for beside its start: more than
/// the largest page, the last of which gives its length.
const TAIL: usize = 128 * 1024;
/// How long the start waits for the end: past that, the end is fetched
/// as the decoder reads it.
const TAIL_TIME: Duration = Duration::from_secs(4);
/// What a file is taken to stream at before it's decoded: 320 kbps.
const BYTES_PER_SECOND: usize = 40 * 1024;

/// Opens MEGA's songs.
pub struct MegaAudio {
    api: Arc<Api>,
    runtime: tokio::runtime::Handle,
    cache: Arc<SongCache>,
    /// Each folder's files by handle, as listed once while the app runs.
    folders: Mutex<HashMap<String, Arc<HashMap<String, Node>>>>,
}

/// What the cache keeps of a song beside its file.
#[derive(Serialize, Deserialize)]
struct Record {
    /// The file's key as the listing gave it, encrypted with the folder's.
    key: String,
    /// The file's name, which says its format.
    name: String,
}

/// A file to read, from wherever it comes.
trait File: Read + Seek + Send + Sync {}
impl<T: Read + Seek + Send + Sync> File for T {}

impl MegaAudio {
    /// Must be called on the tokio runtime, which opening runs on.
    pub fn new(cache: Arc<SongCache>) -> Self {
        Self {
            api: Arc::new(Api::new()),
            runtime: tokio::runtime::Handle::current(),
            cache,
            folders: Mutex::default(),
        }
    }
}

impl Source for MegaAudio {
    fn open(&self, uri: &SongUri) -> Result<Opened, OpenError> {
        let SongSource::Mega { folder, file, key } = uri.source() else {
            return Err(OpenError::Unavailable);
        };
        let folder_key = folder_key(key).ok_or(OpenError::Unavailable)?;
        let song = Song {
            uri: uri.as_str(),
            folder,
            file,
            folder_key,
        };
        self.runtime.block_on(self.open_song(&song))
    }
}

/// A song asked for, its URI read.
struct Song<'a> {
    uri: &'a str,
    folder: &'a str,
    file: &'a str,
    folder_key: [u8; 16],
}

impl Song<'_> {
    fn cache_key(&self) -> String {
        format!("mega/{}", self.file)
    }
}

impl MegaAudio {
    async fn open_song(&self, song: &Song<'_>) -> Result<Opened, OpenError> {
        let cache_key = song.cache_key();
        // A loop only to download a cached file again that didn't decode.
        loop {
            let kept = (self.cache.find(song.uri)).and_then(|found| {
                let record: Record = serde_json::from_value(found.details).ok()?;
                Some((record, found.file))
            });
            let cached = kept.is_some();
            let (record, file, download, length): (Record, Box<dyn File>, Box<dyn Download>, u64) =
                match kept {
                    Some((record, file)) => {
                        let length = file.metadata().map_err(|_| OpenError::Unavailable)?.len();
                        (record, Box::new(file), Box::new(Kept), length)
                    }
                    None => {
                        let node = self.node(song).await?;
                        let record = Record {
                            key: node.key.clone(),
                            name: file_name(&node, song)?,
                        };
                        let (file, progress) = self.download(song, &record).await?;
                        let length = file.size() as u64;
                        (record, Box::new(file), Box::new(progress), length)
                    }
                };
            let key = FileKey::open(&record.key, &song.folder_key).ok_or_else(|| {
                log::error!("the key of {} doesn't open", record.name);
                download.close();
                OpenError::Unavailable
            })?;
            let decrypted = Decrypt::new(Some(key.aes), key.iv, file);
            let media = Media::new(decrypted, 0, length).map_err(|e| {
                log::error!("unable to read {}: {e}", record.name);
                download.close();
                OpenError::Unavailable
            })?;
            let mut hint = Hint::new();
            if let Some((_, extension)) = record.name.rsplit_once('.') {
                hint.with_extension(&extension.to_lowercase());
            }
            let mut decoder = match Decoder::new(Box::new(media), &hint, MINIMUM_PART) {
                Ok(decoder) => decoder,
                Err(e) if cached => {
                    log::warn!("the cached file of {} doesn't decode ({e}); downloading it", record.name);
                    self.cache.remove(&cache_key);
                    continue;
                }
                Err(e) => {
                    log::error!("unable to decode {}: {e}", record.name);
                    let stalled = download.stalled();
                    download.close();
                    return Err(match stalled {
                        true => OpenError::Unreachable,
                        false => OpenError::Unavailable,
                    });
                }
            };
            let details = details(&mut decoder, song.uri, &record.name);
            let gains = decoder.tag_gains().unwrap_or_default();
            download.stream();
            log::info!(
                "{} ({} ms) opened from MEGA{}",
                details.title,
                details.duration_ms,
                if cached { "'s cache" } else { "" }
            );
            return Ok(Opened {
                details,
                decoder,
                gains,
                download,
            });
        }
    }

    /// The song's file in its folder's listing, the folder listed once.
    async fn node(&self, song: &Song<'_>) -> Result<Node, OpenError> {
        let listed = lock(&self.folders).get(song.folder).cloned();
        let files = match listed {
            Some(files) => files,
            None => {
                let nodes = self.api.list(song.folder).await.map_err(|e| {
                    log::error!("unable to list the MEGA folder: {e}");
                    open_error(&e)
                })?;
                let files: HashMap<_, _> = (nodes.into_iter())
                    .filter(|node| !node.folder)
                    .map(|node| (node.handle.clone(), node))
                    .collect();
                let files = Arc::new(files);
                lock(&self.folders).insert(song.folder.to_string(), Arc::clone(&files));
                files
            }
        };
        files.get(song.file).cloned().ok_or_else(|| {
            log::warn!("the song's file isn't in its MEGA folder any more");
            OpenError::Unavailable
        })
    }

    /// Starts downloading the song's file: once whole, it's kept with its
    /// record, found by the song's URI.
    async fn download(&self, song: &Song<'_>, record: &Record) -> Result<(Downloading, crate::playback::download::Progress), OpenError> {
        let (url, size) = self.api.download(song.folder, song.file).await.map_err(|e| {
            log::error!("unable to find where to download {}: {e}", record.name);
            open_error(&e)
        })?;
        let size = usize::try_from(size).map_err(|_| OpenError::Unavailable)?;
        let server = Arc::new(Server {
            client: self.api.downloads().clone(),
            url,
        });
        // An Ogg file's length is read from its last page: its end is asked
        // for beside its start, as MEGA takes a second to answer each.
        let tail = is_ogg(&record.name).then(|| size.saturating_sub(TAIL)..size);
        let tail = tail.filter(|tail| tail.start > MINIMUM_PART);
        let parts = async {
            let tail = async {
                // One slow to come is left for the download to fetch.
                let range = tail?;
                match tokio::time::timeout(TAIL_TIME, server.part(range)).await {
                    Ok(Ok(tail)) => Some(tail),
                    _ => None,
                }
            };
            tokio::join!(server.part(0..MINIMUM_PART.min(size)), tail)
        };
        let (head, tail) = match tokio::time::timeout(FIRST_PART_TIME, parts).await {
            Ok((Ok(head), tail)) => (head, tail),
            Ok((Err(fault), _)) => {
                log::error!("MEGA didn't give the start of {}: {fault}", record.name);
                return Err(match fault {
                    Refusal::OverQuota => OpenError::OverQuota,
                    Refusal::Other(_) => OpenError::Unreachable,
                });
            }
            Err(_) => {
                log::error!("MEGA didn't give the start of {} in time", record.name);
                return Err(OpenError::Unreachable);
            }
        };
        let cache = Arc::clone(&self.cache);
        let (kept, names) = (song.cache_key(), vec![song.uri.to_string()]);
        let details = serde_json::to_value(record).unwrap_or_default();
        Downloading::start(
            server,
            0,
            size,
            BYTES_PER_SECOND,
            head,
            tail,
            &self.cache.temporary_dir(),
            Box::new(move |file| {
                let kept_file = file
                    .seek(SeekFrom::Start(0))
                    .and_then(|_| cache.put(&kept, &names, &details, file));
                match kept_file {
                    Ok(()) => log::debug!("{kept} kept"),
                    Err(e) => log::error!("unable to keep {kept}: {e}"),
                }
            }),
        )
        .map_err(|e| {
            log::error!("unable to start downloading {}: {e}", record.name);
            OpenError::Unavailable
        })
    }
}

fn is_ogg(name: &str) -> bool {
    let name = name.to_lowercase();
    name.ends_with(".ogg") || name.ends_with(".oga")
}

/// The file's name, which says its format, from its attributes.
fn file_name(node: &Node, song: &Song<'_>) -> Result<String, OpenError> {
    FileKey::open(&node.key, &song.folder_key)
        .and_then(|key| key.name(&node.attributes))
        .ok_or_else(|| {
            log::error!("the MEGA file's key or name doesn't open");
            OpenError::Unavailable
        })
}

/// The song as its tags tell it, or its file's name where they don't.
fn details(decoder: &mut Decoder, uri: &str, file_name: &str) -> TrackInfo {
    let tags = decoder.tags();
    let plain_name = file_name.rsplit_once('.').map_or(file_name, |(name, _)| name);
    let artists = tags.artist.unwrap_or_default();
    TrackInfo {
        uri: uri.into(),
        title: tags.title.unwrap_or_else(|| plain_name.to_string()),
        main_artist: artists.split(", ").next().unwrap_or_default().to_string(),
        artists,
        album: tags.album.unwrap_or_default(),
        duration_ms: decoder.duration_ms().unwrap_or_default(),
        cover_url: None,
    }
}

fn open_error(error: &ApiError) -> OpenError {
    match error {
        ApiError::Unreachable(_) => OpenError::Unreachable,
        ApiError::OverQuota => OpenError::OverQuota,
        ApiError::NotFound | ApiError::Refused(_) => OpenError::Unavailable,
    }
}

/// Why the server didn't give a part.
#[derive(Debug, thiserror::Error)]
enum Refusal {
    #[error("over the download allowance")]
    OverQuota,
    #[error("{0}")]
    Other(String),
}

/// The one server MEGA names for a file.
struct Server {
    client: reqwest::Client,
    /// Holds a token, so it isn't logged.
    url: String,
}

impl Server {
    /// A part of the file, whole.
    async fn part(&self, range: Range<usize>) -> Result<Bytes, Refusal> {
        let wanted = range.len();
        let mut response = self.request(range).await?;
        let mut data = BytesMut::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| Refusal::Other(e.without_url().to_string()))?
        {
            data.extend_from_slice(&chunk);
        }
        if data.len() != wanted {
            return Err(Refusal::Other(format!("{} bytes of {wanted}", data.len())));
        }
        Ok(data.freeze())
    }

    /// Asks for `range`; answered with the part, or why not.
    fn request(&self, range: Range<usize>) -> BoxFuture<'static, Result<reqwest::Response, Refusal>> {
        let request = self
            .client
            .get(&self.url)
            .header(reqwest::header::RANGE, format!("bytes={}-{}", range.start, range.end - 1));
        async move {
            let response = request
                .send()
                .await
                .map_err(|e| Refusal::Other(e.without_url().to_string()))?;
            match response.status().as_u16() {
                206 => Ok(response),
                // The whole file, for a range that was all of it.
                200 if range.start == 0 => Ok(response),
                509 => Err(Refusal::OverQuota),
                status => Err(Refusal::Other(format!("answered {status}"))),
            }
        }
        .boxed()
    }
}

impl Remote for Server {
    fn servers(&self) -> usize {
        1
    }

    fn name(&self, _server: usize) -> String {
        self.url.split('/').nth(2).unwrap_or("MEGA").to_string()
    }

    fn get(&self, _server: usize, range: Range<usize>) -> BoxFuture<'static, Result<Body, Fault>> {
        let response = self.request(range);
        async move {
            let response = response.await.map_err(|e| Fault(e.to_string()))?;
            let body: Body = stream::unfold(Some(response), |response| async move {
                let mut response = response?;
                match response.chunk().await {
                    Ok(Some(chunk)) => Some((Ok(chunk), Some(response))),
                    Ok(None) => None,
                    Err(e) => Some((Err(Fault(e.without_url().to_string())), None)),
                }
            })
            .boxed();
            Ok(body)
        }
        .boxed()
    }

    fn gave_out(&self, _server: usize) {}

    /// MEGA takes about a second to answer each request, then sends fast.
    fn stretches(&self) -> bool {
        true
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}
