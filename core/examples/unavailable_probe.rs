//! Check: is a song that can't be played for want of a connection told from
//! one Spotify won't play? The first must wait for the connection, not be
//! skipped like the second, even while the session's socket hasn't noticed
//! the network went (it can take a minute and a half): whether the network
//! goes between songs, or in the middle of one still downloading.
//!
//! The session goes through a proxy run here, which then "loses the
//! network": it closes every connection but the session's own, and refuses
//! new ones. For the middle of a song it first slows downloads to about
//! twice the audio's rate. Plays at volume 0, so nothing is heard.
//!
//!   cargo run -p spotless-core --example unavailable_probe
//!
//! Sign in through the app first: this reuses its saved credential. Run it
//! with XDG_CACHE_HOME set to an empty directory, or the audio cache
//! answers instead of the network.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use librespot_core::SpotifyUri;
use librespot_core::config::SessionConfig;
use librespot_core::session::Session;
use spotless_core::paths::Paths;
use spotless_core::playback::engine::{Engine, EngineEvent, Status};
use spotless_core::spotify::{catalog, session};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::AbortHandle;

/// Boards of Canada, Music Has the Right to Children.
const DEFAULT_ALBUM: &str = "spotify:album:1vWnB0hYmluskQuzxwo25a";
/// A song that doesn't exist.
const MISSING: &str = "spotify:track:0000000000000000000000";
/// Slowed downloads: 80 kB/s, twice a 320 kbit/s song's rate.
const SLOW_CHUNK: usize = 16 * 1024;
const SLOW_PAUSE: Duration = Duration::from_millis(200);

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let proxy = Proxy::start().await?;
    let paths = Paths::new()?;
    let session = connect(&paths, &proxy).await?;
    let tracks = catalog::link_tracks(&session, DEFAULT_ALBUM).await?;
    let [_, first, second, third, fourth, ..] = tracks.as_slice() else {
        bail!("the album needs five songs");
    };

    let (sender, mut events) = mpsc::unbounded_channel();
    let mut engine = Engine::new(
        &session,
        0,
        false,
        Arc::new(move |event| {
            let _ = sender.send(event);
        }),
    )?;
    println!("a song that doesn't exist, network up");
    engine.load(uri(MISSING)?, 0);
    let seen = watch(&mut events, Duration::from_secs(4)).await;
    println!("  {}", verdict(seen.unavailable == Some(false), "skipped"));

    println!("\na song playing");
    engine.load(uri(&first.uri)?, 30_000);
    watch(&mut events, Duration::from_secs(5)).await;

    println!("\nnetwork lost, the session's socket left open; the next song");
    proxy.cut();
    engine.load(uri(&second.uri)?, 0);
    let seen = watch(&mut events, Duration::from_secs(5)).await;
    println!(
        "  {}",
        verdict(seen.unavailable == Some(true), "waits for the connection")
    );

    println!("\nnetwork back: a new session, and the song again");
    proxy.restore();
    engine.set_session(&connect(&paths, &proxy).await?);
    engine.load(uri(&second.uri)?, 0);
    watch(&mut events, Duration::from_secs(5)).await;

    println!("\ndownloads slowed, a song from its start");
    proxy.slow.store(true, Ordering::SeqCst);
    engine.load(uri(&third.uri)?, 0);
    watch_until(
        &mut events,
        Duration::from_secs(30),
        |event| matches!(event, EngineEvent::Snapshot(s) if s.status == Status::Playing),
    )
    .await;
    watch(&mut events, Duration::from_secs(5)).await;

    println!("\nnetwork lost in the middle of it, the session's socket left open");
    proxy.cut();
    let seen = watch_until(&mut events, Duration::from_secs(90), |event| {
        matches!(event, EngineEvent::Cut { .. })
    })
    .await;
    println!(
        "  {}",
        verdict(seen.cut.is_some(), "cut short, to go on from there")
    );

    println!("\nnetwork back: a new session, and the song from where it stopped");
    proxy.restore();
    proxy.slow.store(false, Ordering::SeqCst);
    engine.set_session(&connect(&paths, &proxy).await?);
    engine.load(uri(&third.uri)?, seen.cut.unwrap_or_default());
    watch(&mut events, Duration::from_secs(5)).await;

    println!("\ndownloads slowed, network lost while the next song loads");
    proxy.slow.store(true, Ordering::SeqCst);
    engine.load(uri(&fourth.uri)?, 0);
    watch(&mut events, Duration::from_secs(2)).await;
    proxy.cut();
    let began = Instant::now();
    let seen = watch_until(&mut events, Duration::from_secs(240), |event| {
        matches!(event, EngineEvent::Unavailable { .. })
    })
    .await;
    println!(
        "  {} after {} s",
        verdict(seen.unavailable == Some(true), "waits for the connection"),
        began.elapsed().as_secs()
    );
    engine.stop();
    Ok(())
}

/// A session through `proxy`, on the app's saved credential.
async fn connect(paths: &Paths, proxy: &Proxy) -> Result<Session> {
    let config = SessionConfig {
        proxy: Some(proxy.url.parse()?),
        ..SessionConfig::default()
    };
    let Some((session, credentials)) = session::unconnected(paths, config)? else {
        bail!("not signed in: start Spotless and sign in first");
    };
    session.connect(credentials, true).await?;
    Ok(session)
}

fn uri(text: &str) -> Result<SpotifyUri> {
    SpotifyUri::from_uri(text).context("bad URI")
}

fn verdict(ok: bool, expected: &str) -> String {
    if ok {
        format!("OK: {expected}")
    } else {
        format!("WRONG: not {expected}")
    }
}

/// What the engine said that matters here.
#[derive(Default)]
struct Seen {
    /// A song was unavailable: offline or not.
    unavailable: Option<bool>,
    /// A song was cut short here.
    cut: Option<u32>,
}

/// Prints what the engine reports for `long`.
async fn watch(events: &mut mpsc::UnboundedReceiver<EngineEvent>, long: Duration) -> Seen {
    watch_until(events, long, |_| false).await
}

/// Prints what the engine reports for `long`, or until `done`.
async fn watch_until(
    events: &mut mpsc::UnboundedReceiver<EngineEvent>,
    long: Duration,
    done: impl Fn(&EngineEvent) -> bool,
) -> Seen {
    let began = Instant::now();
    let mut seen = Seen::default();
    while let Ok(Some(event)) =
        tokio::time::timeout(long.saturating_sub(began.elapsed()), events.recv()).await
    {
        let at = began.elapsed().as_millis();
        let last = done(&event);
        match event {
            EngineEvent::Snapshot(snapshot) => println!(
                "  {at:>5} ms  {:?} at {} ms  {}",
                snapshot.status,
                snapshot.position_ms,
                snapshot.error.unwrap_or_default()
            ),
            EngineEvent::Unavailable { offline } => {
                seen.unavailable = Some(offline);
                println!("  {at:>5} ms  unavailable, offline: {offline}");
            }
            EngineEvent::Cut { position_ms } => {
                seen.cut = Some(position_ms);
                println!("  {at:>5} ms  cut at {position_ms} ms");
            }
            other => println!("  {at:>5} ms  {other:?}"),
        }
        if last {
            break;
        }
    }
    seen
}

/// An HTTP CONNECT proxy that can lose the network but for the session's
/// own connection, to an access point, and slow down the rest.
struct Proxy {
    url: String,
    cut: Arc<AtomicBool>,
    slow: Arc<AtomicBool>,
    /// Every connection but the access points'.
    tunnels: Arc<Mutex<Vec<AbortHandle>>>,
}

impl Proxy {
    async fn start() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let proxy = Self {
            url: format!("http://{}", listener.local_addr()?),
            cut: Arc::default(),
            slow: Arc::default(),
            tunnels: Arc::default(),
        };
        let (cut, slow, tunnels) = (
            Arc::clone(&proxy.cut),
            Arc::clone(&proxy.slow),
            Arc::clone(&proxy.tunnels),
        );
        tokio::spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                if cut.load(Ordering::SeqCst) {
                    continue;
                }
                let (slow, tunnels) = (Arc::clone(&slow), Arc::clone(&tunnels));
                tokio::spawn(async move {
                    let _ = tunnel(client, slow, tunnels).await;
                });
            }
        });
        Ok(proxy)
    }

    fn cut(&self) {
        self.cut.store(true, Ordering::SeqCst);
        for tunnel in self.tunnels.lock().unwrap().drain(..) {
            tunnel.abort();
        }
    }

    fn restore(&self) {
        self.cut.store(false, Ordering::SeqCst);
    }
}

async fn tunnel(
    client: TcpStream,
    slow: Arc<AtomicBool>,
    tunnels: Arc<Mutex<Vec<AbortHandle>>>,
) -> Result<()> {
    let mut client = BufReader::new(client);
    let mut request = String::new();
    client.read_line(&mut request).await?;
    let target = request
        .split_whitespace()
        .nth(1)
        .context("no target")?
        .to_owned();
    let mut line = String::new();
    while client.read_line(&mut line).await? > 2 {
        line.clear();
    }
    let server = TcpStream::connect(&target).await?;
    let mut client = client.into_inner();
    client.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await?;
    let access_point = target.starts_with("ap");
    let task = tokio::spawn(async move {
        let (mut client_read, mut client_write) = client.into_split();
        let (mut server_read, mut server_write) = server.into_split();
        let up = tokio::io::copy(&mut client_read, &mut server_write);
        let down = async {
            let mut buffer = vec![0; SLOW_CHUNK];
            while let Ok(read @ 1..) = server_read.read(&mut buffer).await {
                if client_write.write_all(&buffer[..read]).await.is_err() {
                    break;
                }
                if !access_point && slow.load(Ordering::SeqCst) {
                    tokio::time::sleep(SLOW_PAUSE).await;
                }
            }
        };
        let _ = tokio::join!(up, down);
    });
    if !access_point {
        tunnels.lock().unwrap().push(task.abort_handle());
    }
    Ok(())
}
