//! The window: title bar, player strip, body, status bar.
//!
//! Views read a [`View`] and push [`Action`]s; they never change state.

mod player_strip;
pub mod theme;
pub mod widgets;

use egui::{Align, Align2, FontId, Layout, Rect, RichText, TextEdit, Ui, UiBuilder, vec2};

use crate::action::Action;
use crate::backend::SessionState;
use crate::playback::engine::PlayerSnapshot;
use theme::Palette;
use widgets::text_button;

/// Everything the views read.
pub struct View<'a> {
    pub palette: Palette,
    pub session: &'a SessionState,
    pub player: &'a PlayerSnapshot,
    /// The last error to show in the status bar.
    pub error: Option<&'a str>,
}

/// Values the views edit in place: text fields and sliders.
#[derive(Default)]
pub struct Inputs {
    pub link: String,
    pub volume: u8,
    /// The seek slider's position while it is being dragged, in seconds.
    pub seeking: Option<f32>,
}

const TITLE_HEIGHT: f32 = 20.0;
const STATUS_HEIGHT: f32 = 22.0;
const GUTTER: f32 = 10.0;

pub fn draw(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Vec<Action>) {
    let palette = view.palette;
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
    theme::inset(ui.painter(), body, palette.list_bg, &palette);
    ui.scope_builder(UiBuilder::new().max_rect(body.shrink(16.0)), |ui| {
        body_contents(ui, view, inputs, actions);
    });

    status_bar(ui, status, view);
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
        SessionState::Ready { .. } => test_bench(ui, view, inputs, actions),
    }
}

/// M0's stand-in for the playlist view: play any track, album or artist link.
fn test_bench(ui: &mut Ui, view: &View, inputs: &mut Inputs, actions: &mut Vec<Action>) {
    let palette = view.palette;
    ui.label(
        RichText::new("Play a link")
            .size(15.0)
            .color(palette.current),
    );
    ui.label(
        RichText::new("Paste a Spotify track, album or artist link. Albums and artists play their first song.")
            .color(palette.dim),
    );
    ui.horizontal(|ui| {
        let field = ui.add(
            TextEdit::singleline(&mut inputs.link)
                .hint_text("https://open.spotify.com/track/…")
                .desired_width((ui.available_width() - 80.0).max(120.0))
                .min_size(vec2(0.0, widgets::BUTTON_HEIGHT)),
        );
        let submitted = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let clicked =
            text_button(ui, &palette, "PLAY", false, !inputs.link.trim().is_empty()).clicked();
        if (submitted || clicked) && !inputs.link.trim().is_empty() {
            actions.push(Action::PlayLink(inputs.link.trim().to_string()));
        }
    });
    ui.add_space(12.0);
    ui.label(RichText::new("Playlists arrive in M1.").color(palette.dim));
    ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
        if text_button(ui, &palette, "SIGN OUT", false, true).clicked() {
            actions.push(Action::SignOut);
        }
    });
}

fn status_bar(ui: &mut Ui, rect: Rect, view: &View) {
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
    painter
        .with_clip_rect(inner.with_max_x(inner.right() - 30.0))
        .text(
            inner.left_center(),
            Align2::LEFT_CENTER,
            text,
            FontId::proportional(12.0),
            color,
        );
    painter.text(
        inner.right_center(),
        Align2::RIGHT_CENTER,
        "M0",
        FontId::monospace(9.0),
        palette.chrome,
    );
}
