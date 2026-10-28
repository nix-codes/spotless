// Windows: no console window next to Spotless's own. Debug builds keep it,
// for the log.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use anyhow::Result;
use spotless::app::App;
use spotless::paths::Paths;

fn main() -> Result<()> {
    // On X11 the keys go through the input method server XMODIFIERS
    // names (ibus, fcitx). ibus-x11 crashes now and then, and the window
    // then gets no keys, or freezes waiting on it. Xlib's own input method
    // still does compose and dead keys; only typing through an input
    // method (Chinese, Japanese…) is lost, which Spotless can do without.
    // winit reads XMODIFIERS once, building the event loop; it's put back
    // straight after, for the browser and anything else started from here.
    // Both before any thread starts, so none reads the environment then.
    #[cfg(target_os = "linux")]
    let xmodifiers = std::env::var_os("XMODIFIERS");
    #[cfg(target_os = "linux")]
    unsafe {
        std::env::set_var("XMODIFIERS", "@im=local");
    }
    // Raw mouse motion is of no use to Spotless, and eframe repaints for
    // every bit of it while the window has focus and thinks the pointer is
    // inside. On X11 it goes on thinking so once a window manager's resize
    // has taken the pointer, so the window redrew at 60 fps under the
    // resize and shook.
    let event_loop =
        winit::event_loop::EventLoop::<eframe::UserEvent>::with_user_event().build()?;
    event_loop.listen_device_events(winit::event_loop::DeviceEvents::Never);
    #[cfg(target_os = "linux")]
    unsafe {
        match xmodifiers {
            Some(value) => std::env::set_var("XMODIFIERS", value),
            None => std::env::remove_var("XMODIFIERS"),
        }
    }
    env_logger::Builder::from_env(
        env_logger::Env::default()
            .default_filter_or("warn,spotless=info,spotless_core=info,librespot=info"),
    )
    .init();
    // Restarting into an update, the Spotless that started this one is
    // still quitting.
    if std::env::args().any(|arg| arg == spotless::update::RESTARTED) {
        spotless::media::wait_for_exit(std::time::Duration::from_secs(10));
    }
    // One Spotless at a time: they would share the sign-in ports and the
    // playlists file. A second start brings the first one forward.
    if spotless::media::raise_running() {
        log::info!("Spotless is already running; brought it forward");
        return Ok(());
    }
    spotless::update::clean_up();
    let paths = Paths::new()?;

    // The installed .desktop file gives launchers the icon; this one is for
    // the window itself, so a start from a terminal shows it too.
    let icon =
        eframe::icon_data::from_png_bytes(include_bytes!("../packaging/icons/spotless-256.png"))?;
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Spotless")
        .with_app_id("spotless")
        .with_icon(icon)
        .with_inner_size([960.0, 600.0])
        .with_min_inner_size(spotless::ui::MIN_WINDOW);
    // The window opens where it was left, as big.
    if let Some(place) = spotless::store::Settings::load(&paths).window {
        viewport = viewport.with_inner_size(place.size);
        if let Some(position) = place.position {
            viewport = viewport.with_position(position);
        }
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let mut app = eframe::create_native(
        "Spotless",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, paths)))),
        &event_loop,
    );
    event_loop.run_app(&mut app)?;
    // The window has closed and saved. As eframe's own loop does, end here
    // rather than wait on the threads still playing or fetching.
    std::process::exit(0)
}
