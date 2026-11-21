use std::sync::Mutex as StdMutex;
use std::time::Duration;

use futures_util::FutureExt;

use super::plan::MINIMUM_PART;
use super::*;

/// How each made-up server answers.
#[derive(Clone, Copy, PartialEq)]
enum Server {
    Good,
    /// Answers, then never sends a byte.
    Silent,
}

struct FakeRemote {
    data: Bytes,
    servers: Vec<Server>,
    asked: StdMutex<Vec<(usize, Range<usize>)>>,
    gave_out: StdMutex<Vec<usize>>,
}

impl FakeRemote {
    fn new(size: usize, servers: Vec<Server>) -> Arc<Self> {
        Arc::new(Self {
            data: (0..size).map(|i| (i % 251) as u8).collect(),
            servers,
            asked: StdMutex::default(),
            gave_out: StdMutex::default(),
        })
    }
}

impl Remote for FakeRemote {
    fn servers(&self) -> usize {
        self.servers.len()
    }

    fn name(&self, server: usize) -> String {
        format!("server {server}")
    }

    fn get(&self, server: usize, range: Range<usize>) -> BoxFuture<'static, Result<Body, Fault>> {
        self.asked.lock().unwrap().push((server, range.clone()));
        let data = self.data.slice(range);
        let kind = self.servers[server];
        async move {
            let body: Body = match kind {
                Server::Good => {
                    let chunks: Vec<_> = data.chunks(16 * 1024).map(Bytes::copy_from_slice).collect();
                    futures_util::stream::iter(chunks.into_iter().map(Ok)).boxed()
                }
                Server::Silent => futures_util::stream::pending().boxed(),
            };
            Ok(body)
        }
        .boxed()
    }

    fn gave_out(&self, server: usize) {
        self.gave_out.lock().unwrap().push(server);
    }
}

type Kept = Arc<StdMutex<Option<Vec<u8>>>>;

/// Starts downloading the remote's file with its first part in, as a source
/// opens it. Returns what's kept once it's whole.
fn start(remote: &Arc<FakeRemote>, rate: usize) -> (Downloading, Progress, Kept) {
    let kept: Kept = Arc::default();
    let on_whole = {
        let kept = Arc::clone(&kept);
        Box::new(move |file: &mut File| {
            let mut data = Vec::new();
            file.seek(SeekFrom::Start(0)).unwrap();
            file.read_to_end(&mut data).unwrap();
            *kept.lock().unwrap() = Some(data);
        })
    };
    let head = remote.data.slice(0..MINIMUM_PART.min(remote.data.len()));
    let (file, progress) = Downloading::start(
        Arc::clone(remote) as Arc<dyn Remote>,
        0,
        remote.data.len(),
        rate,
        head,
        &std::env::temp_dir(),
        on_whole,
    )
    .unwrap();
    (file, progress, kept)
}

#[tokio::test(flavor = "multi_thread")]
async fn reads_the_whole_file_as_it_comes_and_hands_it_over() {
    let remote = FakeRemote::new(1_000_000, vec![Server::Good]);
    let (mut file, progress, kept) = start(&remote, 40 * 1024);
    progress.stream();
    let read = tokio::task::spawn_blocking(move || {
        let mut all = Vec::new();
        file.read_to_end(&mut all).unwrap();
        all
    })
    .await
    .unwrap();
    assert_eq!(read, remote.data);
    for _ in 0..100 {
        if kept.lock().unwrap().is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(kept.lock().unwrap().as_deref(), Some(&remote.data[..]));
    assert!(progress.whole() && !progress.stalled());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_seek_reads_from_there_without_the_parts_before() {
    let remote = FakeRemote::new(4_000_000, vec![Server::Good]);
    let (mut file, _progress, _) = start(&remote, 40 * 1024);
    let read = tokio::task::spawn_blocking(move || {
        file.seek(SeekFrom::Start(3_000_000)).unwrap();
        let mut some = vec![0; 1000];
        file.read_exact(&mut some).unwrap();
        some
    })
    .await
    .unwrap();
    assert_eq!(read, remote.data[3_000_000..3_001_000]);
    let asked = remote.asked.lock().unwrap();
    assert!(asked.iter().all(|(_, r)| r.start >= 3_000_000), "{asked:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_silent_server_is_left_for_the_next_which_gives_the_part() {
    let remote = FakeRemote::new(500_000, vec![Server::Silent, Server::Good]);
    let (mut file, _progress, _) = start(&remote, 40 * 1024);
    let read = tokio::task::spawn_blocking(move || {
        file.seek(SeekFrom::Start(300_000)).unwrap();
        let mut some = vec![0; 100];
        file.read_exact(&mut some).unwrap();
        some
    })
    .await
    .unwrap();
    assert_eq!(read, remote.data[300_000..300_100]);
    assert_eq!(*remote.gave_out.lock().unwrap(), [0]);
}

#[tokio::test(flavor = "multi_thread")]
async fn with_no_server_giving_the_part_a_read_gives_up() {
    let remote = FakeRemote::new(500_000, vec![Server::Silent]);
    let (mut file, progress, _) = start(&remote, 40 * 1024);
    let result = tokio::task::spawn_blocking(move || {
        file.seek(SeekFrom::Start(300_000)).unwrap();
        file.read(&mut [0; 100])
    })
    .await
    .unwrap();
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
    assert!(progress.stalled());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_file_whose_head_is_all_of_it_is_handed_over_at_once() {
    let remote = FakeRemote::new(1000, vec![Server::Good]);
    let (_file, progress, kept) = start(&remote, 40 * 1024);
    for _ in 0..100 {
        if kept.lock().unwrap().is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(kept.lock().unwrap().as_deref(), Some(&remote.data[..]));
    assert!(progress.whole());
    assert!(remote.asked.lock().unwrap().is_empty());
}
