//! The settings screen: the account, search's personal Spotify app,
//! playback, display, the window and storage.
//!
//! Opens with Ctrl+, or SETTINGS in the status bar, in place of the body.
//! Tab and Shift+Tab move through its controls, Enter uses the framed one,
//! Esc goes back.

use egui::{Align, Id, Key, Layout, Rect, Response, RichText, TextEdit, Ui, UiBuilder, vec2};

use super::View;
use super::theme::{self, Palette};
use super::widgets::{small_action, small_button, text_button};
use crate::action::Action;
use crate::backend::Store;
use crate::backend::{SearchState, SessionState};
use crate::browse::Loadable;
use crate::spotify::webapi::REDIRECT_URI;
use crate::store::AUDIO_CACHE_SIZES;

const DASHBOARD: &str = "https://developer.spotify.com/dashboard";
const FIELD_ID: &str = "settings-client-id";
/// The interface sizes − and + step through.
const ZOOMS: [f32; 9] = [0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5];

/// The screen's state while it is open.
pub struct Form {
    client_id: String,
    /// The control Tab got to, framed.
    item: Option<Item>,
    focus_field: bool,
    copied: bool,
    /// Tab moved: scroll the framed control into view.
    reveal: bool,
}

/// The controls Tab moves through, top to bottom.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Item {
    SignOut,
    Link,
    Copy,
    ClientId,
    Save,
    Levelling,
    /// One of the outputs listed, 0 being the system's default.
    Output(usize),
    Smaller,
    Larger,
    Covers,
    Tray,
    AudioCache,
    CacheDown,
    CacheUp,
    ClearAudio,
    ClearCovers,
}

pub fn open(view: &View) -> Form {
    Form {
        client_id: view.web_client_id.unwrap_or_default().to_string(),
        item: None,
        focus_field: false,
        copied: false,
        reveal: false,
    }
}

/// Draws the screen in `rect`. True when it should close.
pub fn draw(
    ui: &mut Ui,
    rect: Rect,
    view: &View,
    form: &mut Form,
    tab: &mut isize,
    actions: &mut Vec<Action>,
) -> bool {
    let palette = view.palette;
    theme::inset(ui.painter(), rect, palette.list_bg, &palette);
    let signed_in = matches!(view.session, SessionState::Ready { .. });
    let outputs = output_rows(view);
    let items: Vec<Item> = [
        Item::SignOut,
        Item::Link,
        Item::Copy,
        Item::ClientId,
        Item::Save,
        Item::Levelling,
    ]
    .into_iter()
    .chain((0..outputs.len()).map(Item::Output))
    .chain([
        Item::Smaller,
        Item::Larger,
        Item::Covers,
        Item::Tray,
        Item::AudioCache,
        Item::CacheDown,
        Item::CacheUp,
        Item::ClearAudio,
        Item::ClearCovers,
    ])
    .filter(|&item| item != Item::SignOut || signed_in)
    .collect();

    if *tab != 0 {
        let at = form
            .item
            .and_then(|item| items.iter().position(|&i| i == item));
        let next = match at {
            Some(at) => (at as isize + *tab).rem_euclid(items.len() as isize) as usize,
            None if *tab > 0 => 0,
            None => items.len() - 1,
        };
        *tab = 0;
        form.item = Some(items[next]);
        form.reveal = true;
        if items[next] == Item::ClientId {
            form.focus_field = true;
        } else {
            ui.ctx()
                .memory_mut(|m| m.surrender_focus(Id::new(FIELD_ID)));
        }
    }
    let typing = ui.ctx().text_edit_focused();
    let (enter, escape) = ui.input(|i| {
        let plain = i.modifiers.is_none();
        (
            plain && i.key_pressed(Key::Enter),
            plain && i.key_pressed(Key::Escape),
        )
    });
    // Enter uses the framed control (Enter in the field is the field's).
    let activated = if !typing && enter { form.item } else { None };
    let pressed = |item: Item| activated == Some(item);
    let mut close = !typing && escape;

    let current = view.web_client_id.unwrap_or_default();
    let typed = form.client_id.trim().to_string();
    let valid = typed.is_empty() || is_client_id(&typed);
    let changed = typed != current;

    ui.scope_builder(UiBuilder::new().max_rect(rect.shrink(16.0)), |ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Settings").size(15.0).color(palette.current));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if text_button(ui, &palette, "ESC · BACK", false, true).clicked() {
                    close = true;
                }
            });
        });

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
        section(ui, &palette, "ACCOUNT", None);
        ui.horizontal(|ui| {
            let who = match view.session {
                SessionState::Ready { user } => format!("Signed in to Spotify as {user}."),
                _ => "Not signed in.".into(),
            };
            ui.label(RichText::new(who).color(palette.list_fg));
            if signed_in {
                let button = small_button(ui, &palette, "SIGN OUT", false);
                ring(ui, &palette, form, Item::SignOut, &button);
                if button.clicked() || pressed(Item::SignOut) {
                    actions.push(Action::SignOut);
                    close = true;
                }
            }
        });

        let status = match view.search {
            SearchState::NoApp => "NOT SET UP",
            SearchState::SignedOut => "NOT APPROVED YET",
            SearchState::SigningIn => "WAITING FOR YOUR BROWSER",
            SearchState::Ready => "READY",
        };
        section(ui, &palette, "SEARCH", Some(status));
        note(
            ui,
            &palette,
            "Spotify only lets Spotless search through a Spotify app of your own. \
             Setting one up takes a minute:",
        );
        ui.horizontal_wrapped(|ui| {
            step(ui, &palette, "1.");
            ui.label(RichText::new("Create an app at").color(palette.chrome));
            let link = ui.hyperlink_to(
                RichText::new("developer.spotify.com/dashboard").color(palette.accent),
                DASHBOARD,
            );
            ring(ui, &palette, form, Item::Link, &link);
            if pressed(Item::Link) {
                ui.ctx().open_url(egui::OpenUrl::new_tab(DASHBOARD));
            }
            ui.label(RichText::new("(any name and description).").color(palette.chrome));
        });
        ui.horizontal_wrapped(|ui| {
            step(ui, &palette, "2.");
            ui.label(RichText::new("Add the Redirect URI").color(palette.chrome));
            ui.label(
                RichText::new(REDIRECT_URI)
                    .monospace()
                    .color(palette.list_fg),
            );
            let label = if form.copied { "COPIED" } else { "COPY" };
            let button = small_button(ui, &palette, label, false);
            ring(ui, &palette, form, Item::Copy, &button);
            if button.clicked() || pressed(Item::Copy) {
                ui.ctx().copy_text(REDIRECT_URI.into());
                form.copied = true;
            }
            ui.label(RichText::new("and tick Web API.").color(palette.chrome));
        });
        ui.horizontal(|ui| {
            step(ui, &palette, "3.");
            ui.label(RichText::new("Paste its Client ID:").color(palette.chrome));
            let field = ui.add(
                TextEdit::singleline(&mut form.client_id)
                    .id(Id::new(FIELD_ID))
                    .hint_text("Client ID")
                    .font(egui::TextStyle::Monospace)
                    .desired_width(270.0),
            );
            if std::mem::take(&mut form.focus_field) {
                field.request_focus();
            }
            if field.has_focus() {
                form.item = Some(Item::ClientId);
            }
            let submitted = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            let button = small_action(ui, &palette, "SAVE", changed && valid);
            ring(ui, &palette, form, Item::Save, &button);
            if changed && valid && (button.clicked() || pressed(Item::Save) || submitted) {
                actions.push(Action::SetSearchApp(typed.clone()));
            }
        });
        if !valid {
            note(
                ui,
                &palette,
                "That isn't a Client ID: it is 32 letters and digits, shown on the app's page.",
            );
        } else if *view.search == SearchState::SignedOut && !changed {
            note(
                ui,
                &palette,
                "Your browser asks once to approve search, at the first search.",
            );
        }

        let settings = view.settings;
        section(ui, &palette, "PLAYBACK", None);
        ui.horizontal(|ui| {
            let button = small_button(ui, &palette, "LEVEL VOLUME", settings.levelling);
            ring(ui, &palette, form, Item::Levelling, &button);
            if button.clicked() || pressed(Item::Levelling) {
                actions.push(Action::SetLevelling(!settings.levelling));
            }
            label(ui, &palette, "Evens out loudness between songs and albums");
        });
        ui.horizontal_wrapped(|ui| {
            label(ui, &palette, "Output");
            match view.outputs {
                Loadable::Ready(Some(_)) => {
                    for (i, (output, name)) in outputs.iter().enumerate() {
                        let chosen = *output == settings.audio_output;
                        let button = small_button(ui, &palette, name, chosen);
                        ring(ui, &palette, form, Item::Output(i), &button);
                        if !chosen && (button.clicked() || pressed(Item::Output(i))) {
                            actions.push(Action::SetOutput(output.clone()));
                        }
                    }
                }
                Loadable::Ready(None) | Loadable::Failed(_) => label(
                    ui,
                    &palette,
                    "the system's default. Change it in your system's sound settings.",
                ),
                Loadable::Loading => label(ui, &palette, "looking for outputs…"),
            }
        });

        section(ui, &palette, "DISPLAY", None);
        ui.horizontal(|ui| {
            label(ui, &palette, "Interface size");
            let zoom = ui.ctx().zoom_factor();
            let smaller = ZOOMS.iter().rev().find(|&&z| z < zoom - 0.001).copied();
            let larger = ZOOMS.iter().find(|&&z| z > zoom + 0.001).copied();
            let button = small_action(ui, &palette, "−", smaller.is_some());
            ring(ui, &palette, form, Item::Smaller, &button);
            if let Some(z) = smaller.filter(|_| button.clicked() || pressed(Item::Smaller)) {
                ui.ctx().set_zoom_factor(z);
            }
            ui.label(
                RichText::new(format!("{:.0}%", zoom * 100.0))
                    .monospace()
                    .color(palette.list_fg),
            );
            let button = small_action(ui, &palette, "+", larger.is_some());
            ring(ui, &palette, form, Item::Larger, &button);
            if let Some(z) = larger.filter(|_| button.clicked() || pressed(Item::Larger)) {
                ui.ctx().set_zoom_factor(z);
            }
            ui.label(RichText::new("also Ctrl + / − / 0").color(palette.dim));
        });
        ui.horizontal(|ui| {
            let button = small_button(ui, &palette, "COVERS", view.browse.show_covers);
            ring(ui, &palette, form, Item::Covers, &button);
            if button.clicked() || pressed(Item::Covers) {
                actions.push(Action::ToggleCovers);
            }
            label(ui, &palette, "Album covers next to albums in Browse");
        });

        section(ui, &palette, "WINDOW", None);
        ui.horizontal(|ui| {
            let button = small_button(ui, &palette, "SYSTEM TRAY", settings.tray);
            ring(ui, &palette, form, Item::Tray, &button);
            if button.clicked() || pressed(Item::Tray) {
                actions.push(Action::SetTray(!settings.tray));
            }
            let text = if !settings.tray {
                "Closing the window quits Spotless"
            } else if view.tray_shown {
                "Closing the window keeps Spotless playing in the tray (Ctrl+Q quits)"
            } else {
                "No panel shows tray icons here, so closing the window quits"
            };
            label(ui, &palette, text);
        });

        section(ui, &palette, "STORAGE", None);
        let used = |bytes: Option<u64>| bytes.map_or("measuring…".into(), size);
        ui.horizontal(|ui| {
            let button = small_button(ui, &palette, "AUDIO CACHE", settings.audio_cache);
            ring(ui, &palette, form, Item::AudioCache, &button);
            if button.clicked() || pressed(Item::AudioCache) {
                actions.push(Action::SetAudioCache(!settings.audio_cache));
            }
            label(ui, &palette, "Songs played start faster the next time");
        });
        ui.horizontal(|ui| {
            label(ui, &palette, "Up to");
            let at = AUDIO_CACHE_SIZES
                .iter()
                .position(|&mb| mb == settings.audio_cache_mb)
                .unwrap_or(2);
            let button = small_action(ui, &palette, "−", at > 0);
            ring(ui, &palette, form, Item::CacheDown, &button);
            if at > 0 && (button.clicked() || pressed(Item::CacheDown)) {
                actions.push(Action::SetAudioCacheSize(AUDIO_CACHE_SIZES[at - 1]));
            }
            ui.label(
                RichText::new(size(settings.audio_cache_mb * 1024 * 1024))
                    .monospace()
                    .color(palette.list_fg),
            );
            let last = AUDIO_CACHE_SIZES.len() - 1;
            let button = small_action(ui, &palette, "+", at < last);
            ring(ui, &palette, form, Item::CacheUp, &button);
            if at < last && (button.clicked() || pressed(Item::CacheUp)) {
                actions.push(Action::SetAudioCacheSize(AUDIO_CACHE_SIZES[at + 1]));
            }
            label(ui, &palette, &format!("· using {}", used(view.storage.map(|s| s.audio))));
            let button = small_button(ui, &palette, "CLEAR", false);
            ring(ui, &palette, form, Item::ClearAudio, &button);
            if button.clicked() || pressed(Item::ClearAudio) {
                actions.push(Action::ClearCache(Store::Audio));
            }
        });
        ui.horizontal(|ui| {
            label(
                ui,
                &palette,
                &format!("Album covers · using {}", used(view.storage.map(|s| s.covers))),
            );
            let button = small_button(ui, &palette, "CLEAR", false);
            ring(ui, &palette, form, Item::ClearCovers, &button);
            if button.clicked() || pressed(Item::ClearCovers) {
                actions.push(Action::ClearCache(Store::Covers));
            }
        });
        ui.label(
            RichText::new(
                "Kept in ~/.cache/spotless. Turning the audio cache on or off, \
                 and its size, take effect the next time Spotless starts.",
            )
            .color(palette.dim),
        );
            });
    });
    form.reveal = false;
    close
}

/// The outputs to choose from, as (choice, label): the system's default
/// first, then the sound server's, then a saved one that isn't connected.
fn output_rows(view: &View) -> Vec<(Option<String>, String)> {
    let Loadable::Ready(Some(listed)) = view.outputs else {
        return Vec::new();
    };
    let mut rows = vec![(None, "System default".to_string())];
    rows.extend(
        listed
            .iter()
            .map(|output| (Some(output.name.clone()), output.description.clone())),
    );
    if let Some(saved) = &view.settings.audio_output
        && !listed.iter().any(|output| output.name == *saved)
    {
        rows.push((Some(saved.clone()), "Chosen output (not connected)".into()));
    }
    rows
}

/// Spotify's client IDs: 32 hexadecimal digits.
fn is_client_id(text: &str) -> bool {
    text.len() == 32 && text.chars().all(|c| c.is_ascii_hexdigit())
}

/// Frames the control Tab got to, scrolling to it when Tab just moved.
fn ring(ui: &Ui, palette: &Palette, form: &Form, item: Item, response: &Response) {
    if form.item == Some(item) {
        theme::focus_ring(ui.painter(), response.rect.expand(3.0), palette);
        if form.reveal {
            ui.scroll_to_rect(response.rect, None);
        }
    }
}

/// "2.2 MB", "1.4 GB".
fn size(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let mb = bytes as f64 / MB;
    if bytes == 0 {
        "0 MB".into()
    } else if mb >= 1024.0 {
        format!("{:.1} GB", mb / 1024.0).replace(".0 ", " ")
    } else if mb >= 10.0 {
        format!("{mb:.0} MB")
    } else {
        format!("{mb:.1} MB")
    }
}

fn label(ui: &mut Ui, palette: &Palette, text: &str) {
    ui.label(RichText::new(text).color(palette.chrome));
}

fn section(ui: &mut Ui, palette: &Palette, title: &str, status: Option<&str>) {
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(title)
                .monospace()
                .size(10.0)
                .color(palette.dim),
        );
        if let Some(status) = status {
            ui.label(
                RichText::new(status)
                    .monospace()
                    .size(10.0)
                    .color(palette.accent),
            );
        }
    });
}

fn step(ui: &mut Ui, palette: &Palette, number: &str) {
    ui.label(RichText::new(number).monospace().color(palette.dim));
}

fn note(ui: &mut Ui, palette: &Palette, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).color(palette.chrome)).wrap());
}

#[cfg(test)]
mod tests {
    use super::is_client_id;

    #[test]
    fn client_ids_are_32_hex_digits() {
        assert!(is_client_id("0123456789abcdef0123456789ABCDEF"));
        assert!(!is_client_id("0123456789abcdef"));
        assert!(!is_client_id("0123456789abcdef0123456789abcdeg"));
    }

    #[test]
    fn sizes_read_naturally() {
        use super::size;
        assert_eq!(size(0), "0 MB");
        assert_eq!(size(2_306_867), "2.2 MB");
        assert_eq!(size(124_780_544), "119 MB");
        assert_eq!(size(2048 * 1024 * 1024), "2 GB");
        assert_eq!(size(1536 * 1024 * 1024), "1.5 GB");
        assert_eq!(size(512 * 1024 * 1024), "512 MB");
    }
}
