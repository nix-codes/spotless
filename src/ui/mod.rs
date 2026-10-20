//! The window: title bar, player strip, separator, body, status bar.
//!
//! Views read a [`View`] and push [`Action`]s; they never change state.

mod browse_view;
mod keys;
mod player_strip;
mod playlist_view;
mod scrollbar;
mod settings_view;
pub mod skin_paint;
mod skinned_strip;
pub mod theme;
pub mod widgets;

use egui::{Align2, FontId, Rect, RichText, Ui, UiBuilder, ViewportCommand, vec2};

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
    pub lastfm: &'a crate::lastfm::LastFmState,
    /// The personal app search signs in with.
    pub web_client_id: Option<&'a str>,
    pub settings: &'a Settings,
    /// What the caches hold, once measured.
    pub storage: Option<Storage>,
    /// The audio outputs, once listed; None when they can't be.
    pub outputs: &'a Loadable<Option<Vec<Output>>>,
    /// A panel shows the tray icon.
    pub tray_shown: bool,
    /// The skins folder's contents, while Settings shows them.
    pub skins: &'a [String],
    pub skins_dir: &'a std::path::Path,
    /// The skin's pieces, when it has any.
    pub art: Option<&'a skin_paint::SkinPaint>,
    /// What the chosen skin lacks, said in Settings.
    pub skin_note: Option<&'a str>,
    /// Why the chosen skin couldn't be read.
    pub skin_error: Option<&'a str>,
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
    /// The settings category shown last, to open on next time.
    pub settings_category: settings_view::Category,
    /// How the window was last sized for PL.
    pub fit: Fit,
}

/// The window's size for PL: while the player shows alone, the height
/// (and scale) it was fitted to, and the height to give back after.
#[derive(Default)]
pub struct Fit {
    collapsed: Option<(f32, f32)>,
    expanded: Option<f32>,
}

impl Inputs {
    /// Settings is open on the skins (Display).
    pub fn showing_skins(&self) -> bool {
        self.settings
            .as_ref()
            .is_some_and(|form| form.category() == settings_view::Category::Display)
    }
}

const TITLE_HEIGHT: f32 = 20.0;
const TITLE: &str = "S P O T L E S S";
const STATUS_HEIGHT: f32 = 22.0;
const GUTTER: f32 = 10.0;
/// The smallest the window can be made, while the playlists show.
pub const MIN_WINDOW: egui::Vec2 = vec2(560.0, 360.0);
/// The window's height for the playlists when none is known yet.
const DEFAULT_HEIGHT: f32 = 600.0;

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
        close_settings(inputs);
    }
    keys::help(ui, &palette, inputs);
    let full = ui.max_rect();
    ui.painter().rect_filled(full, 0.0, palette.frame);
    theme::bevel(ui.painter(), full, true, &palette);

    let skinned = view.art.filter(|paint| paint.has("titlebar"));
    // A bar of stripes with a name: the window's title, and the separator
    // between the player and the screen below, as Winamp's playlist editor
    // had its own title bar under the main window.
    let bar = |ui: &Ui, top: f32, label: &str| match skinned {
        Some(paint) => {
            let unit = skinned_strip::unit(paint, full.width() - 2.0 * GUTTER);
            let height = paint
                .size("titlebar", unit)
                .map_or(TITLE_HEIGHT, |size| size.y);
            let rect =
                Rect::from_min_size(egui::pos2(full.left(), top), vec2(full.width(), height));
            skinned_strip::title_bar(ui, rect, view, paint, unit, label);
            rect
        }
        None => {
            let rect = Rect::from_min_size(
                egui::pos2(full.left(), top),
                vec2(full.width(), TITLE_HEIGHT),
            );
            title_bar(ui, rect, &palette, label);
            rect
        }
    };
    let title = bar(ui, full.top(), TITLE);

    let strip_top = title.bottom() + 4.0;
    let strip = Rect::from_min_max(
        egui::pos2(full.left() + GUTTER, strip_top),
        egui::pos2(full.right() - GUTTER, strip_top + player_strip::HEIGHT),
    );
    ui.scope_builder(UiBuilder::new().max_rect(strip), |ui| {
        player_strip::draw(ui, view, inputs, actions);
    });

    // With PL off the window is the player alone, unless Browse, the
    // settings, the keys or a question needs the room.
    let ready = matches!(view.session, SessionState::Ready { .. });
    let collapsed = ready
        && !view.settings.playlists
        && !view.browse.open
        && inputs.settings.is_none()
        && !inputs.help
        && inputs.closing.is_none();
    let status_top = strip.bottom() + 6.0;
    fit_window(
        ui,
        full,
        &mut inputs.fit,
        collapsed.then(|| status_top + STATUS_HEIGHT - full.top()),
    );
    if collapsed {
        let status = Rect::from_min_size(
            egui::pos2(full.left(), status_top),
            vec2(full.width(), STATUS_HEIGHT),
        );
        status_bar(ui, status, view, inputs, actions);
        inputs.tab = 0;
        return;
    }
    let screen = if settings_open {
        "S E T T I N G S"
    } else if ready && view.browse.open {
        "B R O W S E"
    } else {
        "P L A Y L I S T S"
    };
    let separator = bar(ui, strip.bottom() + 6.0, screen);

    let status = Rect::from_min_max(
        egui::pos2(full.left(), full.bottom() - STATUS_HEIGHT),
        full.max,
    );
    let body = Rect::from_min_max(
        egui::pos2(full.left() + GUTTER, separator.bottom() + 4.0),
        egui::pos2(full.right() - GUTTER, status.top() - 6.0),
    );
    if let Some(form) = &mut inputs.settings {
        if settings_view::draw(ui, body, view, form, &mut inputs.tab, actions) {
            close_settings(inputs);
        }
        status_bar(ui, status, view, inputs, actions);
        inputs.tab = 0;
        return;
    }
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

/// Fits the window to the player alone, `height` points of the window's
/// content `full`, and holds it there; with None, gives the playlists back
/// the height the window had before.
fn fit_window(ui: &Ui, full: Rect, fit: &mut Fit, height: Option<f32>) {
    let ctx = ui.ctx();
    let screen = ctx.content_rect();
    let send = |command| ctx.send_viewport_cmd(command);
    match height {
        Some(height) => {
            let height = height + screen.height() - full.height();
            let wanted = (height, ctx.pixels_per_point());
            if fit.collapsed == Some(wanted) {
                return;
            }
            if fit.collapsed.is_none() {
                fit.expanded = Some(screen.height());
            }
            fit.collapsed = Some(wanted);
            send(ViewportCommand::MinInnerSize(vec2(MIN_WINDOW.x, height)));
            send(ViewportCommand::MaxInnerSize(vec2(16384.0, height)));
            send(ViewportCommand::InnerSize(vec2(screen.width(), height)));
        }
        None => {
            if fit.collapsed.take().is_none() {
                return;
            }
            let height = fit.expanded.unwrap_or(DEFAULT_HEIGHT);
            send(ViewportCommand::MaxInnerSize(egui::Vec2::INFINITY));
            send(ViewportCommand::MinInnerSize(MIN_WINDOW));
            send(ViewportCommand::InnerSize(vec2(screen.width(), height)));
        }
    }
}

fn title_bar(ui: &Ui, rect: Rect, palette: &Palette, label: &str) {
    let painter = ui.painter();
    let font = FontId::monospace(10.0);
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
    if button(ui, "F10 · SETTINGS") {
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
    inputs.settings = Some(settings_view::open(view, inputs.settings_category));
    actions.push(Action::CloseBrowse);
    actions.push(Action::MeasureStorage);
    actions.push(Action::ListOutputs);
}

/// Leaves the settings, remembering the category shown.
fn close_settings(inputs: &mut Inputs) {
    if let Some(form) = inputs.settings.take() {
        inputs.settings_category = form.category();
    }
}

/// F10 and the SETTINGS button: open, or close when open.
pub(super) fn toggle_settings(view: &View, inputs: &mut Inputs, actions: &mut Vec<Action>) {
    if inputs.settings.is_some() {
        close_settings(inputs);
    } else {
        open_settings(view, inputs, actions);
    }
}
