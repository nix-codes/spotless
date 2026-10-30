//! An '80s Walkman on its side, 336 units wide: a charcoal body with the
//! player's buttons standing up out of its top edge as its keys, and a
//! window over most of its front, the top right corner cut on the
//! diagonal, with graphics printed on the glass over the cassette, and
//! the SINGLE / MIX switch under it.

use egui::{Align2, Color32, Rect};

use super::Tape;
use super::canvas::{Canvas, Face, rgb, rgba};
use super::cassette;

pub const WIDTH: f32 = 336.0;
const BLUE: Color32 = rgb(0x3d7bff);

/// The body, `height` units tall from just under the keys' middle;
/// returns where its switch can be clicked.
pub fn draw(c: &Canvas, height: f32, tape: &Tape) -> Rect {
    let (w, h) = (WIDTH, height);
    c.shaded(
        0.0,
        0.0,
        w,
        h,
        10.0,
        &[(0.0, rgb(0x45484f)), (1.0, rgb(0x2a2c31))],
    );
    c.outline(0.0, 0.0, w, h, 10.0, 1.0, rgb(0x1a1b1f));
    // The slot the keys stand in.
    c.rect(2.0, -2.0, w - 4.0, 16.0, 4.0, rgb(0x18191c));

    // The window.
    let (x0, y0, x1, y1, cut) = (10.0, 20.0, w - 10.0, h - 40.0, 34.0);
    let window = [(x0, y0), (x1 - cut, y0), (x1, y0 + cut), (x1, y1), (x0, y1)];
    c.polygon(&window, rgb(0x0c0d10));
    let k = (y1 - y0 - 12.0) / cassette::HEIGHT;
    let inside = Canvas {
        origin: c.p(x1 - cassette::WIDTH * k - 34.0, y0 + 6.0),
        k: c.k * k,
        ..c.clipped(x0, y0, x1 - x0, y1 - y0)
    };
    cassette::draw(&inside, tape);
    c.polygon(&window, rgba(0x0a0e1a, 0.2));

    // Printed on the glass: how much tape is left…
    let glass = c.clipped(x0, y0, x1 - x0, y1 - y0);
    let ink = rgb(0xe8ecf2);
    for (i, mark) in ["100", "50", "0"].into_iter().enumerate() {
        let (xs, ys, xe) = (x0 + 60.0, y0 + 62.0 + i as f32 * 22.0, x1 - 32.0);
        let end = ys - 0.17 * (xe - xs);
        glass.line(&[(xs, ys), (xe, end)], 0.8, rgba(0xe8ecf2, 0.45));
        glass.circle(xs + (xe - xs) * 0.62, ys + (end - ys) * 0.62, 1.6, ink);
        glass.text(
            mark,
            xe + 3.0,
            end,
            Face::Mono,
            7.0,
            ink,
            Align2::LEFT_CENTER,
        );
    }
    // …a grid of orange dots fading out, and the tagline…
    for row in 0..5 {
        for col in 0..7 {
            if col + row > 8 {
                continue;
            }
            let fade = 1.0 - (col + row) as f32 * 0.08;
            let dot = Color32::from_rgba_unmultiplied(255, 90 + row * 6, 40, (fade * 255.0) as u8);
            glass.circle(
                x0 + 14.0 + col as f32 * 6.5,
                y0 + 12.0 + row as f32 * 6.5,
                2.2,
                dot,
            );
        }
    }
    glass.text(
        "It's Spotless",
        x0 + 12.0,
        y0 + 46.0,
        Face::Sans,
        8.0,
        rgb(0xff6a3a),
        Align2::LEFT_CENTER,
    );
    // …and a corner triangle saying what it is.
    let (tx, ty) = (x0 + 190.0, y1 - 80.0);
    c.polygon(&[(x0, y1), (x0, ty), (tx, y1)], rgba(0x0b1020, 0.94));
    c.polygon_outline(
        &[
            (x0 + 4.0, ty + 4.0),
            (tx - 8.0, y1 - 4.0),
            (x0 + 4.0, y1 - 4.0),
        ],
        1.5,
        BLUE,
    );
    let slant = (y1 - ty).atan2(tx - x0);
    c.text_at_angle(
        "S T E R E O   C A S S E T T E   P L A Y E R",
        x0 + 16.0 + 9.0 * slant.sin(),
        ty + 7.0 + 9.0 * slant.cos(),
        Face::Sans,
        6.0,
        rgb(0xc8ccd4),
        slant,
    );
    c.text(
        "STEREO",
        x0 + 10.0,
        y1 - 14.0,
        Face::Display,
        15.0,
        BLUE,
        Align2::LEFT_CENTER,
    );
    // Glare across the top left of the glass.
    let (dx, dy) = (0.6 * (x1 - x0), y1 - y0);
    let reach = 0.35 * (dx * dx + dy * dy);
    let top = (reach / dx).min(x1 - cut - x0);
    let bottom = ((reach - dy * dy) / dx).max(0.0);
    c.shaded_polygon(
        &[(x0, y0), (x0 + top, y0), (x0 + bottom, y1), (x0, y1)],
        |x, y| {
            let t = ((x - x0) * dx + (y - y0) * dy) / (dx * dx + dy * dy);
            rgba(0xffffff, 0.11 - 0.25 * t)
        },
    );
    // The blue line along the top and down the cut.
    c.line(
        &[
            (x0 + 70.0, y0 + 5.0),
            (x1 - cut - 2.0, y0 + 5.0),
            (x1 - 5.0, y0 + cut + 2.0),
            (x1 - 5.0, y0 + cut + 40.0),
        ],
        2.0,
        BLUE,
    );
    c.polygon_outline(&window, 1.5, rgb(0x08090b));

    // Under the window: the name, the switch, the play light and the model.
    c.outlined(
        "SPOTLESS",
        14.0,
        h - 19.0,
        Face::Display,
        26.0,
        3.0,
        rgb(0x9da1a9),
        rgb(0x1c1d21),
    );
    let (lx, ly) = (w - 82.0, h - 26.0);
    let name_end = 14.0
        + "SPOTLESS"
            .chars()
            .map(|letter| c.width(&letter.to_string(), Face::Display, 26.0) + 3.0)
            .sum::<f32>()
        - 3.0;
    let switch = slide_switch(c, (name_end + lx - 8.0) / 2.0, h - 20.0, tape.switch);
    if tape.playing {
        c.circle(lx, ly, 6.0, rgba(0xff5a3a, 0.25));
        c.circle(lx, ly, 2.5, rgb(0xff5a3a));
    } else {
        c.circle(lx, ly, 2.5, rgb(0x4a2a24));
    }
    // Auto reverse only plays a part on a mixtape.
    c.text(
        "AUTO REVERSE",
        w - 14.0,
        h - 26.0,
        Face::Mono,
        7.0,
        if tape.mixtape.is_some() {
            rgb(0x8a8e96)
        } else {
            rgb(0x4c4f57)
        },
        Align2::RIGHT_CENTER,
    );
    c.text(
        "WM-90",
        w - 14.0,
        h - 13.0,
        Face::Mono,
        10.0,
        rgb(0x9da1a9),
        Align2::RIGHT_CENTER,
    );
    switch
}

/// The SINGLE / MIX switch round `x, y`: a slot with a ribbed silver nub
/// `at` of the way from SINGLE (0) to MIX (1), the two printed above its
/// ends. Returns where it can be clicked.
fn slide_switch(c: &Canvas, x: f32, y: f32, at: f32) -> Rect {
    let (width, height, nub) = (26.0, 8.0, 12.0);
    let (sx, sy) = (x - width / 2.0, y - height / 2.0 + 4.0);
    let print = rgb(0x8a8e96);
    c.text(
        "SINGLE",
        sx + 2.0,
        y - 8.0,
        Face::Mono,
        6.0,
        print,
        Align2::CENTER_CENTER,
    );
    c.text(
        "MIX",
        sx + width - 2.0,
        y - 8.0,
        Face::Mono,
        6.0,
        print,
        Align2::CENTER_CENTER,
    );
    c.rect(sx, sy, width, height, 2.0, rgb(0x0e0f12));
    c.outline(sx, sy, width, height, 2.0, 0.6, rgb(0x56595f));
    let nx = sx + 1.0 + at * (width - nub - 2.0);
    c.shaded(
        nx,
        sy + 1.0,
        nub,
        height - 2.0,
        1.5,
        &[(0.0, rgb(0xe4e7eb)), (1.0, rgb(0x8d929a))],
    );
    for rib in 0..4 {
        let rx = nx + 3.0 + rib as f32 * 2.0;
        c.line(
            &[(rx, sy + 2.0), (rx, sy + height - 2.0)],
            0.6,
            rgb(0x5c6068),
        );
    }
    c.r(sx - 10.0, y - 13.0, width + 20.0, 22.0)
}
