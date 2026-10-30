//! A question in a box over the window, answered with one of its buttons.
//!
//! The keys belong to it while it shows: ← → (and ↑ ↓, Tab) move the ring
//! between the buttons, Enter presses the ringed one, the first at first;
//! Esc, or a click outside, answers none.

use egui::{Event, Id, Key, Stroke, Ui};

use super::theme::{self, Palette};
use super::widgets::small_button;

/// How a question stands after a frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Answer {
    Waiting,
    /// The button at this index, of those given.
    Chose(usize),
    /// Esc, or a click outside.
    Dismissed,
}

/// Shows the question `id`: `body` above a row of `buttons`, each a label
/// and its tooltip ("" for none).
pub fn show(
    ui: &Ui,
    palette: &Palette,
    id: Id,
    buttons: &[(&str, &str)],
    body: impl FnOnce(&mut Ui),
) -> Answer {
    let ring = id.with("ring");
    let last = buttons.len().saturating_sub(1);
    let mut at = ui
        .data(|d| d.get_temp::<usize>(ring))
        .unwrap_or(0)
        .min(last);
    let (step, enter, escape) = ui.input_mut(|i| {
        let (mut step, mut enter, mut escape) = (0isize, false, false);
        for event in &i.events {
            if let Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            {
                match key {
                    Key::ArrowLeft | Key::ArrowUp => step -= 1,
                    Key::ArrowRight | Key::ArrowDown => step += 1,
                    Key::Tab if modifiers.shift => step -= 1,
                    Key::Tab => step += 1,
                    Key::Enter => enter = true,
                    Key::Escape => escape = true,
                    _ => {}
                }
            }
        }
        // Nothing behind the question acts on a key meanwhile.
        i.events
            .retain(|event| !matches!(event, Event::Key { .. } | Event::Text(_)));
        (step, enter, escape)
    });
    at = at.saturating_add_signed(step).min(last);
    let modal = egui::Modal::new(id)
        .frame(
            egui::Frame::new()
                .fill(palette.frame)
                .stroke(Stroke::new(1.0, palette.hi))
                .inner_margin(16),
        )
        .show(ui.ctx(), |ui| {
            body(ui);
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                let mut clicked = None;
                for (index, &(label, tip)) in buttons.iter().enumerate() {
                    let mut button = small_button(ui, palette, label, false);
                    if !tip.is_empty() {
                        button = button.on_hover_text(tip);
                    }
                    if index == at {
                        theme::focus_ring(ui.painter(), button.rect.expand(3.0), palette);
                    }
                    if button.clicked() {
                        clicked = Some(index);
                    }
                }
                clicked
            })
            .inner
        });
    let answer = match modal.inner {
        Some(index) => Answer::Chose(index),
        None if enter && !buttons.is_empty() => Answer::Chose(at),
        None if escape || modal.backdrop_response.clicked() => Answer::Dismissed,
        None => Answer::Waiting,
    };
    ui.data_mut(|d| {
        if answer == Answer::Waiting {
            d.insert_temp(ring, at);
        } else {
            // The next question starts on its first button again.
            d.remove::<usize>(ring);
        }
    });
    answer
}

#[cfg(test)]
mod tests {
    use egui::{Modifiers, Pos2, RawInput, Rect, vec2};

    use super::*;

    /// The answers to a question of three buttons, a frame for each of
    /// `presses`.
    fn answers(presses: &[Key]) -> Vec<Answer> {
        let ctx = egui::Context::default();
        let mut answers = Vec::new();
        for (n, &key) in presses.iter().enumerate() {
            let raw = RawInput {
                time: Some(n as f64 * 0.2),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 400.0))),
                events: vec![Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                }],
                ..RawInput::default()
            };
            let mut output = ctx.run_ui(raw, |ui| {
                let buttons = [("YES", ""), ("NO", ""), ("MAYBE", "")];
                let palette = Palette::CLASSIC;
                answers.push(show(ui, &palette, Id::new("q"), &buttons, |_| {}));
            });
            output.textures_delta.clear();
        }
        answers
    }

    #[test]
    fn enter_presses_the_first_button_at_first() {
        assert_eq!(answers(&[Key::Enter]), [Answer::Chose(0)]);
    }

    #[test]
    fn the_arrows_move_between_the_buttons_and_stop_at_the_ends() {
        use Key::*;
        let last = *answers(&[ArrowRight, ArrowRight, ArrowRight, Enter])
            .last()
            .unwrap();
        assert_eq!(last, Answer::Chose(2));
        let last = *answers(&[ArrowRight, ArrowLeft, ArrowLeft, Enter])
            .last()
            .unwrap();
        assert_eq!(last, Answer::Chose(0));
        // Answered, the next question starts on its first button again.
        let all = answers(&[ArrowDown, Enter, Enter]);
        assert_eq!(all[1..], [Answer::Chose(1), Answer::Chose(0)]);
    }

    #[test]
    fn escape_answers_none() {
        assert_eq!(answers(&[Key::Escape]), [Answer::Dismissed]);
    }
}
