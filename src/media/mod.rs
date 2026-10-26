//! Media keys and the desktop's media controls, plus the single-instance
//! check that rides on them: MPRIS on Linux (`mpris.rs`), the system media
//! transport controls on Windows (`smtc.rs`). Elsewhere there are none yet.

use std::sync::Arc;
use std::time::Instant;

use crate::playback::engine::{PlayerSnapshot, Status};

#[cfg(target_os = "linux")]
mod mpris;
#[cfg(target_os = "linux")]
pub use mpris::{MediaControls, raise_running, wait_for_exit};

#[cfg(windows)]
mod smtc;
#[cfg(windows)]
pub use smtc::{MediaControls, raise_running, wait_for_exit};

#[cfg(not(any(target_os = "linux", windows)))]
pub use none::{MediaControls, raise_running, wait_for_exit};

/// What the desktop asks of the player.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MediaEvent {
    Play,
    Pause,
    PlayPause,
    Stop,
    Next,
    Previous,
    /// Seek by this many microseconds, back if negative.
    SeekBy(i64),
    /// Seek to this many microseconds.
    SeekTo(i64),
    /// 0.0–1.0.
    Volume(f64),
    Raise,
    Quit,
    /// The tray icon was clicked: show the window, or hide it.
    ToggleWindow,
}

pub type EventHook = Arc<dyn Fn(MediaEvent) + Send + Sync>;

/// What the desktop shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NowPlaying {
    pub status: Status,
    pub uri: String,
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    pub cover_url: Option<String>,
    pub length_ms: u32,
    /// The position at `at`; it moves on from there while playing.
    pub position_ms: u32,
    pub at: Option<Instant>,
    /// 0.0–1.0.
    pub volume: f64,
}

impl NowPlaying {
    pub fn from_player(player: &PlayerSnapshot, volume: u8) -> Self {
        let track = player.track.as_ref();
        Self {
            status: player.status,
            uri: track.map(|t| t.uri.clone()).unwrap_or_default(),
            title: track.map(|t| t.title.clone()).unwrap_or_default(),
            artists: track
                .map(|t| t.artists.split(", ").map(String::from).collect())
                .unwrap_or_default(),
            album: track.map(|t| t.album.clone()).unwrap_or_default(),
            cover_url: track.and_then(|t| t.cover_url.clone()),
            length_ms: track.map_or(0, |t| t.duration_ms),
            position_ms: player.position_now(),
            at: Some(Instant::now()),
            volume: f64::from(volume) / 100.0,
        }
    }

    fn position_us(&self) -> i64 {
        let mut ms = u64::from(self.position_ms);
        if self.status == Status::Playing
            && let Some(at) = self.at
        {
            ms += at.elapsed().as_millis() as u64;
        }
        (ms.min(u64::from(self.length_ms)) * 1000) as i64
    }

    /// Whether the desktop needs telling: a new song, status or volume, or
    /// a position that jumped (a seek) rather than moved on.
    pub fn differs(&self, shown: &Self) -> bool {
        let moved = (self.position_us() - shown.position_us()).abs() > 1_500_000;
        self.status != shown.status
            || self.uri != shown.uri
            || self.cover_url != shown.cover_url
            || (self.volume - shown.volume).abs() > 0.005
            || moved
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod none {
    use super::{EventHook, NowPlaying};

    pub fn raise_running() -> bool {
        false
    }

    pub fn wait_for_exit(_limit: std::time::Duration) {}

    pub struct MediaControls;

    impl MediaControls {
        pub fn start(_cc: &eframe::CreationContext<'_>, _on_event: EventHook) -> Self {
            Self
        }

        pub fn update(&mut self, _now: NowPlaying) {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_jumps_in_position_need_telling() {
        let playing = NowPlaying {
            status: Status::Playing,
            uri: "spotify:track:a".into(),
            length_ms: 300_000,
            position_ms: 10_000,
            at: Some(Instant::now()),
            ..NowPlaying::default()
        };
        let later = NowPlaying {
            position_ms: 10_500,
            ..playing.clone()
        };
        assert!(!later.differs(&playing));
        let seeked = NowPlaying {
            position_ms: 90_000,
            ..playing.clone()
        };
        assert!(seeked.differs(&playing));
        let paused = NowPlaying {
            status: Status::Paused,
            ..playing.clone()
        };
        assert!(paused.differs(&playing));
    }
}
