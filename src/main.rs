// Windows: no console window next to Spotless's own. Debug builds keep it,
// for the log.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use anyhow::Result;
use spotless::app::App;
use spotless::paths::Paths;

fn main() -> Result<()> {
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
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Spotless")
            .with_app_id("spotless")
            .with_icon(icon)
            .with_inner_size([960.0, 600.0])
            .with_min_inner_size(spotless::ui::MIN_WINDOW),
        ..Default::default()
    };
    eframe::run_native(
        "Spotless",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, paths)))),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}
