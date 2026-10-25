//! Check: is a song that can't be loaded for want of a connection told from
//! one Spotify won't play? The first must wait for the connection, not be
//! skipped like the second, even while the session's socket hasn't noticed
//! the network went (it can take a minute and a half).
//!
//! The session goes through a proxy run here, which then "loses the
//! network": it closes every connection but the session's own, and refuses
//! new ones. Plays at volume 0, so nothing is heard.
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
use spotless_core::paths::Paths;
use spotless_core::playback::engine::{Engine, EngineEvent};
use spotless_core::spotify::{catalog, session};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::AbortHandle;

/// Boards of Canada, Music Has the Right to Children.
const DEFAULT_ALBUM: &str = "spotify:album:1vWnB0hYmluskQuzxwo25a";
/// A song that doesn't exist.
const MISSING: &str = "spotify:track:0000000000000000000000";

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let proxy = Proxy::start().await?;
    let paths = Paths::new()?;
    let config = || SessionConfig {
        proxy: Some(proxy.url.parse().expect("proxy URL")),
        ..SessionConfig::default()
    };
    let Some((session, credentials)) = session::unconnected(&paths, config())? else {
        bail!("not signed in: start Spotless and sign in first");
    };
    session.connect(credentials, true).await?;
    let tracks = catalog::link_tracks(&session, DEFAULT_ALBUM).await?;
    let (Some(first), Some(second)) = (tracks.get(1), tracks.get(2)) else {
        bail!("the album needs three songs");
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
    let offline = watch(&mut events, Duration::from_secs(4)).await;
    println!("  {}", verdict(offline == Some(false), "skipped"));

    println!("\na song playing");
    engine.load(uri(&first.uri)?, 30_000);
    watch(&mut events, Duration::from_secs(5)).await;

    println!("\nnetwork lost, the session's socket left open; the next song");
    proxy.cut();
    engine.load(uri(&second.uri)?, 0);
    let offline = watch(&mut events, Duration::from_secs(5)).await;
    println!(
        "  {}",
        verdict(offline == Some(true), "waits for the connection")
    );

    println!("\nnetwork back: a new session, and the song again");
    proxy.restore();
    let Some((new, credentials)) = session::unconnected(&paths, config())? else {
        bail!("signed out meanwhile");
    };
    new.connect(credentials, true).await?;
    engine.set_session(&new);
    engine.load(uri(&second.uri)?, 0);
    watch(&mut events, Duration::from_secs(5)).await;
    engine.stop();
    Ok(())
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

/// Prints what the engine reports for `long`. Returns whether a song was
/// unavailable offline, if one was.
async fn watch(events: &mut mpsc::UnboundedReceiver<EngineEvent>, long: Duration) -> Option<bool> {
    let began = Instant::now();
    let mut unavailable = None;
    while let Ok(Some(event)) =
        tokio::time::timeout(long.saturating_sub(began.elapsed()), events.recv()).await
    {
        let at = began.elapsed().as_millis();
        match event {
            EngineEvent::Snapshot(snapshot) => println!(
                "  {at:>5} ms  {:?} at {} ms  {}",
                snapshot.status,
                snapshot.position_ms,
                snapshot.error.unwrap_or_default()
            ),
            EngineEvent::Unavailable { offline } => {
                unavailable = Some(offline);
                println!("  {at:>5} ms  unavailable, offline: {offline}");
            }
            other => println!("  {at:>5} ms  {other:?}"),
        }
    }
    unavailable
}

/// An HTTP CONNECT proxy that can lose the network but for the session's
/// own connection, to an access point.
struct Proxy {
    url: String,
    cut: Arc<AtomicBool>,
    /// Every connection but the access points'.
    tunnels: Arc<Mutex<Vec<AbortHandle>>>,
}

impl Proxy {
    async fn start() -> Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}", listener.local_addr()?);
        let cut = Arc::new(AtomicBool::new(false));
        let tunnels = Arc::new(Mutex::new(Vec::new()));
        let (accepting, tunnelling) = (Arc::clone(&cut), Arc::clone(&tunnels));
        tokio::spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                if accepting.load(Ordering::SeqCst) {
                    continue;
                }
                let tunnels = Arc::clone(&tunnelling);
                tokio::spawn(async move {
                    let _ = tunnel(client, tunnels).await;
                });
            }
        });
        Ok(Self { url, cut, tunnels })
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

async fn tunnel(client: TcpStream, tunnels: Arc<Mutex<Vec<AbortHandle>>>) -> Result<()> {
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
    let mut server = TcpStream::connect(&target).await?;
    let mut client = client.into_inner();
    client.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await?;
    let task = tokio::spawn(async move {
        let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
    });
    if !target.starts_with("ap") {
        tunnels.lock().unwrap().push(task.abort_handle());
    }
    Ok(())
}
