//! A song's file downloading from a source's servers while it's read: the
//! decoder reads it as a file, waiting for the parts not in yet, and a task
//! fetches the parts the plan (`plan`) asks for, one at a time, from the
//! source's servers. Once whole, it's handed over, to be kept in the cache.
//!
//! A server that sends nothing for a while, or too slowly to keep up, is
//! left for the next, and the part asked of it again, whole: a part is only
//! taken once all of it is in, so a lost connection is noticed by the read
//! waiting on it, not lost in the decoder's search through what came.

pub mod plan;
pub mod ranges;

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::ops::Range;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Instant;

use bytes::{Bytes, BytesMut};
use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use futures_util::stream::BoxStream;
use symphonia::core::io::MediaSource;
use tokio::sync::mpsc;

use self::plan::{Part, Plan, READ_AHEAD, READ_TIMEOUT, part_time, reaches, server_limit};
use super::source::Download;

/// Why a server didn't give a part.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct Fault(pub String);

/// A part's bytes as they come.
pub type Body = BoxStream<'static, Result<Bytes, Fault>>;

/// The servers a file is downloaded from.
pub trait Remote: Send + Sync + 'static {
    /// How many there are.
    fn servers(&self) -> usize;
    /// The server's name, for the log.
    fn name(&self, server: usize) -> String;
    /// Asks `server` for the bytes in `range`.
    fn get(&self, server: usize, range: Range<usize>) -> BoxFuture<'static, Result<Body, Fault>>;
    /// The server stopped sending, or sent too slowly.
    fn gave_out(&self, server: usize);
    /// Whether it's asked for whole stretches (`plan`), not parts: its
    /// server takes long to answer each request, then sends fast. Its
    /// first server alone is asked.
    fn stretches(&self) -> bool {
        false
    }
}

/// Handed the file once it's whole.
pub type OnWhole = Box<dyn FnOnce(&mut File) + Send>;

struct Shared {
    plan: Mutex<Plan>,
    /// Woken as parts come in, or are given up.
    changed: Condvar,
    /// A read gave up waiting for its part.
    stalled: AtomicBool,
    /// The server asked now. It stays on the last that answered.
    server: AtomicUsize,
    servers: usize,
    commands: mpsc::UnboundedSender<Command>,
}

enum Command {
    Ask(Vec<Part>),
    Close,
}

impl Shared {
    fn plan(&self) -> MutexGuard<'_, Plan> {
        self.plan.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn ask(&self, parts: Vec<Part>) {
        if !parts.is_empty() {
            // Closed: the file is whole, or no longer wanted.
            let _ = self.commands.send(Command::Ask(parts));
        }
    }

    /// Waits for `range` to be in, as asked for already. Gives up once
    /// nothing has come for a while: the network's gone. A part that failed
    /// isn't asked for again here, which would spin while offline: the
    /// next read asks.
    fn wait_for(&self, range: Range<usize>) -> Result<(), io::Error> {
        let mut plan = self.plan();
        while plan.downloaded.run_from(range.start) < range.len() {
            let (waited, timeout) = self
                .changed
                .wait_timeout(plan, READ_TIMEOUT)
                .unwrap_or_else(|p| p.into_inner());
            plan = waited;
            if timeout.timed_out() {
                self.stalled.store(true, Ordering::Release);
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "the file stopped downloading",
                ));
            }
        }
        Ok(())
    }
}

/// A file downloading: read it as it comes in.
pub struct Downloading {
    file: File,
    size: usize,
    position: u64,
    /// A seek to a part not downloaded yet stopped streaming: the decoder's
    /// reads only look for its place, until one follows on from the last.
    probing: bool,
    /// Seeked since the last read.
    seeked: bool,
    shared: Arc<Shared>,
}

/// How a downloading file is coming in, for the player.
pub struct Progress {
    shared: Arc<Shared>,
}

impl Downloading {
    /// Starts downloading a file of `size` bytes from `remote`, whose
    /// `server` gave its first `head` bytes, into a file in `dir`. Must be
    /// called on the tokio runtime, which the download runs on.
    pub fn start(
        remote: Arc<dyn Remote>,
        server: usize,
        size: usize,
        bytes_per_second: usize,
        head: Bytes,
        dir: &std::path::Path,
        on_whole: OnWhole,
    ) -> io::Result<(Self, Progress)> {
        std::fs::create_dir_all(dir)?;
        let mut write = tempfile::NamedTempFile::new_in(dir)?;
        write.as_file().set_len(size as u64)?;
        write.write_all(&head)?;
        let read = write.reopen()?;
        let mut plan = Plan::new(size, bytes_per_second);
        plan.received(0..head.len().min(size));
        let (commands, received) = mpsc::unbounded_channel();
        let shared = Arc::new(Shared {
            plan: Mutex::new(plan),
            changed: Condvar::new(),
            stalled: AtomicBool::new(false),
            server: AtomicUsize::new(server),
            servers: remote.servers(),
            commands,
        });
        if remote.stretches() {
            tokio::spawn(fetch_stretches(Arc::clone(&shared), remote, write, received, on_whole));
        } else {
            tokio::spawn(fetch(Arc::clone(&shared), remote, write, received, on_whole));
        }
        Ok((
            Self {
                file: read,
                size,
                position: 0,
                probing: false,
                seeked: false,
                shared: Arc::clone(&shared),
            },
            Progress { shared },
        ))
    }
}

impl Read for Downloading {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // A read following on from the last: the decoder found its place
        // and plays on, so stream again.
        if self.probing && !self.seeked {
            self.probing = false;
            self.shared.plan().streaming = true;
        }
        self.seeked = false;
        let offset = self.position as usize;
        if offset >= self.size {
            return Ok(0);
        }
        let length = buf.len().min(self.size - offset);
        if length == 0 {
            return Ok(0);
        }
        let parts = self.shared.plan().for_read(offset, length);
        self.shared.ask(parts);
        self.shared.wait_for(offset..offset + 1)?;
        let available = self.shared.plan().downloaded.run_from(offset);
        self.file.seek(SeekFrom::Start(offset as u64))?;
        let read = self.file.read(&mut buf[..length.min(available)])?;
        self.position += read as u64;
        self.shared.plan().read_position = self.position as usize;
        Ok(read)
    }
}

impl Seek for Downloading {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let at = match position {
            SeekFrom::Start(at) => at as i64,
            SeekFrom::End(back) => self.size as i64 + back,
            SeekFrom::Current(by) => self.position as i64 + by,
        };
        if at < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "before the start"));
        }
        let at = at as u64;
        if at == self.position {
            return Ok(at);
        }
        if self.shared.plan().seek(at as usize) {
            self.probing = true;
        }
        self.seeked = true;
        self.position = at;
        Ok(at)
    }
}

impl MediaSource for Downloading {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.size as u64)
    }
}

impl Downloading {
    /// The whole file's size.
    pub fn size(&self) -> usize {
        self.size
    }
}

impl Download for Progress {
    fn stream(&self) {
        self.shared.plan().streaming = true;
    }

    fn whole(&self) -> bool {
        self.shared.plan().to_end()
    }

    fn stalled(&self) -> bool {
        self.shared.stalled.load(Ordering::Acquire)
    }

    fn read_ahead(&self) {
        let (position, length, parts) = {
            let mut plan = self.shared.plan();
            let position = plan.read_position;
            let length = ((READ_AHEAD.as_secs_f32() * plan.bytes_per_second as f32) as usize)
                .min(plan.size.saturating_sub(position));
            let parts = plan.ask(position..position + length);
            (position, length, parts)
        };
        self.shared.ask(parts);
        if let Err(e) = self.shared.wait_for(position..position + length) {
            log::warn!("reading ahead after a seek: {e}");
        }
    }

    fn close(&self) {
        let _ = self.shared.commands.send(Command::Close);
    }
}

/// A part fetched, or given up.
enum Fetched {
    In {
        range: Range<usize>,
        data: Bytes,
        /// How long the first server asked took to answer, and the whole
        /// part took, when it came from that one.
        timing: Option<(std::time::Duration, std::time::Duration)>,
    },
    Failed(Range<usize>),
}

/// Fetches the parts asked for, one at a time, until the file is whole or
/// closed.
async fn fetch(
    shared: Arc<Shared>,
    remote: Arc<dyn Remote>,
    mut file: tempfile::NamedTempFile,
    mut commands: mpsc::UnboundedReceiver<Command>,
    on_whole: OnWhole,
) {
    let (done, mut fetched) = mpsc::unbounded_channel();
    let mut queue = std::collections::VecDeque::new();
    let mut busy = false;
    // After a part failed, nothing is fetched ahead until a read asks:
    // fetching again at once would spin while offline.
    let mut quiet = false;
    if shared.plan().whole() {
        on_whole(file.as_file_mut());
        return;
    }
    loop {
        tokio::select! {
            command = commands.recv() => match command {
                Some(Command::Ask(parts)) => {
                    quiet = false;
                    queue.extend(parts);
                }
                Some(Command::Close) | None => break,
            },
            Some(result) = fetched.recv() => {
                busy = false;
                match result {
                    Fetched::In { range, data, timing } => {
                        let written = file
                            .as_file_mut()
                            .seek(SeekFrom::Start(range.start as u64))
                            .and_then(|_| file.as_file_mut().write_all(&data));
                        if let Err(e) = written {
                            log::error!("unable to write the download: {e}");
                            break;
                        }
                        let whole = {
                            let mut plan = shared.plan();
                            plan.received(range.clone());
                            if let Some((ping, took)) = timing {
                                plan.measured_ping(ping);
                                plan.measured_throughput(range.len(), took);
                            }
                            plan.whole()
                        };
                        shared.changed.notify_all();
                        if whole {
                            log::debug!("download whole");
                            on_whole(file.as_file_mut());
                            break;
                        }
                    }
                    Fetched::Failed(range) => {
                        shared.plan().dropped(range);
                        shared.changed.notify_all();
                        quiet = true;
                    }
                }
            }
        }
        if busy {
            continue;
        }
        // The next part asked for that's still wanted; else, ahead.
        let next = loop {
            let Some(part) = queue.pop_front() else {
                break None;
            };
            let mut plan = shared.plan();
            if plan.stale(&part) {
                plan.dropped(part.range.clone());
                drop(plan);
                shared.changed.notify_all();
                continue;
            }
            break Some(part);
        };
        let next = next.or_else(|| {
            if quiet {
                return None;
            }
            let mut ahead = shared.plan().ahead();
            let first = (!ahead.is_empty()).then(|| ahead.remove(0));
            queue.extend(ahead);
            first
        });
        if let Some(part) = next {
            busy = true;
            let (shared, remote, done) = (Arc::clone(&shared), Arc::clone(&remote), done.clone());
            tokio::spawn(async move {
                let result = fetch_part(&shared, remote.as_ref(), part.range, &done).await;
                let _ = done.send(result);
            });
        }
    }
}

/// What a stretch's request brings, by the stretch's number.
enum Streamed {
    Chunk { stretch: u64, data: Bytes },
    /// All of it came.
    Ended { stretch: u64 },
    Failed { stretch: u64, fault: Fault },
}

/// A stretch being fetched: what's still coming of it.
struct Stretch {
    coming: Range<usize>,
    /// The main one, which goes on to what's left once it ends; the others
    /// are reads' that it wouldn't reach soon.
    main: bool,
    started: Instant,
    came: usize,
    /// Its speed as measured since `window_since`.
    window_since: Instant,
    window_came: usize,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Stretch {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Fetches the file in stretches until it's whole or closed: the main one
/// from where it's read, then what's left, never given up for a read
/// elsewhere; a read it won't reach soon gets a short one of its own.
async fn fetch_stretches(
    shared: Arc<Shared>,
    remote: Arc<dyn Remote>,
    mut file: tempfile::NamedTempFile,
    mut commands: mpsc::UnboundedReceiver<Command>,
    on_whole: OnWhole,
) {
    if shared.plan().whole() {
        on_whole(file.as_file_mut());
        return;
    }
    let (sent, mut streamed) = mpsc::unbounded_channel();
    let mut numbered = 0;
    let mut stretches: std::collections::HashMap<u64, Stretch> = Default::default();
    let mut start = |range: Range<usize>, main: bool| {
        numbered += 1;
        let number = numbered;
        log::debug!(
            "asking for {}+{}{}",
            range.start,
            range.len(),
            if main { "" } else { " beside" }
        );
        let (remote, sent) = (Arc::clone(&remote), sent.clone());
        let asked = range.clone();
        let task = tokio::spawn(async move {
            let streamed = match remote.get(0, asked.clone()).await {
                Ok(mut body) => {
                    let mut came = 0;
                    loop {
                        match body.next().await {
                            Some(Ok(data)) => {
                                came += data.len();
                                let _ = sent.send(Streamed::Chunk { stretch: number, data });
                            }
                            Some(Err(fault)) => break Streamed::Failed { stretch: number, fault },
                            None if came < asked.len() => {
                                let fault = Fault(format!("{came} bytes came of {}", asked.len()));
                                break Streamed::Failed { stretch: number, fault };
                            }
                            None => break Streamed::Ended { stretch: number },
                        }
                    }
                }
                Err(fault) => Streamed::Failed { stretch: number, fault },
            };
            let _ = sent.send(streamed);
        });
        let stretch = Stretch {
            coming: range,
            main,
            started: Instant::now(),
            came: 0,
            window_since: Instant::now(),
            window_came: 0,
            task,
        };
        (number, stretch)
    };
    if let Some(range) = shared.plan().next_stretch() {
        let (number, stretch) = start(range, true);
        stretches.insert(number, stretch);
    }
    mark_coming(&mut shared.plan(), &stretches);
    // Slow stretches asked again in a row having brought nothing.
    let mut empty = 0;
    let mut check = tokio::time::interval(std::time::Duration::from_secs(1));
    loop {
        tokio::select! {
            _ = check.tick() => {
                let mut plan = shared.plan();
                let rate = plan.bytes_per_second;
                let slow: Vec<u64> = (stretches.iter())
                    .filter(|(_, s)| plan::too_slow(s.window_came, s.window_since.elapsed(), rate))
                    .map(|(n, _)| *n)
                    .collect();
                for stretch in stretches.values_mut() {
                    if stretch.window_since.elapsed() >= plan::SLOW_WINDOW {
                        stretch.window_since = Instant::now();
                        stretch.window_came = 0;
                    }
                }
                for n in slow {
                    let Some(slow) = stretches.remove(&n) else { continue };
                    empty = if slow.came == 0 { empty + 1 } else { 0 };
                    if empty >= plan::EMPTY_TRIES {
                        log::error!("{} gives nothing; waiting for a read to ask", remote.name(0));
                        continue;
                    }
                    let Some(range) = plan.stretch_from(slow.coming.start) else { continue };
                    log::warn!(
                        "{} sends {}+{} too slowly; asking again",
                        remote.name(0),
                        slow.coming.start,
                        slow.coming.len()
                    );
                    let range = if slow.main { range } else { plan::beside(range) };
                    let (number, stretch) = start(range, slow.main);
                    stretches.insert(number, stretch);
                }
                mark_coming(&mut plan, &stretches);
            }
            command = commands.recv() => match command {
                Some(Command::Ask(parts)) => {
                    empty = 0;
                    let Some(wanted) = parts.first().map(|part| part.range.start) else {
                        continue;
                    };
                    let mut plan = shared.plan();
                    let waits = stretches
                        .values()
                        .any(|s| reaches(&s.coming, wanted, reach(s)));
                    if !waits && let Some(range) = plan.stretch_from(wanted) {
                        // With no main stretch (it failed), this one is.
                        let main = !stretches.values().any(|s| s.main);
                        let range = if main { range } else { plan::beside(range) };
                        let (number, stretch) = start(range, main);
                        stretches.insert(number, stretch);
                    }
                    mark_coming(&mut plan, &stretches);
                }
                Some(Command::Close) | None => break,
            },
            Some(news) = streamed.recv() => match news {
                Streamed::Chunk { stretch: n, data } => {
                    let Some(stretch) = stretches.get_mut(&n) else { continue };
                    let at = stretch.coming.start;
                    let data = data.slice(..data.len().min(stretch.coming.len()));
                    let written = file
                        .as_file_mut()
                        .seek(SeekFrom::Start(at as u64))
                        .and_then(|_| file.as_file_mut().write_all(&data));
                    if let Err(e) = written {
                        log::error!("unable to write the download: {e}");
                        break;
                    }
                    stretch.coming.start += data.len();
                    stretch.came += data.len();
                    stretch.window_came += data.len();
                    empty = 0;
                    let whole = {
                        let mut plan = shared.plan();
                        plan.received(at..at + data.len());
                        mark_coming(&mut plan, &stretches);
                        plan.whole()
                    };
                    shared.changed.notify_all();
                    if whole {
                        log::debug!("download whole");
                        on_whole(file.as_file_mut());
                        break;
                    }
                }
                Streamed::Ended { stretch: n } => {
                    let Some(ended) = stretches.remove(&n) else { continue };
                    let mut plan = shared.plan();
                    if ended.main
                        && let Some(range) = plan.next_stretch()
                    {
                        let (number, stretch) = start(range, true);
                        stretches.insert(number, stretch);
                    }
                    mark_coming(&mut plan, &stretches);
                }
                Streamed::Failed { stretch: n, fault } => {
                    let Some(failed) = stretches.remove(&n) else { continue };
                    let mut plan = shared.plan();
                    // A connection dropped midway is asked again at once,
                    // from where it got to. One that brought nothing waits
                    // for the next read to ask: asking here would spin
                    // while offline.
                    let again = (failed.came > 0)
                        .then(|| plan.stretch_from(failed.coming.start))
                        .flatten();
                    match again {
                        Some(range) => {
                            log::warn!("{} stopped giving the file ({fault}); asking again", remote.name(0));
                            let range = if failed.main { range } else { plan::beside(range) };
                            let (number, stretch) = start(range, failed.main);
                            stretches.insert(number, stretch);
                        }
                        None => log::error!("{} stopped giving the file: {fault}", remote.name(0)),
                    }
                    mark_coming(&mut plan, &stretches);
                    drop(plan);
                    shared.changed.notify_all();
                }
            }
        }
    }
}

/// How far ahead of where `stretch` has come a read waits for it: what it
/// brings in about a second, as measured.
fn reach(stretch: &Stretch) -> usize {
    let elapsed = stretch.started.elapsed().as_secs_f64();
    let rate = if elapsed > 0.5 { stretch.came as f64 / elapsed } else { 0.0 };
    plan::stretch_reach(rate as usize)
}

/// Only what's soon to come of the stretches counts as asked for: a read
/// further on asks, and gets a stretch of its own if none reaches it.
fn mark_coming(plan: &mut Plan, stretches: &std::collections::HashMap<u64, Stretch>) {
    let mut requested = ranges::Ranges::new();
    for stretch in stretches.values() {
        let coming = &stretch.coming;
        let end = coming.end.min(coming.start + reach(stretch));
        if coming.start < end {
            requested.add(coming.start..end);
        }
    }
    plan.requested = requested;
}

/// Fetches a part from the servers in turn, starting with the one asked
/// now, until one gives all of it.
async fn fetch_part(
    shared: &Shared,
    remote: &dyn Remote,
    range: Range<usize>,
    done: &mpsc::UnboundedSender<Fetched>,
) -> Fetched {
    let bytes_per_second = shared.plan().bytes_per_second;
    let mut servers_left = shared.servers.saturating_sub(1);
    let mut first = true;
    loop {
        let server = shared.server.load(Ordering::Acquire);
        let started = Instant::now();
        let limit = server_limit(servers_left);
        match fetch_from(remote, server, range.clone(), limit, bytes_per_second, done).await {
            Ok((data, ping)) => {
                let timing = first.then(|| (ping, started.elapsed()));
                return Fetched::In {
                    range,
                    data,
                    timing,
                };
            }
            Err(e) if servers_left == 0 || done.is_closed() => {
                if !done.is_closed() {
                    log::error!("no server gave {}+{}: {e}", range.start, range.len());
                }
                return Fetched::Failed(range);
            }
            Err(e) => {
                log::warn!(
                    "{} didn't give {}+{}: {e}; asking the next, and it goes last from now on",
                    remote.name(server),
                    range.start,
                    range.len(),
                );
                remote.gave_out(server);
                let next = (server + 1) % shared.servers.max(1);
                let _ = shared.server.compare_exchange(
                    server,
                    next,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                );
                servers_left -= 1;
                // The time this one took says nothing of the next.
                first = false;
            }
        }
    }
}

/// Asks one server for the whole part: it may send nothing for `limit` at
/// a time, and take no longer than the part takes to play and `limit`.
/// Returns the part and how long the server took to answer.
async fn fetch_from(
    remote: &dyn Remote,
    server: usize,
    range: Range<usize>,
    limit: Option<std::time::Duration>,
    bytes_per_second: usize,
    done: &mpsc::UnboundedSender<Fetched>,
) -> Result<(Bytes, std::time::Duration), Fault> {
    let started = Instant::now();
    let deadline = limit.map(|limit| started + part_time(range.len(), bytes_per_second, limit));
    let wait = || {
        limit.map(|limit| {
            let left = deadline.map_or(limit, |d| d.saturating_duration_since(Instant::now()));
            limit.min(left)
        })
    };
    let mut body = within(wait(), done, remote.get(server, range.clone())).await??;
    let ping = started.elapsed();
    let mut data = BytesMut::with_capacity(range.len());
    while let Some(chunk) = within(wait(), done, body.next()).await? {
        data.extend_from_slice(&chunk?);
    }
    if data.len() != range.len() {
        return Err(Fault(format!(
            "{} bytes came of {}",
            data.len(),
            range.len()
        )));
    }
    Ok((data.freeze(), ping))
}

/// Waits for `future`, for up to `limit`, and no longer than the file is
/// wanted.
async fn within<F: Future>(
    limit: Option<std::time::Duration>,
    done: &mpsc::UnboundedSender<Fetched>,
    future: F,
) -> Result<F::Output, Fault> {
    let expired = async {
        match limit {
            Some(limit) => tokio::time::sleep(limit).await,
            None => std::future::pending().await,
        }
    };
    tokio::select! {
        output = future => Ok(output),
        _ = done.closed() => Err(Fault("the file is no longer wanted".into())),
        _ = expired => Err(Fault("nothing came in time".into())),
    }
}

#[cfg(test)]
mod tests;
