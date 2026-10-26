//! The bar under the player that says a newer Spotless is out: UPDATE,
//! its notes (WHAT'S NEW), and once it is installed, RESTART. × hides it
//! until Spotless next starts; Settings › About turns it off for good.

use egui::{Align, Id, Label, Layout, Rect, RichText, ScrollArea, Ui, UiBuilder, pos2, vec2};

use super::settings_view::open_in_browser;
use super::theme::{self, Palette};
use super::widgets::small_button;
use super::{View, scrollbar};
use crate::action::{Actions, Desktop};
use crate::update::{Release, Status};

/// The bar's row, and the height its notes take below it when open.
const ROW: f32 = 30.0;
const NOTES: f32 = 160.0;

/// What the bar keeps between frames.
#[derive(Default)]
pub struct Bar {
    /// × was pressed: hidden until Spotless next starts.
    pub hidden: bool,
    /// The notes are open.
    pub notes: bool,
}

/// The bar's height, 0 when it doesn't show.
pub fn height(view: &View, bar: &Bar) -> f32 {
    match shown(view, bar) {
        Some(release) if bar.notes && has_notes(release) => ROW + NOTES,
        Some(_) => ROW,
        None => 0.0,
    }
}

/// The release the bar is about, while it shows.
fn shown<'a>(view: &View<'a>, bar: &Bar) -> Option<&'a Release> {
    release(view.update).filter(|_| view.settings.update_notices && !bar.hidden)
}

fn has_notes(release: &Release) -> bool {
    !release.notes.trim().is_empty()
}

pub fn draw(ui: &mut Ui, rect: Rect, view: &View, bar: &mut Bar, actions: &mut Actions) {
    let Some(release) = shown(view, bar) else {
        return;
    };
    let palette = view.palette;
    theme::panel(ui.painter(), rect, &palette);
    let version = release.version;
    let text = match view.update {
        Status::Installing(_) => format!("Downloading Spotless {version}…"),
        Status::Installed(_) => {
            format!("Spotless {version} is installed: it starts the next time Spotless does.")
        }
        Status::Failed { error, .. } => format!("Couldn't update to Spotless {version}: {error}"),
        _ => format!("Spotless {version} is out."),
    };
    let row = Rect::from_min_size(rect.min, vec2(rect.width(), ROW)).shrink2(vec2(10.0, 0.0));
    ui.scope_builder(UiBuilder::new().max_rect(row), |ui| {
        // The buttons from the right; the words take what is left.
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if small_button(ui, &palette, "×", false)
                .on_hover_text("Hide until Spotless starts again")
                .clicked()
            {
                bar.hidden = true;
            }
            if has_notes(release) {
                let label = if bar.notes {
                    "HIDE NOTES"
                } else {
                    "WHAT'S NEW"
                };
                if small_button(ui, &palette, label, bar.notes).clicked() {
                    bar.notes = !bar.notes;
                }
            }
            if !release.installable() {
                if small_button(ui, &palette, "DOWNLOAD", false).clicked() {
                    open_in_browser(&release.page);
                }
            } else if let Some((label, action)) = update_button(view.update)
                && small_button(ui, &palette, label, false).clicked()
            {
                actions.push(action);
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.add(Label::new(RichText::new(text).color(palette.current)).truncate());
            });
        });
    });
    if !bar.notes || !has_notes(release) {
        return;
    }
    let area = Rect::from_min_max(
        pos2(rect.left() + 10.0, row.bottom()),
        rect.max - vec2(4.0, 6.0),
    );
    ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
        let id = Id::new("update-notes");
        // An ID of its own: the playlist's area would otherwise share its
        // offset.
        let area = ScrollArea::vertical().id_salt(id);
        scrollbar::show(ui, view, id, area, |area, ui| {
            area.show(ui, |ui| {
                notes(ui, &palette, &release.notes);
                ui.add_space(4.0);
                let link = RichText::new("Release page on GitHub").color(palette.accent);
                if ui.link(link).clicked() {
                    open_in_browser(&release.page);
                }
            })
        });
    });
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
