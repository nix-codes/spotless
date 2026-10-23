//! Can Spotless watch Spotify Connect without taking part in it?
//!
//! Registers as a hidden, uncontrollable Connect device that never plays, and
//! prints every change Spotify pushes: which device is active, and whether
//! it plays. With `--pause`, the first time another device is seen playing,
//! it waits 10 s and then tells that device to pause.
//!
//! While it runs: start and stop playing on a phone, and check that
//! "Spotless probe" is NOT in the phone's list of devices.
//!
//!   cargo run -p spotless-core --example listen_probe -- [--pause]
//!
//! Sign in through the app first: this reuses its saved credential.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use librespot_core::cache::Cache;
use librespot_core::dealer::protocol::Message;
use librespot_core::{Session, SessionConfig};
use librespot_protocol::connect::{
    Capabilities, Cluster, ClusterUpdate, Device, DeviceInfo, MemberType, PutStateReason,
    PutStateRequest,
};
use librespot_protocol::devices::DeviceType;
use protobuf::{EnumOrUnknown, Message as _, MessageField};
use reqwest::Method;
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};
use spotless_core::paths::Paths;

const LISTEN: Duration = Duration::from_secs(300);
const PAUSE_AFTER: Duration = Duration::from_secs(10);

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .format_timestamp_millis()
        .init();
    let pause = std::env::args().any(|arg| arg == "--pause");
    let paths = Paths::new()?;
    let cache = Cache::new(Some(paths.config.as_path()), None, None, None)?;
    let Some(credentials) = cache.credentials() else {
        bail!("not signed in: start Spotless and sign in first");
    };
    let session = Session::new(SessionConfig::default(), Some(cache));

    // Listeners first: the dealer takes them when it starts.
    let mut connection_ids =
        session
            .dealer()
            .listen_for("hm://pusher/v1/connections/", |message: Message| {
                message
                    .headers
                    .get("Spotify-Connection-Id")
                    .cloned()
                    .context("no connection ID")
                    .map_err(|e| librespot_core::Error::failed_precondition(e.to_string()))
            })?;
    let mut updates = session.dealer().listen_for(
        "hm://connect-state/v1/cluster",
        Message::from_raw::<ClusterUpdate>,
    )?;
    session.connect(credentials, true).await?;
    session.dealer().start().await?;
    println!("device ID {}", session.device_id());

    let began = Instant::now();
    let mut pause_at: Option<(Instant, String)> = None;
    let mut paused = false;
    while began.elapsed() < LISTEN {
        let t = || began.elapsed().as_secs_f32();
        tokio::select! {
            Some(id) = connection_ids.next() => {
                let id = id?;
                session.set_connection_id(&id);
                let reply = session
                    .spclient()
                    .put_connect_state_request(&register(&session))
                    .await
                    .context("registering failed")?;
                println!("{:6.1}s  registered", t());
                let cluster = Cluster::parse_from_bytes(&reply)?;
                print_cluster(t(), "now", &cluster, &session);
            }
            Some(update) = updates.next() => {
                let update = update?;
                let Some(cluster) = update.cluster.as_ref() else { continue };
                let reason = format!("{:?}", update.update_reason.enum_value_or_default());
                print_cluster(t(), &reason, cluster, &session);
                let other = &cluster.active_device_id;
                let playing = cluster.player_state.is_playing && !cluster.player_state.is_paused;
                if pause && !paused && pause_at.is_none() && playing && other != session.device_id() {
                    println!("{:6.1}s  pausing it in {} s", t(), PAUSE_AFTER.as_secs());
                    pause_at = Some((Instant::now() + PAUSE_AFTER, other.clone()));
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(250)) => {}
        }
        if let Some((_, device)) = pause_at.take_if(|(at, _)| Instant::now() >= *at) {
            paused = true;
            match send_pause(&session, &device).await {
                Ok(()) => println!("{:6.1}s  pause sent", t()),
                Err(e) => println!("{:6.1}s  pause failed: {e:#}", t()),
            }
        }
    }
    let _ = session.spclient().delete_connect_state_request().await;
    println!("unregistered");
    Ok(())
}

/// A device that watches but never plays, and that others can't see or
/// control.
fn register(session: &Session) -> PutStateRequest {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    PutStateRequest {
        member_type: EnumOrUnknown::new(MemberType::CONNECT_STATE),
        put_state_reason: EnumOrUnknown::new(PutStateReason::NEW_DEVICE),
        is_active: false,
        client_side_timestamp: now,
        device: MessageField::some(Device {
            device_info: MessageField::some(DeviceInfo {
                can_play: false,
                name: "Spotless probe".into(),
                device_id: session.device_id().to_string(),
                device_type: EnumOrUnknown::new(DeviceType::COMPUTER),
                client_id: session.client_id(),
                capabilities: MessageField::some(Capabilities {
                    is_observable: true,
                    is_controllable: false,
                    hidden: true,
                    can_be_player: false,
                    gaia_eq_connect_id: true,
                    supports_gzip_pushes: true,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn print_cluster(t: f32, reason: &str, cluster: &Cluster, session: &Session) {
    let name = |id: &str| {
        if id.is_empty() {
            "none".to_string()
        } else if id == session.device_id() {
            "THIS PROBE".to_string()
        } else {
            cluster
                .device
                .get(id)
                .map_or_else(|| id.to_string(), |d| d.name.clone())
        }
    };
    let state = &cluster.player_state;
    let status = match (state.is_playing, state.is_paused) {
        (true, false) => "playing",
        (true, true) => "paused",
        _ => "stopped",
    };
    let track = state.track.as_ref().map_or("", |t| t.uri.as_str());
    println!(
        "{t:6.1}s  {reason}: active {} {status} {track}",
        name(&cluster.active_device_id)
    );
    let listed: Vec<_> = cluster.device.keys().map(|id| name(id)).collect();
    println!("          devices: {}", listed.join(", "));
}

/// Asks `device` to pause, the way Spotify's own apps do.
async fn send_pause(session: &Session, device: &str) -> Result<()> {
    let endpoint = format!(
        "/connect-state/v1/player/command/from/{}/to/{device}",
        session.device_id()
    );
    let body = serde_json::json!({
        "command": {
            "endpoint": "pause",
            "logging_params": { "command_id": format!("{:032x}", fastrand::u128(..)) },
        }
    })
    .to_string();
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    session
        .spclient()
        .request(
            &Method::POST,
            &endpoint,
            Some(headers),
            Some(body.as_bytes()),
        )
        .await?;
    Ok(())
}
