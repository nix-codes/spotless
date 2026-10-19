//! Media keys and the desktop's media controls, through the system media
//! transport controls (Windows).
//!
//! Windows sends the keyboard's media keys to the player that holds these
//! controls, and shows what it is playing in the volume flyout and on the
//! lock screen. They belong to the window. If Windows refuses them,
//! Spotless logs why and runs without media keys.
//!
//! The single-instance check lives here too, as MPRIS's does on Linux: a
//! named event that a second start signals, which brings the first one's
//! window forward.

use std::sync::{Arc, OnceLock};

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::Foundation::{TimeSpan, TypedEventHandler, Uri};
use windows::Media::{
    MediaPlaybackStatus, MediaPlaybackType, PlaybackPositionChangeRequestedEventArgs,
    SystemMediaTransportControls, SystemMediaTransportControlsButton,
    SystemMediaTransportControlsButtonPressedEventArgs,
    SystemMediaTransportControlsTimelineProperties,
};
use windows::Storage::Streams::RandomAccessStreamReference;
use windows::Win32::Foundation::WAIT_OBJECT_0;
use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND};
use windows::Win32::System::Threading::{CreateEventW, INFINITE, SetEvent, WaitForSingleObject};
use windows::Win32::System::WinRT::ISystemMediaTransportControlsInterop;
use windows::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow};
use windows::core::{HSTRING, Ref, w};

use super::{EventHook, MediaEvent, NowPlaying};
use crate::playback::engine::Status;

/// The event a second start signals. `Local\`: one Spotless per signed-in
/// Windows user.
const RAISE_EVENT: windows::core::PCWSTR = w!("Local\\Spotless.raise");

/// This Spotless's raise event, once [`raise_running`] made it: kept open
/// for as long as Spotless runs, which is what tells a second start.
static RAISE: OnceLock<usize> = OnceLock::new();

/// Single instance: if Spotless already runs (the raise event exists),
/// signals it to bring its window forward and returns true.
pub fn raise_running() -> bool {
    let event = match unsafe { CreateEventW(None, false, false, RAISE_EVENT) } {
        Ok(event) => event,
        Err(error) => {
            log::warn!("unable to check for a running Spotless: {error}");
            return false;
        }
    };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            // Windows lets the process the user just started give focus away;
            // the running one couldn't take it on its own.
            let _ = AllowSetForegroundWindow(ASFW_ANY);
            let _ = SetEvent(event);
            let _ = CloseHandle(event);
        }
        return true;
    }
    let _ = RAISE.set(event.0 as usize);
    false
}

/// The window's media controls.
pub struct MediaControls {
    controls: Option<SystemMediaTransportControls>,
    shown: NowPlaying,
}

impl MediaControls {
    /// Takes the window's media controls, and listens for a second start.
    pub fn start(cc: &eframe::CreationContext<'_>, on_event: EventHook) -> Self {
        if let Some(&event) = RAISE.get() {
            let on_event = Arc::clone(&on_event);
            let spawned = std::thread::Builder::new()
                .name("spotless-raise".into())
                .spawn(move || {
                    let event = HANDLE(event as *mut _);
                    while unsafe { WaitForSingleObject(event, INFINITE) } == WAIT_OBJECT_0 {
                        on_event(MediaEvent::Raise);
                    }
                });
            if let Err(error) = spawned {
                log::warn!("a second start can't bring this one forward: {error}");
            }
        }
        let controls = match window(cc) {
            Some(window) => open(window, on_event)
                .inspect_err(|error| log::warn!("media keys unavailable: {error}"))
                .ok(),
            None => {
                log::warn!("media keys unavailable: no window handle");
                None
            }
        };
        Self {
            controls,
            shown: NowPlaying::default(),
        }
    }

    /// Tells Windows about `now` if it changed.
    pub fn update(&mut self, now: NowPlaying) {
        let Some(controls) = &self.controls else {
            return;
        };
        if !now.differs(&self.shown) {
            return;
        }
        if let Err(error) = show(controls, &now, &self.shown) {
            log::warn!("media controls: {error}");
        }
        self.shown = now;
    }
}

fn window(cc: &eframe::CreationContext<'_>) -> Option<HWND> {
    match cc.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => Some(HWND(handle.hwnd.get() as *mut _)),
        _ => None,
    }
}

fn open(window: HWND, on_event: EventHook) -> windows::core::Result<SystemMediaTransportControls> {
    let interop = windows::core::factory::<
        SystemMediaTransportControls,
        ISystemMediaTransportControlsInterop,
    >()?;
    let controls: SystemMediaTransportControls = unsafe { interop.GetForWindow(window)? };
    controls.SetIsEnabled(true)?;
    controls.SetIsPlayEnabled(true)?;
    controls.SetIsPauseEnabled(true)?;
    controls.SetIsStopEnabled(true)?;
    controls.SetIsNextEnabled(true)?;
    controls.SetIsPreviousEnabled(true)?;
    controls
        .DisplayUpdater()?
        .SetType(MediaPlaybackType::Music)?;

    let hook = Arc::clone(&on_event);
    controls.ButtonPressed(&TypedEventHandler::new(
        move |_, args: Ref<SystemMediaTransportControlsButtonPressedEventArgs>| {
            let event = match args.ok()?.Button()? {
                SystemMediaTransportControlsButton::Play => MediaEvent::Play,
                SystemMediaTransportControlsButton::Pause => MediaEvent::Pause,
                SystemMediaTransportControlsButton::Stop => MediaEvent::Stop,
                SystemMediaTransportControlsButton::Next => MediaEvent::Next,
                SystemMediaTransportControlsButton::Previous => MediaEvent::Previous,
                _ => return Ok(()),
            };
            hook(event);
            Ok(())
        },
    ))?;
    controls.PlaybackPositionChangeRequested(&TypedEventHandler::new(
        move |_, args: Ref<PlaybackPositionChangeRequestedEventArgs>| {
            // Windows counts in 100 ns ticks; MediaEvent in microseconds.
            let position = args.ok()?.RequestedPlaybackPosition()?;
            on_event(MediaEvent::SeekTo(position.Duration / 10));
            Ok(())
        },
    ))?;
    Ok(controls)
}

fn show(
    controls: &SystemMediaTransportControls,
    now: &NowPlaying,
    shown: &NowPlaying,
) -> windows::core::Result<()> {
    controls.SetPlaybackStatus(match now.status {
        Status::Playing | Status::Loading => MediaPlaybackStatus::Playing,
        Status::Paused => MediaPlaybackStatus::Paused,
        Status::Stopped => MediaPlaybackStatus::Stopped,
    })?;

    if now.uri != shown.uri || now.cover_url != shown.cover_url {
        let display = controls.DisplayUpdater()?;
        display.ClearAll()?;
        display.SetType(MediaPlaybackType::Music)?;
        if !now.uri.is_empty() {
            let music = display.MusicProperties()?;
            music.SetTitle(&HSTRING::from(&now.title))?;
            music.SetArtist(&HSTRING::from(now.artists.join(", ")))?;
            music.SetAlbumTitle(&HSTRING::from(&now.album))?;
            if let Some(url) = &now.cover_url {
                let uri = Uri::CreateUri(&HSTRING::from(url))?;
                display.SetThumbnail(&RandomAccessStreamReference::CreateFromUri(&uri)?)?;
            }
        }
        display.Update()?;
    }

    let ticks = |us: i64| TimeSpan { Duration: us * 10 };
    let length = i64::from(now.length_ms) * 1000;
    let timeline = SystemMediaTransportControlsTimelineProperties::new()?;
    timeline.SetStartTime(ticks(0))?;
    timeline.SetMinSeekTime(ticks(0))?;
    timeline.SetEndTime(ticks(length))?;
    timeline.SetMaxSeekTime(ticks(length))?;
    timeline.SetPosition(ticks(now.position_us()))?;
    controls.UpdateTimelineProperties(&timeline)
}
