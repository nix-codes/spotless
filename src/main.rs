use anyhow::Result;
use spotless::app::App;
use spotless::paths::Paths;

fn main() -> Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,spotless=info,librespot=info"),
    )
    .init();
    let paths = Paths::new()?;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Spotless")
            .with_app_id("spotless")
            .with_inner_size([960.0, 600.0])
            .with_min_inner_size([560.0, 360.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Spotless",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, paths)))),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}
