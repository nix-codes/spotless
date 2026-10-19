//! Media keys and the desktop's media controls, through MPRIS (Linux).
//!
//! GNOME, KDE and others send the keyboard's media keys to the player that
//! registers `org.mpris.MediaPlayer2.*` on the session bus, and show what it
//! is playing. This serves the two MPRIS interfaces on zbus (already in the
//! build for accessibility) on a thread of its own. Without a session bus
//! it logs why and Spotless runs without media keys.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

use super::{EventHook, MediaEvent, NowPlaying};
use crate::playback::engine::Status;

const BUS_NAME: &str = "org.mpris.MediaPlayer2.spotless";
const PATH: &str = "/org/mpris/MediaPlayer2";

/// Single instance: if Spotless already runs (its MPRIS name is taken),
/// asks it to bring its window forward and returns true. Without a
/// session bus, says false and lets this one run.
pub fn raise_running() -> bool {
    let attempt = || -> zbus::Result<bool> {
        let connection = zbus::blocking::Connection::session()?;
        let bus = zbus::blocking::fdo::DBusProxy::new(&connection)?;
        if !bus.name_has_owner(BUS_NAME.try_into()?)? {
            return Ok(false);
        }
        connection.call_method(
            Some(BUS_NAME),
            PATH,
            Some("org.mpris.MediaPlayer2"),
            "Raise",
            &(),
        )?;
        Ok(true)
    };
    attempt().unwrap_or_else(|error| {
        log::warn!("unable to check for a running Spotless: {error}");
        false
    })
}

/// The running MPRIS service.
pub struct MediaControls {
    updates: UnboundedSender<NowPlaying>,
    shown: NowPlaying,
}

impl MediaControls {
    /// Registers on the session bus in the background.
    pub fn start(_cc: &eframe::CreationContext<'_>, on_event: EventHook) -> Self {
        let (updates, receiver) = unbounded_channel();
        let spawned = std::thread::Builder::new()
            .name("spotless-mpris".into())
            .spawn(move || {
                if let Err(error) = zbus::block_on(serve(on_event, receiver)) {
                    log::warn!("media keys unavailable: {error}");
                }
            });
        if let Err(error) = spawned {
            log::warn!("media keys unavailable: {error}");
        }
        Self {
            updates,
            shown: NowPlaying::default(),
        }
    }

    /// Tells the desktop about `now` if it changed.
    pub fn update(&mut self, now: NowPlaying) {
        if now.differs(&self.shown) {
            self.shown = now.clone();
            let _ = self.updates.send(now);
        }
    }
}

async fn serve(
    on_event: EventHook,
    mut updates: UnboundedReceiver<NowPlaying>,
) -> zbus::Result<()> {
    let connection = zbus::connection::Builder::session()?
        .name(BUS_NAME)?
        .serve_at(
            PATH,
            Root {
                on_event: Arc::clone(&on_event),
            },
        )?
        .serve_at(
            PATH,
            Player {
                on_event,
                now: NowPlaying::default(),
            },
        )?
        .build()
        .await?;
    log::info!("media keys: registered {BUS_NAME}");
    let player = connection
        .object_server()
        .interface::<_, Player>(PATH)
        .await?;
    while let Some(now) = updates.recv().await {
        let (song, status, volume, seeked) = {
            let mut current = player.get_mut().await;
            let song = now.uri != current.now.uri || now.cover_url != current.now.cover_url;
            let status = now.status != current.now.status;
            let volume = (now.volume - current.now.volume).abs() > 0.005;
            let seeked = !song && !status && !volume;
            current.now = now;
            (song, status, volume, seeked)
        };
        let emitter = player.signal_emitter();
        let current = player.get().await;
        if song {
            current.metadata_changed(emitter).await?;
        }
        if status {
            current.playback_status_changed(emitter).await?;
        }
        if volume {
            current.volume_changed(emitter).await?;
        }
        if seeked || song {
            Player::seeked(emitter, current.now.position_us()).await?;
        }
    }
    Ok(())
}

/// `org.mpris.MediaPlayer2`: the application.
struct Root {
    on_event: EventHook,
}

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {
        (self.on_event)(MediaEvent::Raise);
    }

    fn quit(&self) {
        (self.on_event)(MediaEvent::Quit);
    }

    #[zbus(property)]
    fn can_raise(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_quit(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn identity(&self) -> &str {
        "Spotless"
    }

    #[zbus(property)]
    fn desktop_entry(&self) -> &str {
        "spotless"
    }

    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        Vec::new()
    }

    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        Vec::new()
    }
}

/// `org.mpris.MediaPlayer2.Player`: playback.
struct Player {
    on_event: EventHook,
    now: NowPlaying,
}

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl Player {
    fn play(&self) {
        (self.on_event)(MediaEvent::Play);
    }

    fn pause(&self) {
        (self.on_event)(MediaEvent::Pause);
    }

    fn play_pause(&self) {
        (self.on_event)(MediaEvent::PlayPause);
    }

    fn stop(&self) {
        (self.on_event)(MediaEvent::Stop);
    }

    fn next(&self) {
        (self.on_event)(MediaEvent::Next);
    }

    fn previous(&self) {
        (self.on_event)(MediaEvent::Previous);
    }

    fn seek(&self, offset: i64) {
        (self.on_event)(MediaEvent::SeekBy(offset));
    }

    fn set_position(&self, _track: ObjectPath<'_>, position: i64) {
        (self.on_event)(MediaEvent::SeekTo(position));
    }

    fn open_uri(&self, _uri: &str) {}

    #[zbus(signal)]
    async fn seeked(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        position: i64,
    ) -> zbus::Result<()>;

    #[zbus(property)]
    fn playback_status(&self) -> &str {
        match self.now.status {
            Status::Playing | Status::Loading => "Playing",
            Status::Paused => "Paused",
            Status::Stopped => "Stopped",
        }
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        let mut map = HashMap::new();
        let mut put = |key: &str, value: Value<'_>| {
            if let Ok(value) = OwnedValue::try_from(value) {
                map.insert(key.to_string(), value);
            }
        };
        put("mpris:trackid", Value::from(track_path(&self.now.uri)));
        if self.now.uri.is_empty() {
            return map;
        }
        put(
            "mpris:length",
            Value::from(i64::from(self.now.length_ms) * 1000),
        );
        put("xesam:title", Value::from(self.now.title.as_str()));
        put("xesam:artist", Value::from(self.now.artists.clone()));
        put("xesam:album", Value::from(self.now.album.as_str()));
        if let Some(url) = &self.now.cover_url {
            put("mpris:artUrl", Value::from(url.as_str()));
        }
        put("xesam:url", Value::from(spotify_link(&self.now.uri)));
        map
    }

    #[zbus(property)]
    fn position(&self) -> i64 {
        self.now.position_us()
    }

    #[zbus(property)]
    fn volume(&self) -> f64 {
        self.now.volume
    }

    #[zbus(property)]
    fn set_volume(&mut self, volume: f64) {
        (self.on_event)(MediaEvent::Volume(volume.clamp(0.0, 1.0)));
    }

    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_seek(&self) -> bool {
        !self.now.uri.is_empty()
    }

    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
}

/// An MPRIS track ID: an object path made from the Spotify URI's ID.
fn track_path(uri: &str) -> ObjectPath<'static> {
    let id: String = uri
        .rsplit(':')
        .next()
        .unwrap_or_default()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    let path = if id.is_empty() {
        "/org/mpris/MediaPlayer2/TrackList/NoTrack".to_string()
    } else {
        format!("/org/spotless/track/{id}")
    };
    ObjectPath::try_from(path).unwrap_or_else(|_| ObjectPath::from_static_str_unchecked("/"))
}

/// `spotify:track:ID` as an open.spotify.com link.
fn spotify_link(uri: &str) -> String {
    let mut parts = uri.split(':').skip(1);
    match (parts.next(), parts.next()) {
        (Some(kind), Some(id)) => format!("https://open.spotify.com/{kind}/{id}"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_ids_are_object_paths() {
        assert_eq!(
            track_path("spotify:track:4uLU6hMCjMI75M1A2tKUQC").as_str(),
            "/org/spotless/track/4uLU6hMCjMI75M1A2tKUQC"
        );
        assert_eq!(
            track_path("").as_str(),
            "/org/mpris/MediaPlayer2/TrackList/NoTrack"
        );
        assert_eq!(
            spotify_link("spotify:track:abc"),
            "https://open.spotify.com/track/abc"
        );
    }
}
