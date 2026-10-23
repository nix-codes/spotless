//! The account's other devices, watched so that only one of them plays.
//!
//! Spotify allows one stream per account, but it never stops Spotless: the
//! engine plays without Spotify Connect, so Spotify doesn't see it. Spotless
//! keeps to the rule itself. It joins Connect as a hidden device that never
//! plays and can't be controlled, so it isn't in other devices' lists and
//! its own playback stays unseen, and it follows which device plays. When
//! another one starts, Spotless pauses; when Spotless starts, it pauses the
//! other one, as Spotify's own apps do.

use anyhow::{Context, Result};
use librespot_core::Session;
use librespot_core::dealer::protocol::Message;
use librespot_protocol::connect::{
    Capabilities, Cluster, ClusterUpdate, Device, DeviceInfo, MemberType, PutStateReason,
    PutStateRequest,
};
use librespot_protocol::devices::DeviceType;
use protobuf::{EnumOrUnknown, Message as _, MessageField};
use reqwest::Method;
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};
use tokio::task::JoinHandle;

/// Another device of the account, playing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Playing {
    pub device: String,
    /// The device's name, as Spotify shows it.
    pub name: String,
}

/// Follows the other devices until closed.
pub struct Watch {
    session: Session,
    task: JoinHandle<()>,
}

impl Watch {
    /// Starts following the devices on `session`, which must be connected.
    /// `report` is told which other device plays (None: none) whenever that
    /// changes.
    pub fn start(
        session: &Session,
        report: impl Fn(Option<Playing>) + Send + Sync + 'static,
    ) -> Result<Self> {
        let dealer = session.dealer();
        let mut connections = dealer.listen_for("hm://pusher/v1/connections/", connection_id)?;
        let mut updates = dealer.listen_for(
            "hm://connect-state/v1/cluster",
            Message::from_raw::<ClusterUpdate>,
        )?;
        let session = session.clone();
        let task = tokio::spawn({
            let session = session.clone();
            async move {
                use futures_util::StreamExt;
                if let Err(error) = session.dealer().start().await {
                    log::warn!("unable to watch the other devices: {error}");
                    return;
                }
                let mut last = None;
                let mut tell = |cluster: &Cluster| {
                    let now = playing(&session, cluster);
                    if now != last {
                        last.clone_from(&now);
                        report(now);
                    }
                };
                loop {
                    tokio::select! {
                        Some(id) = connections.next() => {
                            // A new connection, at the start or after the
                            // dealer reconnected: join on it.
                            let Ok(id) = id else { continue };
                            session.set_connection_id(&id);
                            match join(&session).await {
                                Ok(cluster) => {
                                    log::info!("watching the other devices");
                                    tell(&cluster);
                                }
                                Err(error) => log::warn!("unable to join Spotify Connect: {error:#}"),
                            }
                        }
                        Some(update) = updates.next() => match update {
                            Ok(update) => {
                                if let Some(cluster) = update.cluster.as_ref() {
                                    tell(cluster);
                                }
                            }
                            Err(error) => log::warn!("unreadable device update: {error}"),
                        },
                        else => break,
                    }
                }
            }
        });
        Ok(Self { session, task })
    }

    /// Stops following, and leaves Connect.
    pub async fn close(self) {
        self.task.abort();
        self.session.dealer().close().await;
    }
}

/// Asks `device` to pause.
pub async fn pause(session: &Session, device: &str) -> Result<()> {
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
        .await
        .context("Spotify didn't take the pause")?;
    Ok(())
}

fn connection_id(message: Message) -> Result<String, librespot_core::Error> {
    message
        .headers
        .get("Spotify-Connection-Id")
        .cloned()
        .ok_or_else(|| librespot_core::Error::failed_precondition("no connection ID"))
}

/// Joins Connect as a device that watches but never plays, and that other
/// devices can't see or control. Spotify answers with the devices' state.
async fn join(session: &Session) -> Result<Cluster> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let request = PutStateRequest {
        member_type: EnumOrUnknown::new(MemberType::CONNECT_STATE),
        put_state_reason: EnumOrUnknown::new(PutStateReason::NEW_DEVICE),
        is_active: false,
        client_side_timestamp: now,
        device: MessageField::some(Device {
            device_info: MessageField::some(DeviceInfo {
                can_play: false,
                name: "Spotless".into(),
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
    };
    let reply = session
        .spclient()
        .put_connect_state_request(&request)
        .await?;
    Ok(Cluster::parse_from_bytes(&reply)?)
}

/// The other device playing in `cluster`, if one is.
fn playing(session: &Session, cluster: &Cluster) -> Option<Playing> {
    let device = cluster.active_device_id.as_str();
    let state = &cluster.player_state;
    if device.is_empty() || device == session.device_id() || !state.is_playing || state.is_paused {
        return None;
    }
    let name = cluster
        .device
        .get(device)
        .map(|info| info.name.clone())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "another device".into());
    Some(Playing {
        device: device.into(),
        name,
    })
}
