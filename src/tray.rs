//! The tray icon (Linux: a StatusNotifierItem on the session bus).
//!
//! Clicking it shows or hides the window; its menu plays, pauses, skips,
//! shows the window and quits. While a panel shows it, closing the window
//! hides Spotless here instead of quitting. Its clicks arrive as
//! [`MediaEvent`]s, like the media keys.
//!
//! Elsewhere there is no tray yet, and closing the window quits.

use crate::media::{EventHook, MediaEvent};

/// What the tray shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrayState {
    pub playing: bool,
    /// "Artist - Title", or empty.
    pub now_playing: String,
    pub window_hidden: bool,
}

pub use imp::Tray;

#[cfg(target_os = "linux")]
mod imp {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use ksni::blocking::{Handle, TrayMethods};
    use ksni::menu::{MenuItem, StandardItem};

    use super::{EventHook, MediaEvent, TrayState};

    pub struct Tray {
        handle: Handle<Item>,
        /// A panel shows the icon.
        shown: Arc<AtomicBool>,
        state: TrayState,
    }

    impl Tray {
        /// Puts the icon in the tray. None without a tray to put it in.
        pub fn start(on_event: EventHook) -> Option<Self> {
            let shown = Arc::new(AtomicBool::new(true));
            let item = Item {
                on_event,
                shown: Arc::clone(&shown),
                state: TrayState::default(),
                icons: icons(),
            };
            match item.spawn() {
                Ok(handle) => Some(Self {
                    handle,
                    shown,
                    state: TrayState::default(),
                }),
                Err(error) => {
                    log::warn!("no tray icon: {error}");
                    None
                }
            }
        }

        /// A panel shows the icon, so a hidden window can be found again.
        pub fn shown(&self) -> bool {
            self.shown.load(Ordering::SeqCst) && !self.handle.is_closed()
        }

        pub fn update(&mut self, state: TrayState) {
            if state == self.state {
                return;
            }
            self.state = state.clone();
            self.handle.update(|item| item.state = state);
        }
    }

    impl Drop for Tray {
        fn drop(&mut self) {
            let _ = self.handle.shutdown();
        }
    }

    struct Item {
        on_event: EventHook,
        shown: Arc<AtomicBool>,
        state: TrayState,
        icons: Vec<ksni::Icon>,
    }

    impl ksni::Tray for Item {
        fn id(&self) -> String {
            "spotless".into()
        }

        fn title(&self) -> String {
            "Spotless".into()
        }

        fn category(&self) -> ksni::Category {
            ksni::Category::ApplicationStatus
        }

        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            self.icons.clone()
        }

        fn tool_tip(&self) -> ksni::ToolTip {
            ksni::ToolTip {
                title: "Spotless".into(),
                description: self.state.now_playing.clone(),
                ..Default::default()
            }
        }

        fn activate(&mut self, _x: i32, _y: i32) {
            (self.on_event)(MediaEvent::ToggleWindow);
        }

        fn menu(&self) -> Vec<MenuItem<Self>> {
            let item = |label: &str, event: MediaEvent| {
                StandardItem {
                    label: label.into(),
                    activate: Box::new(move |item: &mut Self| (item.on_event)(event)),
                    ..Default::default()
                }
                .into()
            };
            let window = if self.state.window_hidden {
                ("Show Spotless", MediaEvent::Raise)
            } else {
                ("Hide Spotless", MediaEvent::ToggleWindow)
            };
            vec![
                item(
                    if self.state.playing { "Pause" } else { "Play" },
                    MediaEvent::PlayPause,
                ),
                item("Previous", MediaEvent::Previous),
                item("Next", MediaEvent::Next),
                MenuItem::Separator,
                item(window.0, window.1),
                MenuItem::Separator,
                item("Quit", MediaEvent::Quit),
            ]
        }

        fn watcher_online(&self) {
            self.shown.store(true, Ordering::SeqCst);
        }

        fn watcher_offline(&self, _reason: ksni::OfflineReason) -> bool {
            self.shown.store(false, Ordering::SeqCst);
            // Keep serving: the panel may come back.
            true
        }
    }

    /// The app's icon at the sizes panels ask for, as ARGB.
    fn icons() -> Vec<ksni::Icon> {
        let Ok(image) =
            image::load_from_memory(include_bytes!("../packaging/icons/spotless-256.png"))
        else {
            return Vec::new();
        };
        [22, 32, 48, 64, 128]
            .into_iter()
            .map(|size| {
                let image = image::imageops::resize(
                    &image.to_rgba8(),
                    size,
                    size,
                    image::imageops::FilterType::Lanczos3,
                );
                let data = image
                    .pixels()
                    .flat_map(|p| [p[3], p[0], p[1], p[2]])
                    .collect();
                ksni::Icon {
                    width: size as i32,
                    height: size as i32,
                    data,
                }
            })
            .collect()
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
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
