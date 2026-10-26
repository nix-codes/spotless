//! The deck under the player (VIS): the player's buttons become the keys
//! of a Walkman or a reel-to-reel deck, which plays the song on a cassette
//! or open reels, with the music's spectrum and its left and right levels
//! beside it.
//!
//! The Walkman's keys stay where the buttons always are, standing out of
//! its top edge; the reel deck has its keys along its bottom, as real ones
//! do. Clicking the device switches to the other. The tape winds from the
//! left reel to the right one as the song plays, and back fast when
//! another starts; the reels turn only while it plays.

mod canvas;
mod cassette;
mod reel;
mod walkman;

pub use canvas::install_fonts;

use egui::{Align2, Color32, FontId, Painter, Rect, Sense, Ui, pos2, vec2};

use super::{Inputs, View, player_strip, theme};
use crate::action::{Action, Actions};
use crate::playback::analyser::{self, BANDS, Reading};
use crate::playback::engine::Status;
use crate::store::DeckMedia;
use canvas::{Canvas, rgb};

/// Points round the keys, beside them and above and below.
const SIDE: f32 = 4.0;
const MARGIN: f32 = 6.0;
/// Points between the device and the spectrum.
const GAP: f32 = 10.0;
/// The Walkman's body below the keys' band, in its units.
const WALKMAN_BODY: f32 = 172.0;
/// The levels' box under the spectrum, in points.
const LEVELS_HEIGHT: f32 = 38.0;
/// The bars span this many dB: a band at TOP fills its bar.
const BAR_FLOOR: f32 = -60.0;
const BAR_TOP: f32 = -6.0;
/// The levels span -40 to 0 dBFS, in LEDs of a dB each.
const LEVEL_FLOOR: f32 = -40.0;
const LEVEL_LEDS: usize = 40;
const SEGMENTS: usize = 16;
/// How fast the tape moves, in the cassette's units per second; the reels
/// turn by it, faster the less tape they hold.
const TAPE_SPEED: f32 = 48.0;
/// How fast it winds to a new place, in songs per second.
const WIND_SPEED: f32 = 1.6;
const GREEN: Color32 = rgb(0x33e65f);
const YELLOW: Color32 = rgb(0xffd02b);
const RED: Color32 = rgb(0xff3b2b);

/// What a frame of the device shows.
pub struct Tape {
    /// How much of the song has played: the share on the right reel.
    pub progress: f32,
    /// The left and right reels' turn, in radians.
    pub angles: [f32; 2],
    /// A for odd songs in the playlist, B for even ones.
    pub side: char,
    pub list: String,
    pub song: String,
    pub playing: bool,
    pub counter: f32,
    /// The VU meters' needles, 0–1.
    pub needles: [f32; 2],
}

/// What moves from frame to frame.
#[derive(Default)]
pub struct Motion {
    angles: [f32; 2],
    counter: f32,
    /// The song's progress as drawn, which winds to the real one.
    drawn: Option<f32>,
    winding: bool,
    bars: [f32; BANDS],
    tops: [f32; BANDS],
    holds: [f32; BANDS],
    levels: [f32; 2],
    level_tops: [f32; 2],
    level_holds: [f32; 2],
    needles: [f32; 2],
}

impl Motion {
    /// Moves everything on by `dt` seconds; says whether anything still
    /// moves.
    fn step(&mut self, view: &View, reading: &Reading, dt: f32) -> bool {
        let player = view.player;
        let playing = player.status == Status::Playing;
        let target = match &player.track {
            Some(track) if track.duration_ms > 0 && player.status != Status::Stopped => {
                (player.position_now() as f32 / track.duration_ms as f32).clamp(0.0, 1.0)
            }
            _ => 0.0,
        };
        let drawn = self.drawn.get_or_insert(target);
        let gap = target - *drawn;
        let pack = |share: f32| match view.settings.deck_media {
            DeckMedia::Cassette => cassette::pack(share),
            DeckMedia::Reel => reel::pack(share) / 1.6,
        };
        let mut turn = |speed: f32, share: f32| {
            self.angles[0] += speed / pack(1.0 - share) * dt;
            self.angles[1] += speed / pack(share) * dt;
        };
        if gap.abs() > 0.02 || (self.winding && gap.abs() > 0.001) {
            self.winding = true;
            let step = (WIND_SPEED * dt).min(gap.abs()) * gap.signum();
            *drawn += step;
            turn(900.0 * gap.signum(), *drawn);
            self.counter += 40.0 * dt * gap.signum();
        } else {
            self.winding = false;
            *drawn = target;
            if playing {
                turn(TAPE_SPEED, target);
                self.counter += 0.9 * dt;
            }
        }
        self.counter = self.counter.max(0.0);

        let share = |db: f32, floor: f32, top: f32| ((db - floor) / (top - floor)).clamp(0.0, 1.0);
        for band in 0..BANDS {
            let value = share(reading.bands[band], BAR_FLOOR, BAR_TOP);
            fall(&mut self.bars[band], value, 1.6 * dt);
            hold(
                &mut self.tops[band],
                &mut self.holds[band],
                self.bars[band],
                0.5,
                0.7 * dt,
                dt,
            );
        }
        for side in 0..2 {
            let value = share(reading.peaks[side], LEVEL_FLOOR, 0.0);
            fall(&mut self.levels[side], value, 1.4 * dt);
            let level = self.levels[side];
            hold(
                &mut self.level_tops[side],
                &mut self.level_holds[side],
                level,
                0.8,
                0.5 * dt,
                dt,
            );
            self.needles[side] += (level * 0.92 - self.needles[side]) * (dt * 6.0).min(1.0);
        }
        playing
            || self.winding
            || self
                .tops
                .iter()
                .chain(&self.level_tops)
                .any(|top| *top > 0.0)
            || self.needles.iter().any(|needle| *needle > 0.002)
    }

    fn tape(&self, view: &View) -> Tape {
        let (side, list, song) = match player_strip::playing_entry(view)
            .and_then(|(list, entry)| Some((view.list(list)?, entry)))
            .and_then(|(list, entry)| Some((list, list.index_of(entry)?, list.get(entry)?)))
        {
            Some((list, index, entry)) => (
                if index % 2 == 0 { 'A' } else { 'B' },
                list.name.clone(),
                format!("{}. {}", index + 1, entry.track.title),
            ),
            None => ('A', String::new(), String::new()),
        };
        Tape {
            progress: self.drawn.unwrap_or(0.0),
            angles: self.angles,
            side,
            list,
            song,
            playing: view.player.status == Status::Playing,
            counter: self.counter,
            needles: self.needles,
        }
    }
}

/// Rises at once to `value`, falls by at most `by`.
fn fall(level: &mut f32, value: f32, by: f32) {
    *level = if value > *level {
        value
    } else {
        (*level - by).max(value)
    };
}

/// A peak mark: up with `level`, held `seconds`, then falling by `by`.
fn hold(top: &mut f32, held: &mut f32, level: f32, seconds: f32, by: f32, dt: f32) {
    if level >= *top {
        *top = level;
        *held = seconds;
    } else if *held > 0.0 {
        *held -= dt;
    } else {
        *top = (*top - by).max(0.0);
    }
}

/// A bar at `angle` from `r0` to `r1` round `x, y`, `width` across: a
/// hub's tooth or key.
#[allow(clippy::too_many_arguments)]
fn spoke(c: &Canvas, x: f32, y: f32, angle: f32, r0: f32, r1: f32, width: f32, colour: Color32) {
    let (ax, ay) = (angle.cos(), angle.sin());
    let (nx, ny) = (-ay * width / 2.0, ax * width / 2.0);
    c.polygon(
        &[
            (x + ax * r0 + nx, y + ay * r0 + ny),
            (x + ax * r1 + nx, y + ay * r1 + ny),
            (x + ax * r1 - nx, y + ay * r1 - ny),
            (x + ax * r0 - nx, y + ay * r0 - ny),
        ],
        colour,
    );
}

/// Draws the deck from the strip's buttons down, the buttons among it;
/// returns its bottom.
pub fn draw(
    ui: &mut Ui,
    strip: Rect,
    view: &View,
    inputs: &mut Inputs,
    actions: &mut Actions,
) -> f32 {
    let keys = player_strip::keys(ui, view, strip);
    let width = keys.width + 2.0 * SIDE;
    let k = width / walkman::WIDTH;
    let band = keys.height + 2.0 * MARGIN;
    let top = keys.row.center().y - band / 2.0;
    let left = strip.left();

    let reading = analyser::read();
    let dt = ui.input(|i| i.stable_dt).min(0.1);
    let moving = inputs.deck.step(view, &reading, dt);
    let tape = inputs.deck.tape(view);
    let painter = ui.painter().clone();
    let media = view.settings.deck_media;
    let (art, keys_top, bottom, spectrum_top) = match media {
        DeckMedia::Reel => {
            let plate = reel::PLATE * k;
            let canvas = Canvas {
                painter: painter.clone(),
                origin: pos2(left, top),
                k,
            };
            reel::draw(&canvas, band / k, &tape);
            let art = Rect::from_min_size(pos2(left, top), vec2(width, plate));
            (art, top + plate + MARGIN, top + plate + band, top)
        }
        DeckMedia::Cassette => {
            let body_top = keys.row.center().y;
            let bottom = top + band + WALKMAN_BODY * k;
            let canvas = Canvas {
                painter: painter.clone(),
                origin: pos2(left, body_top),
                k,
            };
            walkman::draw(&canvas, (bottom - body_top) / k, &tape);
            let art = Rect::from_min_max(pos2(left, top + band), pos2(left + width, bottom));
            (art, top + MARGIN, bottom, top + band)
        }
    };
    let other = match media {
        DeckMedia::Cassette => "Play it on reels",
        DeckMedia::Reel => "Play it on a cassette",
    };
    if ui
        .interact(art, ui.id().with("deck media"), Sense::click())
        .on_hover_text(other)
        .clicked()
    {
        actions.push(Action::SetDeckMedia(media.next()));
    }
    let row = Rect::from_min_size(pos2(left + SIDE, keys_top), vec2(keys.width, keys.height));
    player_strip::draw_keys(ui, row, &keys, view, inputs, actions);

    let zone = Rect::from_min_max(
        pos2(left + width + GAP, spectrum_top),
        pos2(strip.right(), bottom),
    );
    if zone.width() > 60.0 {
        let spectrum = Rect::from_min_max(
            zone.min,
            pos2(zone.right(), zone.bottom() - LEVELS_HEIGHT - 6.0),
        );
        let levels = Rect::from_min_max(pos2(zone.left(), zone.bottom() - LEVELS_HEIGHT), zone.max);
        draw_spectrum(&painter, spectrum, view, &inputs.deck);
        draw_levels(&painter, levels, view, &inputs.deck);
    }
    if moving {
        ui.ctx().request_repaint();
    }
    bottom
}

/// An LED's colour, lit or not, `place` of the way up its column (0–1).
fn led(place: f32, lit: bool) -> Color32 {
    let colour = if place >= 0.87 {
        RED
    } else if place >= 0.62 {
        YELLOW
    } else {
        GREEN
    };
    if lit {
        colour
    } else {
        colour.gamma_multiply(0.14)
    }
}

fn draw_spectrum(painter: &Painter, rect: Rect, view: &View, motion: &Motion) {
    let palette = view.palette;
    theme::inset(painter, rect, Color32::BLACK, &palette);
    let labels = 16.0;
    let inner = Rect::from_min_max(rect.min + vec2(10.0, 10.0), rect.max - vec2(10.0, labels));
    let column = inner.width() / BANDS as f32;
    let segment = inner.height() / SEGMENTS as f32;
    for band in 0..BANDS {
        let x = inner.left() + band as f32 * column;
        let lit = (motion.bars[band] * SEGMENTS as f32).round() as usize;
        let top = ((motion.tops[band] * SEGMENTS as f32).round() as usize).min(SEGMENTS);
        for s in 0..SEGMENTS {
            let y = inner.bottom() - (s + 1) as f32 * segment;
            let on = s < lit || (top > 0 && s + 1 == top && top > lit);
            let cell = Rect::from_min_size(
                pos2(x + column * 0.12, y + segment * 0.12),
                vec2(column * 0.76, segment * 0.76),
            );
            painter.rect_filled(cell, 0.0, led(s as f32 / SEGMENTS as f32, on));
        }
    }
    let font = FontId::monospace(8.0);
    for (hz, label) in [
        (60.0, "60"),
        (150.0, "150"),
        (400.0, "400"),
        (1000.0, "1K"),
        (2500.0, "2.5K"),
        (6000.0, "6K"),
        (15000.0, "15K"),
    ] {
        let x = inner.left() + analyser::place(hz) * column;
        painter.text(
            pos2(x, rect.bottom() - labels / 2.0 - 2.0),
            Align2::CENTER_CENTER,
            label,
            font.clone(),
            palette.chrome,
        );
    }
}

fn draw_levels(painter: &Painter, rect: Rect, view: &View, motion: &Motion) {
    let palette = view.palette;
    theme::inset(painter, rect, Color32::BLACK, &palette);
    let font = FontId::monospace(8.0);
    let left = rect.left() + 20.0;
    let width = rect.right() - 10.0 - left;
    let cell = width / LEVEL_LEDS as f32;
    for (side, name) in ["L", "R"].into_iter().enumerate() {
        let y = rect.top() + 6.0 + side as f32 * 11.0;
        painter.text(
            pos2(rect.left() + 10.0, y + 4.0),
            Align2::CENTER_CENTER,
            name,
            font.clone(),
            palette.chrome,
        );
        let lit = (motion.levels[side] * LEVEL_LEDS as f32).round() as usize;
        let top = (motion.level_tops[side] * LEVEL_LEDS as f32).round() as usize;
        for i in 0..LEVEL_LEDS {
            let db = LEVEL_FLOOR + (i + 1) as f32;
            let colour = if db > -3.0 {
                RED
            } else if db > -10.0 {
                YELLOW
            } else {
                GREEN
            };
            let on = i < lit || (top > 0 && i + 1 == top && top > lit);
            let led =
                Rect::from_min_size(pos2(left + i as f32 * cell + 1.0, y), vec2(cell - 2.5, 8.0));
            painter.rect_filled(
                led,
                0.0,
                if on {
                    colour
                } else {
                    colour.gamma_multiply(0.14)
                },
            );
        }
    }
    for db in [-40, -30, -20, -10, -6, -3, 0] {
        let x = left + (db as f32 - LEVEL_FLOOR) / -LEVEL_FLOOR * width;
        painter.text(
            pos2(x, rect.bottom() - 7.0),
            Align2::CENTER_CENTER,
            db.to_string(),
            font.clone(),
            palette.chrome,
        );
    }
}
