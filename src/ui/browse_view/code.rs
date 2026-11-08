//! The Spotify Code a page was opened from, drawn again under its header,
//! so it's plain where the page came from.

use egui::{Align2, FontId, Sense, Ui, pos2, vec2};

use super::album::share;
use crate::action::Actions;
use crate::ui::View;

/// The tallest bar, and the logo's width.
const HEIGHT: f32 = 44.0;
const BAR: f32 = 5.0;
const PITCH: f32 = 8.6;
const GAP: f32 = 12.0;
const CAPTION: f32 = 16.0;

/// The code the page shown was opened from, if it was, centred on a row
/// of its own, with `caption` (what it points to on the page) under it.
/// Right-clicked, it copies the link to what it points to.
pub(super) fn opened_code(ui: &mut Ui, view: &View, caption: Option<&str>, actions: &mut Actions) {
    let Some(code) = view.browse.code_here() else {
        return;
    };
    let width = HEIGHT + GAP + PITCH * 22.0 + BAR;
    let height = HEIGHT + if caption.is_some() { CAPTION } else { 0.0 };
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let rect = egui::Rect::from_center_size(row.center(), vec2(width, height));
    let response = ui.interact(rect, ui.id().with("opened code"), Sense::click());
    let copy = match code.uri.split(':').nth(1) {
        Some("artist") => "Copy artist link",
        Some("album") => "Copy album link",
        Some("track") => "Copy song link",
        _ => "Copy playlist link",
    };
    share(&response, copy, &code.uri, actions);
    response.on_hover_text("Opened from this Spotify Code. Right-click to copy its link");
    let painter = ui.painter();
    let color = view.palette.dim;
    let middle = rect.top() + HEIGHT / 2.0;
    painter.circle_filled(
        pos2(rect.left() + HEIGHT / 2.0, middle),
        HEIGHT / 2.0,
        color,
    );
    for (i, level) in code.bars.iter().enumerate() {
        let x = rect.left() + HEIGHT + GAP + PITCH * i as f32;
        let tall = BAR + (HEIGHT - BAR) * f32::from(*level) / 7.0;
        let bar = egui::Rect::from_center_size(pos2(x + BAR / 2.0, middle), vec2(BAR, tall));
        painter.rect_filled(bar, BAR / 2.0, color);
    }
    if let Some(caption) = caption {
        painter.text(
            pos2(rect.center().x, rect.bottom()),
            Align2::CENTER_BOTTOM,
            caption,
            FontId::monospace(10.0),
            color,
        );
    }
}
