//! The settings screen: the account, search's personal Spotify app, covers.
//!
//! Opens with Ctrl+, or SETTINGS in the status bar, in place of the body.
//! Tab and Shift+Tab move through its controls, Enter uses the framed one,
//! Esc goes back.

use egui::{Align, Id, Key, Layout, Rect, Response, RichText, TextEdit, Ui, UiBuilder, vec2};

use super::View;
use super::theme::{self, Palette};
use super::widgets::{small_action, small_button, text_button};
use crate::action::Action;
use crate::backend::{SearchState, SessionState};
use crate::spotify::webapi::REDIRECT_URI;

const DASHBOARD: &str = "https://developer.spotify.com/dashboard";
const FIELD_ID: &str = "settings-client-id";

/// The screen's state while it is open.
pub struct Form {
    client_id: String,
    /// The control Tab got to, framed.
    item: Option<Item>,
    focus_field: bool,
    copied: bool,
}

/// The controls Tab moves through, top to bottom.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Item {
    SignOut,
    Link,
    Copy,
    ClientId,
    Save,
    Covers,
}

pub fn open(view: &View) -> Form {
    Form {
        client_id: view.web_client_id.unwrap_or_default().to_string(),
        item: None,
        focus_field: false,
        copied: false,
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
    let items: Vec<Item> = [
        Item::SignOut,
        Item::Link,
        Item::Copy,
        Item::ClientId,
        Item::Save,
        Item::Covers,
    ]
    .into_iter()
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

        section(ui, &palette, "CATALOG", None);
        ui.horizontal(|ui| {
            let button = small_button(ui, &palette, "COVERS", view.browse.show_covers);
            ring(ui, &palette, form, Item::Covers, &button);
            if button.clicked() || pressed(Item::Covers) {
                actions.push(Action::ToggleCovers);
            }
            ui.label(RichText::new("Album covers next to albums in Browse").color(palette.chrome));
        });
    });
    close
}

/// Spotify's client IDs: 32 hexadecimal digits.
fn is_client_id(text: &str) -> bool {
    text.len() == 32 && text.chars().all(|c| c.is_ascii_hexdigit())
}

/// Frames the control Tab got to.
fn ring(ui: &Ui, palette: &Palette, form: &Form, item: Item, response: &Response) {
    if form.item == Some(item) {
        theme::focus_ring(ui.painter(), response.rect.expand(3.0), palette);
    }
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
}
