//! The tray icon: a StatusNotifierItem on Linux (`sni.rs`), a notification
//! area icon on Windows (`notify_icon.rs`).
//!
//! Clicking it shows or hides the window; its menu plays, pauses, skips,
//! shows the window and quits. While a panel shows it, closing the window
//! hides Spotless here instead of quitting. Its clicks arrive as
//! [`MediaEvent`](crate::media::MediaEvent)s, like the media keys.
//!
//! Elsewhere there is no tray yet, and closing the window quits.

use crate::media::EventHook;

/// What the tray shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrayState {
    pub playing: bool,
    /// "Artist - Title", or empty.
    pub now_playing: String,
    pub window_hidden: bool,
}

#[cfg(target_os = "linux")]
mod sni;
#[cfg(target_os = "linux")]
pub use sni::Tray;

#[cfg(windows)]
mod notify_icon;
#[cfg(windows)]
pub use notify_icon::Tray;

#[cfg(not(any(target_os = "linux", windows)))]
pub use none::Tray;

#[cfg(not(any(target_os = "linux", windows)))]
mod none {
    use super::{EventHook, TrayState};

    pub struct Tray;

    impl Tray {
        pub fn start(_on_event: EventHook) -> Option<Self> {
            None
        }

        pub fn shown(&self) -> bool {
            false
        }

        pub fn update(&mut self, _state: TrayState) {}
    }
}
