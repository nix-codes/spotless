//! The settings only the desktop has: the window, the zoom, the skin, the
//! tray and updates. They are kept in the core's settings file, beside
//! the settings both apps share, under keys of their own
//! ([`spotless_core::settings::Settings::extra`]), and saved with them.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const ZOOM_RANGE: std::ops::RangeInclusive<f32> = 0.75..=2.5;

#[derive(Clone, Debug, PartialEq)]
pub struct DesktopSettings {
    /// The interface's zoom, 1 at normal size.
    pub zoom: f32,
    /// The tray icon, which closing the window hides Spotless in.
    pub tray: bool,
    /// The desktop app looks for a newer release of itself once a day.
    pub updates: bool,
    /// A newer release found is announced in a bar under the player.
    pub update_notices: bool,
    /// The release the user asked not to be told of again, by its version.
    pub skipped_update: Option<String>,
    /// The skin the colours come from, by its name in the skins folder;
    /// None is Classic.
    pub skin: Option<String>,
    /// The window, as it was left.
    pub window: Option<WindowPlace>,
}

/// Where the window was and how big, to open it there again. In points
/// at a zoom of 1.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowPlace {
    /// The window's outer top left corner; None where the desktop doesn't
    /// tell (Wayland).
    #[serde(default)]
    pub position: Option<[f32; 2]>,
    /// Its content's size, as tall as it is with the playlists showing.
    pub size: [f32; 2],
    #[serde(default)]
    pub maximized: bool,
}

impl DesktopSettings {
    /// Reads them from the settings file's keys; what's missing or can't
    /// be read is as at first.
    pub fn read(raw: &Map<String, Value>) -> Self {
        Self {
            zoom: raw.get("zoom").and_then(Value::as_f64).map_or(1.0, |z| {
                (z as f32).clamp(*ZOOM_RANGE.start(), *ZOOM_RANGE.end())
            }),
            tray: raw.get("tray").and_then(Value::as_bool).unwrap_or(true),
            updates: raw.get("updates").and_then(Value::as_bool).unwrap_or(true),
            update_notices: raw
                .get("update_notices")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            skipped_update: raw
                .get("skipped_update")
                .and_then(Value::as_str)
                .map(str::to_string),
            skin: raw.get("skin").and_then(Value::as_str).map(String::from),
            window: raw
                .get("window")
                .and_then(|value| WindowPlace::deserialize(value).ok())
                .filter(|place| {
                    place
                        .size
                        .iter()
                        .all(|side| side.is_finite() && *side > 0.0)
                }),
        }
    }

    /// Writes them into the settings file's keys.
    pub fn write(&self, raw: &mut Map<String, Value>) {
        raw.insert("zoom".into(), self.zoom.into());
        raw.insert("tray".into(), self.tray.into());
        raw.insert("updates".into(), self.updates.into());
        raw.insert("update_notices".into(), self.update_notices.into());
        match &self.skipped_update {
            Some(version) => raw.insert("skipped_update".into(), version.clone().into()),
            None => raw.remove("skipped_update"),
        };
        match &self.skin {
            Some(skin) => raw.insert("skin".into(), skin.clone().into()),
            None => raw.remove("skin"),
        };
        match self.window.map(serde_json::to_value) {
            Some(Ok(window)) => raw.insert("window".into(), window),
            _ => raw.remove("window"),
        };
    }
}

#[cfg(test)]
mod tests {
    use spotless_core::Paths;
    use spotless_core::settings::Settings;

    use super::*;

    fn temporary_paths(name: &str) -> Paths {
        let root =
            std::env::temp_dir().join(format!("spotless-desktop-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        Paths::from_dirs(root.join("config"), root.join("cache")).unwrap()
    }

    /// Written into the core's settings, saved, and read back.
    fn saved(paths: &Paths, desktop: &DesktopSettings) -> DesktopSettings {
        let mut settings = Settings::load(paths);
        desktop.write(settings.extra_mut());
        settings.save(paths).unwrap();
        DesktopSettings::read(Settings::load(paths).extra())
    }

    #[test]
    fn they_default_as_at_first() {
        let desktop = DesktopSettings::read(&Map::new());
        assert_eq!(desktop.zoom, 1.0);
        assert!(desktop.tray && desktop.updates && desktop.update_notices);
        assert_eq!(desktop.skipped_update, None);
        assert_eq!(desktop.skin, None);
        assert_eq!(desktop.window, None);
    }

    #[test]
    fn they_are_saved_with_the_cores_settings() {
        let paths = temporary_paths("saved");
        let desktop = DesktopSettings {
            zoom: 1.5,
            tray: false,
            updates: false,
            update_notices: false,
            skipped_update: Some("1.3.0".into()),
            skin: Some("built-in:winamp-classic".into()),
            window: None,
        };
        assert_eq!(saved(&paths, &desktop), desktop);
        // The core's own still read as they were.
        assert_eq!(Settings::load(&paths).volume, 70);
    }

    #[test]
    fn a_skin_or_skipped_release_taken_away_is_gone_from_the_file() {
        let paths = temporary_paths("removed");
        let mut desktop = DesktopSettings::read(&Map::new());
        desktop.skin = Some("built-in:winamp-classic".into());
        desktop.skipped_update = Some("1.3.0".into());
        let mut desktop = saved(&paths, &desktop);
        desktop.skin = None;
        desktop.skipped_update = None;
        saved(&paths, &desktop);
        let raw = Settings::load(&paths).extra().clone();
        assert!(!raw.contains_key("skin"));
        assert!(!raw.contains_key("skipped_update"));
    }

    #[test]
    fn the_zoom_stays_in_range() {
        let raw: Map<String, Value> = serde_json::from_str(r#"{"zoom":9}"#).unwrap();
        assert_eq!(DesktopSettings::read(&raw).zoom, *ZOOM_RANGE.end());
    }

    #[test]
    fn the_window_place_is_kept_and_a_broken_one_dropped() {
        let paths = temporary_paths("window");
        let mut desktop = DesktopSettings::read(&Map::new());
        let place = WindowPlace {
            position: Some([120.0, 80.0]),
            size: [900.0, 640.0],
            maximized: false,
        };
        desktop.window = Some(place);
        assert_eq!(saved(&paths, &desktop).window, Some(place));
        let raw: Map<String, Value> =
            serde_json::from_str(r#"{"window":{"size":[0,600]}}"#).unwrap();
        assert_eq!(DesktopSettings::read(&raw).window, None);
    }
}
