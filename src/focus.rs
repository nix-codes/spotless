//! Bringing the window to the front, on opening and when it's raised.
//!
//! On X11, winit asks for the focus as an application, without a time, and
//! window managers that prevent focus stealing (Cinnamon's muffin, GNOME's
//! mutter) turn that down: Spotless opened behind the window it was
//! started from. A pager's or taskbar's request is granted, so the window
//! asks as one. On Wayland and Windows, egui's own request is all there is.

use std::time::Duration;

/// How long a request goes on being made, while the window isn't yet shown
/// or the window manager hasn't answered.
const TRYING: Duration = Duration::from_secs(1);
/// How often it's made meanwhile.
const EVERY: Duration = Duration::from_millis(100);

pub struct Focus {
    #[cfg(target_os = "linux")]
    x11: Option<x11::Activate>,
    /// Until when (egui's time) the window asks for the focus.
    until: Option<f64>,
    /// When it last asked.
    asked: f64,
}

impl Focus {
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            #[cfg(target_os = "linux")]
            x11: x11::Activate::new(cc),
            until: None,
            asked: f64::NEG_INFINITY,
        }
    }

    /// Asks for the focus from now on, until the window has it.
    pub fn want(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        self.until = Some(now + TRYING.as_secs_f64());
        self.asked = f64::NEG_INFINITY;
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        ctx.request_repaint();
    }

    /// Asks again, each frame, while it's wanted.
    pub fn tick(&mut self, ctx: &egui::Context) {
        let Some(until) = self.until else {
            return;
        };
        let (now, focused) = ctx.input(|i| (i.time, i.viewport().focused));
        if focused == Some(true) || now > until {
            self.until = None;
            return;
        }
        if now - self.asked >= EVERY.as_secs_f64() {
            self.asked = now;
            #[cfg(target_os = "linux")]
            if let Some(x11) = &self.x11 {
                x11.ask();
            }
        }
        ctx.request_repaint_after(EVERY);
    }
}

#[cfg(target_os = "linux")]
mod x11 {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ClientMessageEvent, ConnectionExt, EventMask};
    use x11rb::rust_connection::RustConnection;

    /// `_NET_ACTIVE_WINDOW`'s source: a pager.
    const FROM_PAGER: u32 = 2;

    /// The window's own connection to the X server, to ask for it.
    pub struct Activate {
        connection: RustConnection,
        root: u32,
        window: u32,
        active: u32,
    }

    impl Activate {
        /// None on Wayland, or without an X server to ask.
        pub fn new(cc: &eframe::CreationContext<'_>) -> Option<Self> {
            let window = match cc.window_handle().ok()?.as_raw() {
                RawWindowHandle::Xlib(handle) => handle.window as u32,
                RawWindowHandle::Xcb(handle) => handle.window.get(),
                _ => return None,
            };
            let (connection, screen) = x11rb::connect(None)
                .inspect_err(|error| log::warn!("focusing the window: {error}"))
                .ok()?;
            let root = connection.setup().roots.get(screen)?.root;
            let active = connection
                .intern_atom(false, b"_NET_ACTIVE_WINDOW")
                .ok()?
                .reply()
                .ok()?
                .atom;
            Some(Self {
                connection,
                root,
                window,
                active,
            })
        }

        pub fn ask(&self) {
            let message = ClientMessageEvent::new(
                32,
                self.window,
                self.active,
                [FROM_PAGER, x11rb::CURRENT_TIME, 0, 0, 0],
            );
            let mask = EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY;
            let sent = self
                .connection
                .send_event(false, self.root, mask, message)
                .map(|_| ())
                .and_then(|()| self.connection.flush());
            if let Err(error) = sent {
                log::warn!("focusing the window: {error}");
            }
        }
    }
}
