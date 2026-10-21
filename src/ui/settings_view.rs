//! The settings screen, laid out like Winamp's Preferences: the categories
//! on the left, the chosen one's options on the right.
//!
//! Opens with F10 (or Ctrl+,) or SETTINGS in the status bar, in place of the
//! body. Up/Down move through the categories, or through the options once
//! Enter or Tab went into them; Tab and Shift+Tab go on through the options
//! and back to the categories. Enter uses the framed option, Esc goes back.

use egui::{
    Align, Align2, FontId, Id, Key, Layout, Rect, Response, RichText, Sense, Stroke, TextEdit, Ui,
    UiBuilder, pos2, vec2,
};

use super::View;
use super::skin_paint;
use super::theme::{self, Palette};
use super::widgets::{field, small_action, small_button, text_button};
use crate::action::{Action, Actions, Desktop};
use crate::backend::{SearchState, SessionState, Store};
use crate::browse::Loadable;
use crate::lastfm::LastFmStatus;
use crate::lastfm::api::is_key;
use crate::skin;
use crate::spotify::webapi::REDIRECT_URI;
use crate::store::AUDIO_CACHE_SIZES;

const DASHBOARD: &str = "https://developer.spotify.com/dashboard";
const LASTFM_CREATE: &str = "https://www.last.fm/api/account/create";
const FIELD_ID: &str = "settings-client-id";
const KEY_FIELD_ID: &str = "settings-lastfm-key";
const SECRET_FIELD_ID: &str = "settings-lastfm-secret";
/// The interface sizes − and + step through.
const ZOOMS: [f32; 9] = [0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5];

const LIST_WIDTH: f32 = 150.0;
const LIST_ROW: f32 = 26.0;
const LABEL_WIDTH: f32 = 150.0;
const ROW_GAP: f32 = 14.0;
const PAD: f32 = 10.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Category {
    #[default]
    Account,
    LastFm,
    Playback,
    Display,
    Window,
    Storage,
}

const CATEGORIES: [(Category, &str); 6] = [
    (Category::Account, "Account"),
    (Category::LastFm, "Last.fm"),
    (Category::Playback, "Playback"),
    (Category::Display, "Display"),
    (Category::Window, "Window"),
    (Category::Storage, "Storage"),
];

/// The screen's state while it is open.
pub struct Form {
    category: Category,
    /// The keyboard is in the options, not the categories.
    in_options: bool,
    /// The option the keyboard is on, framed.
    item: Option<Item>,
    /// The keyboard was used last: the frames show.
    keyboard: bool,
    /// The keyboard just moved: scroll the framed option into view.
    reveal: bool,
    client_id: String,
    lastfm_key: String,
    lastfm_secret: String,
    /// The text field to put the keyboard in next frame.
    focus_field: Option<Item>,
    copied: bool,
}

impl Form {
    pub fn category(&self) -> Category {
        self.category
    }
}

/// The options, in the order the keyboard goes through them.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Item {
    SignOut,
    ClientId,
    Save,
    Link,
    Copy,
    LastFmKey,
    LastFmSecret,
    LastFmConnect,
    LastFmDisconnect,
    LastFmLink,
    Levelling,
    /// One of the outputs listed, 0 being the system's default.
    Output(usize),
    Smaller,
    Larger,
    Covers,
    /// One of the skins listed, 0 being Spotless Classic.
    Skin(usize),
    Tray,
    AudioCache,
    CacheDown,
    CacheUp,
    ClearAudio,
    ClearCovers,
}

pub fn open(view: &View, category: Category) -> Form {
    Form {
        category,
        in_options: false,
        item: None,
        keyboard: false,
        reveal: false,
        client_id: view.web_client_id.unwrap_or_default().to_string(),
        lastfm_key: view.lastfm.api_key.clone(),
        lastfm_secret: view.lastfm.secret.clone(),
        focus_field: None,
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
    actions: &mut Actions,
) -> bool {
    let palette = view.palette;
    let outputs = output_rows(view);
    let items = items(form.category, view, outputs.len());
    let typing = ui.ctx().text_edit_focused();
    if ui.input(|i| i.pointer.any_pressed()) {
        form.keyboard = false;
    }
    if *tab != 0 {
        step_tab(ui, form, &items, *tab);
        *tab = 0;
    }
    let (up, down, enter, escape) = ui.input(|i| {
        let plain = i.modifiers.is_none();
        (
            plain && i.key_pressed(Key::ArrowUp),
            plain && i.key_pressed(Key::ArrowDown),
            plain && i.key_pressed(Key::Enter),
            plain && i.key_pressed(Key::Escape),
        )
    });
    if !typing && (up || down) {
        form.keyboard = true;
        form.reveal = true;
        if form.in_options {
            let at = form
                .item
                .and_then(|item| items.iter().position(|&i| i == item));
            let to = match at {
                Some(at) if up => at.saturating_sub(1),
                Some(at) => (at + 1).min(items.len().saturating_sub(1)),
                None => 0,
            };
            form.item = items.get(to).copied();
        } else {
            let at = CATEGORIES
                .iter()
                .position(|(c, _)| *c == form.category)
                .unwrap_or(0);
            let to = if up {
                at.saturating_sub(1)
            } else {
                (at + 1).min(CATEGORIES.len() - 1)
            };
            form.category = CATEGORIES[to].0;
        }
    }
    // Enter on a category goes into its options; on an option, uses it.
    let mut activated = None;
    if !typing && enter {
        form.keyboard = true;
        if form.in_options {
            activated = form.item;
        } else if let Some(&first) = items.first() {
            enter_option(ui, form, first);
        }
    }
    let pressed = |item: Item| activated == Some(item);
    let mut close = !typing && escape;

    // The header, the categories and the panel.
    let header = Rect::from_min_size(rect.min, vec2(rect.width(), 30.0));
    let body = Rect::from_min_max(pos2(rect.left(), header.bottom() + 6.0), rect.max);
    let list = Rect::from_min_size(body.min, vec2(LIST_WIDTH, body.height()));
    let panel = Rect::from_min_max(pos2(list.right() + PAD, body.top()), body.max);
    ui.scope_builder(UiBuilder::new().max_rect(header), |ui| {
        ui.horizontal_centered(|ui| {
            ui.label(RichText::new("Settings").size(15.0).color(palette.current));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if text_button(ui, &palette, "ESC · BACK", false, true).clicked() {
                    close = true;
                }
            });
        });
    });
    categories(ui, list, &palette, form);
    theme::panel(ui.painter(), panel, &palette);
    if form.keyboard && !form.in_options {
        theme::focus_ring(ui.painter(), list, &palette);
    }
    let title = CATEGORIES
        .iter()
        .find(|(c, _)| *c == form.category)
        .map_or("", |(_, name)| name)
        .to_uppercase();
    let title_at = pos2(panel.left() + 14.0, panel.top() + 16.0);
    ui.painter().text(
        title_at,
        Align2::LEFT_CENTER,
        title,
        FontId::monospace(10.0),
        palette.dim,
    );
    ui.painter().hline(
        panel.left() + 12.0..=panel.right() - 12.0,
        panel.top() + 28.0,
        Stroke::new(1.0, palette.lo),
    );
    let content = Rect::from_min_max(
        pos2(panel.left() + 14.0, panel.top() + 40.0),
        pos2(panel.right() - 8.0, panel.bottom() - 8.0),
    );
    ui.scope_builder(UiBuilder::new().max_rect(content), |ui| {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 4.0);
                let mut c = Controls {
                    view,
                    palette: &palette,
                    form,
                    pressed: &pressed,
                    actions,
                    close: &mut close,
                };
                match c.form.category {
                    Category::Account => {
                        c.account(ui);
                        c.search(ui);
                    }
                    Category::LastFm => c.lastfm(ui),
                    Category::Playback => c.playback(ui, &outputs),
                    Category::Display => c.display(ui),
                    Category::Window => c.window(ui),
                    Category::Storage => c.storage(ui),
                }
            });
    });
    form.reveal = false;
    close
}

/// The category list, the chosen one lit with a bar at its left.
fn categories(ui: &mut Ui, rect: Rect, palette: &Palette, form: &mut Form) {
    theme::panel(ui.painter(), rect, palette);
    let mut top = rect.top() + 6.0;
    for (category, name) in CATEGORIES {
        let row = Rect::from_min_size(
            pos2(rect.left() + 3.0, top),
            vec2(rect.width() - 6.0, LIST_ROW),
        );
        top += LIST_ROW;
        let response = ui.interact(row, Id::new(("settings-category", name)), Sense::click());
        let chosen = form.category == category;
        if chosen {
            ui.painter().rect_filled(row, 0.0, palette.selected_bg);
            ui.painter().rect_filled(
                Rect::from_min_size(row.min, vec2(3.0, row.height())),
                0.0,
                palette.accent,
            );
        } else if response.hovered() {
            ui.painter()
                .rect_filled(row, 0.0, palette.hi.gamma_multiply(0.35));
        }
        ui.painter().text(
            pos2(row.left() + 12.0, row.center().y),
            Align2::LEFT_CENTER,
            name,
            FontId::proportional(13.0),
            if chosen {
                palette.current
            } else {
                palette.chrome
            },
        );
        if response.clicked() {
            form.category = category;
            form.in_options = false;
            form.item = None;
        }
    }
}

/// The options of each category, in the order Tab and the arrows take.
fn items(category: Category, view: &View, outputs: usize) -> Vec<Item> {
    match category {
        Category::Account => {
            let signed_in = matches!(view.session, SessionState::Ready { .. });
            signed_in
                .then_some(Item::SignOut)
                .into_iter()
                .chain([Item::ClientId, Item::Save, Item::Link, Item::Copy])
                .collect()
        }
        Category::LastFm => {
            let mut items = Vec::new();
            if matches!(view.lastfm.status, LastFmStatus::Connected { .. }) {
                items.push(Item::LastFmDisconnect);
            }
            items.extend([
                Item::LastFmKey,
                Item::LastFmSecret,
                Item::LastFmConnect,
                Item::LastFmLink,
            ]);
            items
        }
        Category::Playback => std::iter::once(Item::Levelling)
            .chain((0..outputs).map(Item::Output))
            .collect(),
        Category::Display => [Item::Smaller, Item::Larger, Item::Covers]
            .into_iter()
            .chain((0..skin_rows().len()).map(Item::Skin))
            .collect(),
        Category::Window => vec![Item::Tray],
        Category::Storage => vec![
            Item::AudioCache,
            Item::CacheDown,
            Item::CacheUp,
            Item::ClearAudio,
            Item::ClearCovers,
        ],
    }
}

/// Tab and Shift+Tab: from the categories into the options, through them,
/// and back.
fn step_tab(ui: &Ui, form: &mut Form, items: &[Item], tab: isize) {
    form.keyboard = true;
    form.reveal = true;
    // Position 0 is the category list; the options follow.
    let at = match (form.in_options, form.item) {
        (true, Some(item)) => items.iter().position(|&i| i == item).map_or(0, |i| i + 1),
        _ => 0,
    };
    let stops = items.len() as isize + 1;
    let to = (at as isize + tab).rem_euclid(stops) as usize;
    if to == 0 {
        form.in_options = false;
        form.item = None;
        surrender_fields(ui);
    } else {
        enter_option(ui, form, items[to - 1]);
    }
}

fn enter_option(ui: &Ui, form: &mut Form, item: Item) {
    form.in_options = true;
    form.item = Some(item);
    form.reveal = true;
    if matches!(item, Item::ClientId | Item::LastFmKey | Item::LastFmSecret) {
        form.focus_field = Some(item);
    } else {
        surrender_fields(ui);
    }
}

/// Takes the keyboard out of the text fields.
fn surrender_fields(ui: &Ui) {
    for id in [FIELD_ID, KEY_FIELD_ID, SECRET_FIELD_ID] {
        ui.ctx().memory_mut(|m| m.surrender_focus(Id::new(id)));
    }
}

/// What each category's panel draws with.
struct Controls<'a, 'v> {
    view: &'a View<'v>,
    palette: &'a Palette,
    form: &'a mut Form,
    pressed: &'a dyn Fn(Item) -> bool,
    actions: &'a mut Actions,
    close: &'a mut bool,
}

impl Controls<'_, '_> {
    fn account(&mut self, ui: &mut Ui) {
        let palette = self.palette;
        match self.view.session {
            SessionState::Ready { user } => {
                row(ui, palette, "Spotify account", |ui| {
                    ui.label(RichText::new(user).color(palette.list_fg));
                });
                row(ui, palette, "", |ui| {
                    let button = small_button(ui, palette, "SIGN OUT", false);
                    self.ring(ui, Item::SignOut, &button);
                    if button.clicked() || (self.pressed)(Item::SignOut) {
                        self.actions.push(Action::SignOut);
                        *self.close = true;
                    }
                    note(ui, palette, "Signing in again asks your browser once.");
                });
            }
            _ => row(ui, palette, "Spotify account", |ui| {
                note(ui, palette, "Not signed in. Sign in from the main screen.");
            }),
        }
    }

    fn search(&mut self, ui: &mut Ui) {
        let palette = self.palette;
        let view = self.view;
        let (status, ready) = match view.search {
            SearchState::NoApp => ("NOT SET UP", false),
            SearchState::SignedOut => ("NOT APPROVED YET", false),
            SearchState::SigningIn => ("WAITING FOR YOUR BROWSER", false),
            SearchState::Ready => ("READY", true),
        };
        row(ui, palette, "Search", |ui| {
            let color = if ready {
                palette.accent
            } else {
                palette.current
            };
            ui.label(RichText::new(status).monospace().color(color));
            note(
                ui,
                palette,
                "Besides signing in, Spotify only lets Spotless search through a Spotify \
                 app of your own.",
            );
        });
        let current = view.web_client_id.unwrap_or_default();
        let typed = self.form.client_id.trim().to_string();
        let valid = typed.is_empty() || is_client_id(&typed);
        let changed = typed != current;
        row(ui, palette, "Client ID", |ui| {
            ui.horizontal(|ui| {
                let field = field(
                    ui,
                    TextEdit::singleline(&mut self.form.client_id)
                        .id(Id::new(FIELD_ID))
                        .hint_text("32 letters and digits")
                        .font(egui::TextStyle::Monospace)
                        .desired_width(270.0),
                );
                if self.form.focus_field == Some(Item::ClientId) {
                    self.form.focus_field = None;
                    field.request_focus();
                }
                if field.has_focus() {
                    self.form.in_options = true;
                    self.form.item = Some(Item::ClientId);
                }
                let submitted = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                let button = small_action(ui, palette, "SAVE", changed && valid);
                self.ring(ui, Item::Save, &button);
                if changed && valid && (button.clicked() || (self.pressed)(Item::Save) || submitted)
                {
                    self.actions.push(Action::SetSearchApp(typed.clone()));
                }
            });
            if !valid {
                note(
                    ui,
                    palette,
                    "That isn't a Client ID: it is 32 letters and digits, on the app's page.",
                );
            } else if *view.search == SearchState::SignedOut && !changed {
                note(
                    ui,
                    palette,
                    "Your browser asks once to approve search, at the first search.",
                );
            }
        });
        row(ui, palette, "Setting one up", |ui| {
            ui.horizontal_wrapped(|ui| {
                step(ui, palette, "1.");
                label(ui, palette, "Create an app at");
                let link =
                    ui.link(RichText::new("developer.spotify.com/dashboard").color(palette.accent));
                self.ring(ui, Item::Link, &link);
                if link.clicked() || (self.pressed)(Item::Link) {
                    open_in_browser(DASHBOARD);
                }
            });
            ui.horizontal_wrapped(|ui| {
                step(ui, palette, "2.");
                label(ui, palette, "Add the Redirect URI");
                ui.label(
                    RichText::new(REDIRECT_URI)
                        .monospace()
                        .color(palette.list_fg),
                );
                let text = if self.form.copied { "COPIED" } else { "COPY" };
                let button = small_button(ui, palette, text, false);
                self.ring(ui, Item::Copy, &button);
                if button.clicked() || (self.pressed)(Item::Copy) {
                    ui.ctx().copy_text(REDIRECT_URI.into());
                    self.form.copied = true;
                }
            });
            ui.horizontal_wrapped(|ui| {
                step(ui, palette, "3.");
                label(
                    ui,
                    palette,
                    "Tick Web API, then paste the app's Client ID above.",
                );
            });
        });
    }

    fn lastfm(&mut self, ui: &mut Ui) {
        let palette = self.palette;
        let state = self.view.lastfm;
        let (status, connected) = match &state.status {
            LastFmStatus::Off => ("NOT CONNECTED".to_string(), false),
            LastFmStatus::SigningIn => ("WAITING FOR YOUR BROWSER".to_string(), false),
            LastFmStatus::Connected { user } => (format!("CONNECTED AS {user}"), true),
        };
        row(ui, palette, "Status", |ui| {
            let color = if connected {
                palette.accent
            } else {
                palette.current
            };
            ui.label(RichText::new(status).monospace().color(color));
            note(
                ui,
                palette,
                "Spotless scrobbles the songs you play through a Last.fm API account of your own.",
            );
        });
        if connected {
            row(ui, palette, "", |ui| {
                let button = small_button(ui, palette, "DISCONNECT", false);
                self.ring(ui, Item::LastFmDisconnect, &button);
                if button.clicked() || (self.pressed)(Item::LastFmDisconnect) {
                    self.actions.push(Action::DisconnectLastFm);
                }
                note(
                    ui,
                    palette,
                    "Listens Last.fm can't take at once are kept and sent later.",
                );
            });
        }
        let key = self.form.lastfm_key.trim().to_string();
        let secret = self.form.lastfm_secret.trim().to_string();
        let valid = is_key(&key) && is_key(&secret);
        let changed = key != state.api_key || secret != state.secret;
        let ready = valid && state.status != LastFmStatus::SigningIn && (changed || !connected);
        let mut submitted = false;
        row(ui, palette, "API key", |ui| {
            let field = field(
                ui,
                TextEdit::singleline(&mut self.form.lastfm_key)
                    .id(Id::new(KEY_FIELD_ID))
                    .hint_text("32 letters and digits")
                    .font(egui::TextStyle::Monospace)
                    .desired_width(270.0),
            );
            self.field_focus(&field, Item::LastFmKey);
            submitted |= field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        });
        row(ui, palette, "Shared secret", |ui| {
            ui.horizontal(|ui| {
                let field = field(
                    ui,
                    TextEdit::singleline(&mut self.form.lastfm_secret)
                        .id(Id::new(SECRET_FIELD_ID))
                        .hint_text("32 letters and digits")
                        .password(true)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(270.0),
                );
                self.field_focus(&field, Item::LastFmSecret);
                submitted |= field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                let button = small_action(ui, palette, "CONNECT", ready);
                self.ring(ui, Item::LastFmConnect, &button);
                if ready && (button.clicked() || (self.pressed)(Item::LastFmConnect) || submitted) {
                    self.actions
                        .push(Action::ConnectLastFm(key.clone(), secret.clone()));
                }
            });
            let typed = !key.is_empty() || !secret.is_empty();
            if typed && !valid {
                note(
                    ui,
                    palette,
                    "Both are 32 letters and digits, on your API account's page.",
                );
            } else if connected && changed {
                note(
                    ui,
                    palette,
                    "CONNECT to scrobble through this account instead.",
                );
            }
        });
        row(ui, palette, "Setting one up", |ui| {
            ui.horizontal_wrapped(|ui| {
                step(ui, palette, "1.");
                label(ui, palette, "Create an API account at");
                let link =
                    ui.link(RichText::new("last.fm/api/account/create").color(palette.accent));
                self.ring(ui, Item::LastFmLink, &link);
                if link.clicked() || (self.pressed)(Item::LastFmLink) {
                    open_in_browser(LASTFM_CREATE);
                }
            });
            ui.horizontal_wrapped(|ui| {
                step(ui, palette, "2.");
                label(
                    ui,
                    palette,
                    "Any application name will do. Leave the callback URL empty.",
                );
            });
            ui.horizontal_wrapped(|ui| {
                step(ui, palette, "3.");
                label(
                    ui,
                    palette,
                    "Paste its API key and shared secret above, then CONNECT and allow Spotless in your browser.",
                );
            });
        });
    }

    /// Puts the keyboard in a text field when asked, and keeps the
    /// framed option on it while it has the keyboard.
    fn field_focus(&mut self, field: &Response, item: Item) {
        if self.form.focus_field == Some(item) {
            self.form.focus_field = None;
            field.request_focus();
        }
        if field.has_focus() {
            self.form.in_options = true;
            self.form.item = Some(item);
        }
    }

    fn playback(&mut self, ui: &mut Ui, outputs: &[(Option<String>, String)]) {
        let palette = self.palette;
        let settings = self.view.settings;
        row(ui, palette, "Volume levelling", |ui| {
            let switch = toggle(ui, palette, settings.levelling);
            self.ring(ui, Item::Levelling, &switch);
            if switch.clicked() || (self.pressed)(Item::Levelling) {
                self.actions.push(Action::SetLevelling(!settings.levelling));
            }
            note(ui, palette, "Evens out loudness between songs and albums.");
        });
        row(ui, palette, "Output", |ui| match self.view.outputs {
            Loadable::Ready(Some(_)) => {
                for (i, (output, name)) in outputs.iter().enumerate() {
                    let chosen = *output == settings.audio_output;
                    let choice = radio(ui, palette, chosen, name);
                    self.ring(ui, Item::Output(i), &choice);
                    if !chosen && (choice.clicked() || (self.pressed)(Item::Output(i))) {
                        self.actions.push(Action::SetOutput(output.clone()));
                    }
                }
            }
            Loadable::Ready(None) | Loadable::Failed(_) => note(
                ui,
                palette,
                "The system's default. Change it in your system's sound settings.",
            ),
            Loadable::Loading => note(ui, palette, "Looking for outputs…"),
        });
    }

    fn display(&mut self, ui: &mut Ui) {
        let palette = self.palette;
        row(ui, palette, "Interface size", |ui| {
            ui.horizontal(|ui| {
                let zoom = ui.ctx().zoom_factor();
                let smaller = ZOOMS.iter().rev().find(|&&z| z < zoom - 0.001).copied();
                let larger = ZOOMS.iter().find(|&&z| z > zoom + 0.001).copied();
                let button = small_action(ui, palette, "−", smaller.is_some());
                self.ring(ui, Item::Smaller, &button);
                if let Some(z) =
                    smaller.filter(|_| button.clicked() || (self.pressed)(Item::Smaller))
                {
                    ui.ctx().set_zoom_factor(z);
                }
                value(ui, palette, &format!("{:.0}%", zoom * 100.0));
                let button = small_action(ui, palette, "+", larger.is_some());
                self.ring(ui, Item::Larger, &button);
                if let Some(z) = larger.filter(|_| button.clicked() || (self.pressed)(Item::Larger))
                {
                    ui.ctx().set_zoom_factor(z);
                }
            });
            note(ui, palette, "Also Ctrl + / − / 0, anywhere.");
        });
        row(ui, palette, "Album covers", |ui| {
            let switch = toggle(ui, palette, self.view.browse.show_covers);
            self.ring(ui, Item::Covers, &switch);
            if switch.clicked() || (self.pressed)(Item::Covers) {
                self.actions.push(Action::ToggleCovers);
            }
            note(ui, palette, "Thumbnails next to albums in Browse.");
        });
        row(ui, palette, "Skin", |ui| {
            let chosen = self.view.settings.skin.as_deref();
            for (i, (skin, name)) in skin_rows().into_iter().enumerate() {
                let on = skin.as_deref() == chosen;
                let choice = radio(ui, palette, on, &name);
                self.ring(ui, Item::Skin(i), &choice);
                if !on && (choice.clicked() || (self.pressed)(Item::Skin(i))) {
                    self.actions.push(Desktop::SetSkin(skin));
                }
            }
            if let Some(error) = self.view.skin_error {
                ui.label(RichText::new(error).color(palette.current));
            } else if let Some(text) = self.view.skin_note {
                note(ui, palette, text);
            }
        });
    }

    fn window(&mut self, ui: &mut Ui) {
        let palette = self.palette;
        let settings = self.view.settings;
        row(ui, palette, "System tray", |ui| {
            let switch = toggle(ui, palette, settings.tray);
            self.ring(ui, Item::Tray, &switch);
            if switch.clicked() || (self.pressed)(Item::Tray) {
                self.actions.push(Desktop::SetTray(!settings.tray));
            }
            let text = if !settings.tray {
                "Closing the window quits Spotless."
            } else if self.view.tray_shown {
                "Closing the window keeps Spotless playing in the tray. Ctrl+Q quits."
            } else {
                "No panel shows tray icons here, so closing the window quits."
            };
            note(ui, palette, text);
        });
    }

    fn storage(&mut self, ui: &mut Ui) {
        let palette = self.palette;
        let settings = self.view.settings;
        let storage = self.view.storage;
        let used = |bytes: Option<u64>| bytes.map_or("measuring…".into(), size);
        row(ui, palette, "Audio cache", |ui| {
            let switch = toggle(ui, palette, settings.audio_cache);
            self.ring(ui, Item::AudioCache, &switch);
            if switch.clicked() || (self.pressed)(Item::AudioCache) {
                self.actions
                    .push(Action::SetAudioCache(!settings.audio_cache));
            }
            note(ui, palette, "Songs played start faster the next time.");
        });
        row(ui, palette, "Cache size", |ui| {
            ui.horizontal(|ui| {
                let at = AUDIO_CACHE_SIZES
                    .iter()
                    .position(|&mb| mb == settings.audio_cache_mb)
                    .unwrap_or(2);
                let last = AUDIO_CACHE_SIZES.len() - 1;
                let button = small_action(ui, palette, "−", at > 0);
                self.ring(ui, Item::CacheDown, &button);
                if at > 0 && (button.clicked() || (self.pressed)(Item::CacheDown)) {
                    self.actions
                        .push(Action::SetAudioCacheSize(AUDIO_CACHE_SIZES[at - 1]));
                }
                value(ui, palette, &size(settings.audio_cache_mb * 1024 * 1024));
                let button = small_action(ui, palette, "+", at < last);
                self.ring(ui, Item::CacheUp, &button);
                if at < last && (button.clicked() || (self.pressed)(Item::CacheUp)) {
                    self.actions
                        .push(Action::SetAudioCacheSize(AUDIO_CACHE_SIZES[at + 1]));
                }
            });
            note(
                ui,
                palette,
                "The switch and the size take effect the next time Spotless starts.",
            );
        });
        row(ui, palette, "Audio in use", |ui| {
            ui.horizontal(|ui| {
                value(ui, palette, &used(storage.map(|s| s.audio)));
                let button = small_button(ui, palette, "CLEAR", false);
                self.ring(ui, Item::ClearAudio, &button);
                if button.clicked() || (self.pressed)(Item::ClearAudio) {
                    self.actions.push(Action::ClearCache(Store::Audio));
                }
            });
        });
        row(ui, palette, "Covers in use", |ui| {
            ui.horizontal(|ui| {
                value(ui, palette, &used(storage.map(|s| s.covers)));
                let button = small_button(ui, palette, "CLEAR", false);
                self.ring(ui, Item::ClearCovers, &button);
                if button.clicked() || (self.pressed)(Item::ClearCovers) {
                    self.actions.push(Action::ClearCache(Store::Covers));
                }
            });
            note(ui, palette, "Both are kept in ~/.cache/spotless.");
        });
    }

    /// Frames the option the keyboard is on, scrolling to it when the
    /// keyboard just moved there.
    fn ring(&self, ui: &Ui, item: Item, response: &Response) {
        if self.form.keyboard && self.form.in_options && self.form.item == Some(item) {
            theme::focus_ring(ui.painter(), response.rect.expand(3.0), self.palette);
            if self.form.reveal {
                ui.scroll_to_rect(response.rect, None);
            }
        }
    }
}

/// A labelled row: the label in its column, the controls beside it.
fn row(ui: &mut Ui, palette: &Palette, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal_top(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(LABEL_WIDTH, 22.0), Sense::hover());
        ui.painter().text(
            rect.left_center(),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(12.5),
            palette.chrome,
        );
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 5.0;
            add(ui);
        });
    });
    ui.add_space(ROW_GAP);
}

/// An on/off switch: a bevelled button with a light and ON or OFF.
fn toggle(ui: &mut Ui, palette: &Palette, on: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(62.0, 22.0), Sense::click());
    let painter = ui.painter();
    let state = skin_paint::state(&response, false, true);
    let skinned = skin_paint::chrome(painter, "small_button", state, rect);
    if !skinned {
        painter.rect_filled(rect, 0.0, palette.button);
        theme::bevel(
            painter,
            rect,
            !response.is_pointer_button_down_on(),
            &palette.for_buttons(),
        );
    }
    let light = Rect::from_center_size(pos2(rect.left() + 13.0, rect.center().y), vec2(9.0, 9.0));
    painter.rect_filled(light, 0.0, if on { palette.accent } else { palette.lcd_bg });
    painter.rect_stroke(
        light,
        0.0,
        Stroke::new(1.0, palette.lo),
        egui::StrokeKind::Outside,
    );
    let color = if skinned {
        palette.button_text
    } else if response.hovered() || on {
        palette.accent
    } else {
        palette.button_text
    };
    painter.text(
        pos2(light.right() + 8.0, rect.center().y),
        Align2::LEFT_CENTER,
        if on { "ON" } else { "OFF" },
        FontId::monospace(10.0),
        color,
    );
    response
}

/// One of several choices: a round light and its name.
fn radio(ui: &mut Ui, palette: &Palette, chosen: bool, text: &str) -> Response {
    let font = FontId::proportional(12.5);
    let width = ui
        .painter()
        .layout_no_wrap(text.to_string(), font.clone(), palette.chrome)
        .size()
        .x;
    let (rect, response) = ui.allocate_exact_size(vec2(22.0 + width, 20.0), Sense::click());
    let painter = ui.painter();
    let center = pos2(rect.left() + 7.0, rect.center().y);
    painter.circle_filled(center, 6.0, palette.lcd_bg);
    painter.circle_stroke(center, 6.0, Stroke::new(1.0, palette.hi));
    if chosen {
        painter.circle_filled(center, 3.0, palette.accent);
    }
    let color = if chosen {
        palette.current
    } else if response.hovered() {
        palette.accent
    } else {
        palette.chrome
    };
    painter.text(
        pos2(rect.left() + 20.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        font,
        color,
    );
    response
}

/// The skins to choose from, as (choice, label).
fn skin_rows() -> [(Option<String>, String); 2] {
    let winamp = skin::format::built_in::WINAMP_CLASSIC_KEY;
    [
        (None, "Spotless Classic".to_string()),
        (Some(winamp.to_string()), "Winamp Classic".to_string()),
    ]
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

/// Opens a page in the browser. Not through egui, whose link opening is
/// left out of the build.
fn open_in_browser(url: &str) {
    if let Err(error) = open::that_detached(url) {
        log::warn!("unable to open {url}: {error}");
    }
}

/// Spotify's client IDs: 32 hexadecimal digits.
fn is_client_id(text: &str) -> bool {
    text.len() == 32 && text.chars().all(|c| c.is_ascii_hexdigit())
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

/// A value shown between − and +, or beside CLEAR.
fn value(ui: &mut Ui, palette: &Palette, text: &str) {
    ui.label(RichText::new(text).monospace().color(palette.list_fg));
}

fn step(ui: &mut Ui, palette: &Palette, number: &str) {
    ui.label(RichText::new(number).monospace().color(palette.dim));
}

/// An explanation under a control.
fn note(ui: &mut Ui, palette: &Palette, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).size(11.5).color(palette.dim)).wrap());
}

#[cfg(test)]
mod tests {
    use super::{is_client_id, size};

    #[test]
    fn client_ids_are_32_hex_digits() {
        assert!(is_client_id("0123456789abcdef0123456789ABCDEF"));
        assert!(!is_client_id("0123456789abcdef"));
        assert!(!is_client_id("0123456789abcdef0123456789abcdeg"));
    }

    #[test]
    fn sizes_read_naturally() {
        assert_eq!(size(0), "0 MB");
        assert_eq!(size(2_306_867), "2.2 MB");
        assert_eq!(size(124_780_544), "119 MB");
        assert_eq!(size(2048 * 1024 * 1024), "2 GB");
        assert_eq!(size(1536 * 1024 * 1024), "1.5 GB");
        assert_eq!(size(512 * 1024 * 1024), "512 MB");
    }
}
