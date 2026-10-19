//! The window: title bar, player strip, body, status bar.
//!
//! Views read a [`View`] and push [`Action`]s; they never change state.

mod browse_view;
mod keys;
mod player_strip;
mod playlist_view;
mod settings_view;
pub mod theme;
pub mod widgets;

use egui::{Align2, FontId, Rect, RichText, Ui, UiBuilder, vec2};

use crate::action::Action;
use crate::backend::{SearchState, SessionState, Storage};
use crate::browse::{Browse, Loadable};
use crate::model::{EntryId, Library, Playlist, PlaylistId};
use crate::playback::engine::PlayerSnapshot;
use crate::playback::outputs::Output;
use crate::store::Settings;
use theme::Palette;
use widgets::text_button;

/// Everything the views read.
pub struct View<'a> {
    pub palette: Palette,
    pub session: &'a SessionState,
    pub player: &'a PlayerSnapshot,
    /// The last error to show in the status bar.
    pub error: Option<&'a str>,
    /// News for the status bar, shown when there is no error.
    pub notice: Option<&'a str>,
    /// The playlist in the editor.
    pub playlist: &'a Playlist,
    pub shuffle: bool,
    pub repeat: bool,
    pub library: &'a Library,
    /// The playlist a song of is loaded from.
    pub playing_list: Option<PlaylistId>,
    pub browse: &'a Browse,
    pub search: &'a SearchState,
    /// The personal app search signs in with.
    pub web_client_id: Option<&'a str>,
    pub settings: &'a Settings,
    /// What the caches hold, once measured.
    pub storage: Option<Storage>,
    /// The audio outputs, once listed; None when they can't be.
    pub outputs: &'a Loadable<Option<Vec<Output>>>,
}

/// Values the views edit in place: text fields and sliders.
#[derive(Default)]
pub struct Inputs {
    pub volume: u8,
    /// The seek slider's position while it is being dragged, in seconds.
    pub seeking: Option<f32>,
    pub selection: playlist_view::Selection,
    /// The catalog's search field.
    pub query: String,
    /// Put the keyboard in the search field next frame.
    pub focus_search: bool,
    /// The tab being renamed, and the name typed so far.
    pub renaming: Option<(PlaylistId, String)>,
    /// The playlist waiting for Close to be confirmed.
    pub closing: Option<PlaylistId>,
    /// The volume to go back to after muting with M.
    pub unmuted: Option<u8>,
    /// A row to select once its playlist is the one shown (J).
    pub jump: Option<(PlaylistId, EntryId)>,
    /// The F1 list of keys is open.
    pub help: bool,
    /// Tab presses this frame, minus Shift+Tab ones.
    pub tab: isize,
    /// Browse's areas and highlighted rows.
    pub nav: browse_view::Nav,
    /// The settings screen, while it is open.
    pub settings: Option<settings_view::Form>,
}

const TITLE_HEIGHT: f32 = 20.0;
const STATUS_HEIGHT: f32 = 22.0;
const GUTTER: f32 = 10.0;

pub fn draw(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Vec<Action>) {
    let palette = view.palette;
    let settings_open = inputs.settings.is_some();
    keys::global(ui, view, inputs, actions);
    // A key for another screen (Ctrl+B, J, Ctrl+T…) leaves the settings.
    let elsewhere = actions.iter().any(|action| {
        matches!(
            action,
            Action::ToggleBrowse | Action::SelectPlaylist(_) | Action::NewPlaylist
        )
    });
    if settings_open && (elsewhere || inputs.renaming.is_some() || inputs.focus_search) {
        inputs.settings = None;
    }
    keys::help(ui, &palette, inputs);
    let full = ui.max_rect();
    ui.painter().rect_filled(full, 0.0, palette.frame);
    theme::bevel(ui.painter(), full, true, &palette);

    let title = Rect::from_min_size(full.min, vec2(full.width(), TITLE_HEIGHT));
    title_bar(ui, title, &palette);

    let strip_top = title.bottom() + 4.0;
    let strip = Rect::from_min_max(
        egui::pos2(full.left() + GUTTER, strip_top),
        egui::pos2(full.right() - GUTTER, strip_top + player_strip::HEIGHT),
    );
    ui.scope_builder(UiBuilder::new().max_rect(strip), |ui| {
        player_strip::draw(ui, view, inputs, actions);
    });

    let status = Rect::from_min_max(
        egui::pos2(full.left(), full.bottom() - STATUS_HEIGHT),
        full.max,
    );
    let body = Rect::from_min_max(
        egui::pos2(full.left() + GUTTER, strip.bottom() + GUTTER),
        egui::pos2(full.right() - GUTTER, status.top() - 6.0),
    );
    if let Some(form) = &mut inputs.settings {
        if settings_view::draw(ui, body, view, form, &mut inputs.tab, actions) {
            inputs.settings = None;
        }
        status_bar(ui, status, view, inputs, actions);
        inputs.tab = 0;
        return;
    }
    let ready = matches!(view.session, SessionState::Ready { .. });
    if ready && view.browse.open {
        browse_view::draw(ui, body, view, inputs, actions);
        status_bar(ui, status, view, inputs, actions);
        inputs.tab = 0;
        return;
    }
    theme::inset(ui.painter(), body, palette.list_bg, &palette);
    let padding = if matches!(view.session, SessionState::Ready { .. }) {
        8.0
    } else {
        16.0
    };
    ui.scope_builder(UiBuilder::new().max_rect(body.shrink(padding)), |ui| {
        body_contents(ui, view, inputs, actions);
    });

    status_bar(ui, status, view, inputs, actions);
    // Tab only means something in Browse.
    inputs.tab = 0;
}

fn title_bar(ui: &Ui, rect: Rect, palette: &Palette) {
    let painter = ui.painter();
    let font = FontId::monospace(10.0);
    let label = "S P O T L E S S";
    let text_width = painter
        .layout_no_wrap(label.into(), font.clone(), palette.chrome)
        .size()
        .x;
    let groove = |left: f32, right: f32| {
        Rect::from_min_max(
            egui::pos2(left, rect.center().y - 3.5),
            egui::pos2(right, rect.center().y + 3.5),
        )
    };
    let gap = text_width / 2.0 + 10.0;
    theme::stripes(
        painter,
        groove(rect.left() + 8.0, rect.center().x - gap),
        palette,
    );
    theme::stripes(
        painter,
        groove(rect.center().x + gap, rect.right() - 8.0),
        palette,
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        font,
        palette.chrome,
    );
}

fn body_contents(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Vec<Action>) {
    let palette = view.palette;
    let heading = |text: &str| RichText::new(text).size(15.0).color(palette.current);
    let dim = |text: &str| RichText::new(text).color(palette.dim);
    ui.spacing_mut().item_spacing = vec2(8.0, 10.0);
    match view.session {
        SessionState::Connecting => {
            ui.label(dim("Connecting to Spotify…"));
        }
        SessionState::SignedOut => {
            ui.label(heading("Sign in to play"));
            ui.label(dim("Spotless plays through your Spotify Premium account. \
                 Your browser opens for Spotify's approval."));
            if text_button(ui, &palette, "SIGN IN WITH SPOTIFY", false, true).clicked() {
                actions.push(Action::SignIn);
            }
        }
        SessionState::SigningIn => {
            ui.label(heading("Waiting for your browser"));
            ui.label(dim(
                "Approve Spotless in the Spotify page that opened, then come back here.",
            ));
        }
        SessionState::Failed(message) => {
            ui.label(heading("Couldn't connect"));
            ui.label(RichText::new(message).color(palette.list_fg));
            if text_button(ui, &palette, "SIGN IN AGAIN", false, true).clicked() {
                actions.push(Action::SignIn);
            }
        }
        SessionState::Ready { .. } => playlist_view::draw(ui, view, inputs, actions),
    }
}

fn status_bar(
    ui: &mut Ui,
    rect: Rect,
    view: &View,
    inputs: &mut Inputs,
    actions: &mut Vec<Action>,
) {
    let palette = view.palette;
    let painter = ui.painter();
    painter.line_segment(
        [
            rect.left_top() + vec2(1.0, 0.0),
            rect.right_top() - vec2(1.0, 0.0),
        ],
        egui::Stroke::new(1.0, palette.lo),
    );
    let (text, color) = match view.error {
        Some(error) => (error.to_string(), palette.current),
        None if view.notice.is_some() => (view.notice.unwrap_or_default().to_string(), palette.dim),
        None => (
            match view.session {
                SessionState::Ready { user } => format!("Signed in as {user}"),
                SessionState::Connecting => "Connecting…".into(),
                _ => "Not signed in".into(),
            },
            palette.dim,
        ),
    };
    let inner = rect.shrink2(vec2(GUTTER, 0.0));
    // Buttons from the right: SETTINGS, then KEYS.
    let mut right = inner.right();
    let mut button = |ui: &mut Ui, label: &str| {
        let width = widgets::small_button_width(ui, label);
        let at = Rect::from_center_size(
            egui::pos2(right - width / 2.0, inner.center().y),
            vec2(width, widgets::SMALL_HEIGHT),
        );
        right -= width + 6.0;
        let mut child = ui.new_child(UiBuilder::new().max_rect(at));
        widgets::small_button(&mut child, &palette, label, false).clicked()
    };
    if button(ui, "SETTINGS") {
        toggle_settings(view, inputs, actions);
    }
    if button(ui, "F1 · KEYS") {
        inputs.help = true;
    }
    ui.painter()
        .with_clip_rect(inner.with_max_x(right - 10.0))
        .text(
            inner.left_center(),
            Align2::LEFT_CENTER,
            text,
            FontId::proportional(12.0),
            color,
        );
}

/// Shows the settings in place of the playlists or Browse.
fn open_settings(view: &View, inputs: &mut Inputs, actions: &mut Vec<Action>) {
    inputs.settings = Some(settings_view::open(view));
    actions.push(Action::CloseBrowse);
    actions.push(Action::MeasureStorage);
    actions.push(Action::ListOutputs);
}

/// Ctrl+, and the SETTINGS button: open, or close when open.
pub(super) fn toggle_settings(view: &View, inputs: &mut Inputs, actions: &mut Vec<Action>) {
    if inputs.settings.take().is_none() {
        open_settings(view, inputs, actions);
    }
}
