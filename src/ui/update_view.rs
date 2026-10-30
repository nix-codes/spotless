//! The window that says a newer Spotless is out, with its notes: UPDATE,
//! LATER (until Spotless next starts), and SKIP THIS VERSION. Once UPDATE
//! is pressed it follows the download, then offers RESTART. Settings ›
//! About turns it off for good.

use egui::{Id, Label, RichText, ScrollArea, Sense, Ui, UiBuilder, vec2};

use super::dialog::{self, Answer};
use super::settings_view::open_in_browser;
use super::theme::Palette;
use super::{View, scrollbar};
use crate::action::{Actions, Desktop};
use crate::update::{Release, Status};

/// The most height the notes take before they scroll.
const NOTES: f32 = 220.0;
const WIDTH: f32 = 380.0;

/// What the window keeps between frames.
#[derive(Default)]
pub struct Popup {
    /// LATER was pressed: hidden until Spotless next starts.
    pub hidden: bool,
}

/// The release the window is about, while it shows.
pub fn shown<'a>(view: &View<'a>, popup: &Popup) -> Option<&'a Release> {
    let skipped = view.settings.skipped_update.as_deref();
    release(view.update).filter(|release| {
        view.settings.update_notices
            && !popup.hidden
            && skipped != Some(release.version.to_string().as_str())
    })
}

/// What a button of the window does.
enum Choice {
    Act(&'static str, Desktop),
    Download,
    Later,
    Skip,
}

pub fn draw(ui: &Ui, view: &View, popup: &mut Popup, actions: &mut Actions) {
    let Some(release) = shown(view, popup) else {
        return;
    };
    let palette = view.palette;
    let version = release.version;
    let (title, text) = match view.update {
        Status::Installing(_) => (format!("Downloading Spotless {version}…"), None),
        Status::Installed(_) => (
            format!("Spotless {version} is installed"),
            Some(
                "It starts the next time Spotless does. RESTART starts it now, stopping the \
                 song playing.",
            ),
        ),
        Status::Failed { error, .. } => (
            format!("Couldn't update to Spotless {version}"),
            Some(error.as_str()),
        ),
        _ => (format!("Spotless {version} is out"), None),
    };
    let mut choices = Vec::new();
    if !release.installable() {
        choices.push(Choice::Download);
    } else if let Some((label, action)) = update_button(view.update) {
        choices.push(Choice::Act(label, action));
    }
    choices.push(Choice::Later);
    if matches!(view.update, Status::Available(_) | Status::Failed { .. }) {
        choices.push(Choice::Skip);
    }
    let buttons: Vec<(&str, &str)> = choices
        .iter()
        .map(|choice| match choice {
            Choice::Act(label, _) => (*label, ""),
            Choice::Download => ("DOWNLOAD", "Opens its page on GitHub"),
            Choice::Later => ("LATER", "Asks again when Spotless next starts"),
            Choice::Skip => ("SKIP THIS VERSION", "Not asked again about this one"),
        })
        .collect();
    let answer = dialog::show(ui, &palette, Id::new("update"), &buttons, |ui| {
        ui.set_width(WIDTH);
        ui.label(RichText::new(title).size(15.0).color(palette.current));
        if let Some(text) = text {
            ui.add_space(4.0);
            ui.add(Label::new(RichText::new(text).color(palette.chrome)).wrap());
        }
        if !release.notes.trim().is_empty() {
            ui.add_space(8.0);
            // As tall as the notes, measured the frame before, up to NOTES.
            let id = Id::new("update-notes");
            let measured = id.with("height");
            let height = ui.data(|d| d.get_temp::<f32>(measured)).unwrap_or(NOTES);
            let (rect, _) = ui.allocate_exact_size(vec2(WIDTH, height), Sense::hover());
            let tall = ui
                .scope_builder(UiBuilder::new().max_rect(rect), |ui| {
                    let area = ScrollArea::vertical().id_salt(id);
                    scrollbar::show(ui, view, id, area, |area, ui| {
                        area.show(ui, |ui| {
                            notes(ui, &palette, &release.notes);
                            ui.min_rect().height()
                        })
                    })
                })
                .inner
                .min(NOTES);
            if tall != height {
                ui.data_mut(|d| d.insert_temp(measured, tall));
                ui.ctx().request_repaint();
            }
        }
        ui.add_space(4.0);
        let link = RichText::new("Release page on GitHub").color(palette.accent);
        if ui.link(link).clicked() {
            open_in_browser(&release.page);
        }
    });
    match answer {
        Answer::Waiting => {}
        Answer::Dismissed => popup.hidden = true,
        Answer::Chose(index) => match &choices[index] {
            // The window stays, to follow the download and offer RESTART.
            Choice::Act(_, action) => actions.push(action.clone()),
            Choice::Download => {
                open_in_browser(&release.page);
                popup.hidden = true;
            }
            Choice::Later => popup.hidden = true,
            Choice::Skip => actions.push(Desktop::SkipUpdate(version.to_string())),
        },
    }
}

/// A release's notes: Markdown's headings and lists, as plain text.
fn notes(ui: &mut Ui, palette: &Palette, markdown: &str) {
    ui.spacing_mut().item_spacing.y = 3.0;
    for line in markdown.lines() {
        let line = line.trim().replace(['`'], "").replace("**", "");
        if line.is_empty() {
            continue;
        }
        if line.starts_with('#') {
            ui.add_space(4.0);
            let heading = line.trim_start_matches('#').trim().to_uppercase();
            ui.label(
                RichText::new(heading)
                    .monospace()
                    .size(10.0)
                    .color(palette.dim),
            );
        } else if let Some(item) = line.strip_prefix("- ").or(line.strip_prefix("* ")) {
            ui.horizontal_top(|ui| {
                ui.label(RichText::new("·").color(palette.dim));
                ui.add(Label::new(RichText::new(item).color(palette.list_fg)).wrap());
            });
        } else {
            ui.add(Label::new(RichText::new(line).color(palette.list_fg)).wrap());
        }
    }
}

/// The status bar's word on an update, whose button shows it in Settings.
/// None when there is no update to tell of, or no telling is wanted.
pub fn status_label(view: &View) -> Option<String> {
    if !view.settings.update_notices {
        return None;
    }
    match view.update {
        Status::Available(release)
        | Status::Failed {
            release: Some(release),
            ..
        } => Some(format!("UPDATE · {}", release.version)),
        Status::Installing(_) => Some("UPDATING…".into()),
        Status::Installed(release) => Some(format!("RESTART · {}", release.version)),
        _ => None,
    }
}

/// The button that takes updating on from where it stands, and what it
/// does.
pub fn update_button(status: &Status) -> Option<(&'static str, Desktop)> {
    match status {
        Status::Unknown | Status::Current | Status::Failed { release: None, .. } => {
            Some(("CHECK NOW", Desktop::CheckForUpdate))
        }
        Status::Available(release) if release.installable() => Some(("UPDATE", Desktop::Update)),
        Status::Available(_) => None,
        Status::Failed {
            release: Some(_), ..
        } => Some(("TRY AGAIN", Desktop::Update)),
        Status::Installed(_) => Some(("RESTART", Desktop::Restart)),
        Status::Checking | Status::Installing(_) => None,
    }
}

/// The release found, while there is one to install or restart into.
pub fn release(status: &Status) -> Option<&Release> {
    match status {
        Status::Available(release)
        | Status::Installing(release)
        | Status::Installed(release)
        | Status::Failed {
            release: Some(release),
            ..
        } => Some(release),
        _ => None,
    }
}
