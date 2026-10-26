//! Drawing the deck's devices in their own units: each is designed at a
//! fixed size and scaled to the room it gets, as a picture would be. Adds
//! the gradients, rings and rotated text egui's painter lacks.

// Shapes are placed by their coordinates and sizes, one argument each.
#![allow(clippy::too_many_arguments)]

use egui::epaint::{Mesh, PathShape, PathStroke, TextShape, Vertex, WHITE_UV};
use egui::{Align2, Color32, FontFamily, FontId, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

/// The fonts the devices are printed in, besides egui's own.
pub const MARKER: &str = "marker";
pub const DISPLAY: &str = "display";

/// Adds the marker pen and the '80s display face to egui's fonts.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    for (name, bytes) in [
        (
            MARKER,
            &include_bytes!("fonts/PermanentMarker-Regular.ttf")[..],
        ),
        (DISPLAY, &include_bytes!("fonts/Audiowide-Regular.ttf")[..]),
    ] {
        fonts.font_data.insert(
            name.into(),
            std::sync::Arc::new(egui::FontData::from_static(bytes)),
        );
        fonts
            .families
            .insert(FontFamily::Name(name.into()), vec![name.into()]);
    }
    ctx.set_fonts(fonts);
}

/// Which face a piece of text is in.
#[derive(Clone, Copy)]
pub enum Face {
    Sans,
    Mono,
    Marker,
    Display,
}

/// A painter in a device's units: `origin` is its (0, 0), `k` points per
/// unit.
#[derive(Clone)]
pub struct Canvas {
    pub painter: Painter,
    pub origin: Pos2,
    pub k: f32,
}

/// A colour from 0xRRGGBB.
pub const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// A colour from 0xRRGGBB at `alpha` (0–1).
pub fn rgba(hex: u32, alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        (hex >> 16) as u8,
        (hex >> 8) as u8,
        hex as u8,
        (alpha.clamp(0.0, 1.0) * 255.0) as u8,
    )
}

/// The colour `t` of the way through `stops`, (position, colour) pairs in
/// order.
pub fn blend(stops: &[(f32, Color32)], t: f32) -> Color32 {
    let Some(&(first, first_colour)) = stops.first() else {
        return Color32::TRANSPARENT;
    };
    if t <= first {
        return first_colour;
    }
    for pair in stops.windows(2) {
        let ((a, from), (b, to)) = (pair[0], pair[1]);
        if t <= b {
            let f = if b > a { (t - a) / (b - a) } else { 1.0 };
            return lerp(from, to, f);
        }
    }
    stops[stops.len() - 1].1
}

pub fn lerp(from: Color32, to: Color32, t: f32) -> Color32 {
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(
        mix(from.r(), to.r()),
        mix(from.g(), to.g()),
        mix(from.b(), to.b()),
        mix(from.a(), to.a()),
    )
}

impl Canvas {
    pub fn p(&self, x: f32, y: f32) -> Pos2 {
        self.origin + vec2(x, y) * self.k
    }

    pub fn r(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(self.p(x, y), vec2(w, h) * self.k)
    }

    /// The same canvas drawing only inside `x, y, w, h`.
    pub fn clipped(&self, x: f32, y: f32, w: f32, h: f32) -> Self {
        let clip = self.painter.clip_rect().intersect(self.r(x, y, w, h));
        Self {
            painter: self.painter.with_clip_rect(clip),
            ..self.clone()
        }
    }

    fn add(&self, shape: impl Into<Shape>) {
        self.painter.add(shape);
    }

    pub fn rect(&self, x: f32, y: f32, w: f32, h: f32, radius: f32, fill: Color32) {
        self.painter
            .rect_filled(self.r(x, y, w, h), radius * self.k, fill);
    }

    pub fn outline(
        &self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        width: f32,
        colour: Color32,
    ) {
        self.painter.rect_stroke(
            self.r(x, y, w, h),
            radius * self.k,
            Stroke::new(width * self.k, colour),
            egui::StrokeKind::Inside,
        );
    }

    pub fn circle(&self, x: f32, y: f32, r: f32, fill: Color32) {
        self.painter.circle_filled(self.p(x, y), r * self.k, fill);
    }

    pub fn ring(&self, x: f32, y: f32, r: f32, width: f32, colour: Color32) {
        self.painter.circle_stroke(
            self.p(x, y),
            r * self.k,
            Stroke::new(width * self.k, colour),
        );
    }

    pub fn line(&self, points: &[(f32, f32)], width: f32, colour: Color32) {
        let points = points.iter().map(|&(x, y)| self.p(x, y)).collect();
        self.add(PathShape::line(
            points,
            PathStroke::new(width * self.k, colour),
        ));
    }

    /// A convex polygon.
    pub fn polygon(&self, points: &[(f32, f32)], fill: Color32) {
        let points = points.iter().map(|&(x, y)| self.p(x, y)).collect();
        self.add(PathShape::convex_polygon(points, fill, Stroke::NONE));
    }

    pub fn polygon_outline(&self, points: &[(f32, f32)], width: f32, colour: Color32) {
        let points = points.iter().map(|&(x, y)| self.p(x, y)).collect();
        self.add(PathShape::closed_line(
            points,
            PathStroke::new(width * self.k, colour),
        ));
    }

    /// An arc of radius `r` from angle `from` to `to`, in radians.
    pub fn arc(&self, x: f32, y: f32, r: f32, from: f32, to: f32, width: f32, colour: Color32) {
        let steps = ((to - from).abs() * r / 2.0).clamp(4.0, 64.0) as usize;
        let points: Vec<(f32, f32)> = (0..=steps)
            .map(|i| {
                let a = from + (to - from) * i as f32 / steps as f32;
                (x + a.cos() * r, y + a.sin() * r)
            })
            .collect();
        self.line(&points, width, colour);
    }

    /// A ring's piece from radius `r0` to `r1` and angle `from` to `to`,
    /// each point coloured by `colour(x, y)` in this canvas's units.
    pub fn sector(
        &self,
        x: f32,
        y: f32,
        r0: f32,
        r1: f32,
        from: f32,
        to: f32,
        colour: impl Fn(f32, f32) -> Color32,
    ) {
        let steps = ((to - from).abs() * r1 / 3.0).clamp(6.0, 96.0) as usize;
        let mut mesh = Mesh::default();
        for i in 0..=steps {
            let a = from + (to - from) * i as f32 / steps as f32;
            for r in [r0, r1] {
                let (px, py) = (x + a.cos() * r, y + a.sin() * r);
                mesh.vertices.push(Vertex {
                    pos: self.p(px, py),
                    uv: WHITE_UV,
                    color: colour(px, py),
                });
            }
            if i > 0 {
                let at = (i as u32) * 2;
                mesh.add_triangle(at - 2, at - 1, at);
                mesh.add_triangle(at - 1, at + 1, at);
            }
        }
        self.add(Shape::mesh(mesh));
    }

    /// A rounded rectangle shaded top to bottom through `stops`.
    pub fn shaded(&self, x: f32, y: f32, w: f32, h: f32, radius: f32, stops: &[(f32, Color32)]) {
        self.shaded_by(x, y, w, h, radius, |_, v| blend(stops, v));
    }

    /// A rounded rectangle, each point coloured by `colour(u, v)`, its
    /// place across and down from 0 to 1. Built from rows, so a colour
    /// that changes downwards is exact.
    pub fn shaded_by(
        &self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        colour: impl Fn(f32, f32) -> Color32,
    ) {
        let radius = radius.min(w / 2.0).min(h / 2.0);
        // Rows at every few units, and more round the corners.
        let mut ys: Vec<f32> = Vec::new();
        let corner_steps = 6;
        for i in 0..=corner_steps {
            let a = i as f32 / corner_steps as f32;
            ys.push(radius * (1.0 - (a * std::f32::consts::FRAC_PI_2).cos()));
        }
        let mut at = radius;
        while at < h - radius {
            ys.push(at);
            at += 6.0;
        }
        for i in (0..=corner_steps).rev() {
            let a = i as f32 / corner_steps as f32;
            ys.push(h - radius * (1.0 - (a * std::f32::consts::FRAC_PI_2).cos()));
        }
        ys.dedup_by(|a, b| (*a - *b).abs() < 0.01);
        let inset = |dy: f32| {
            let from_edge = if dy < radius {
                radius - dy
            } else if dy > h - radius {
                dy - (h - radius)
            } else {
                0.0
            };
            radius - (radius * radius - from_edge * from_edge).max(0.0).sqrt()
        };
        let mut mesh = Mesh::default();
        for (row, &dy) in ys.iter().enumerate() {
            let side = inset(dy);
            for &dx in &[side, w - side] {
                mesh.vertices.push(Vertex {
                    pos: self.p(x + dx, y + dy),
                    uv: WHITE_UV,
                    color: colour(dx / w, dy / h),
                });
            }
            if row > 0 {
                let at = (row as u32) * 2;
                mesh.add_triangle(at - 2, at - 1, at);
                mesh.add_triangle(at - 1, at + 1, at);
            }
        }
        self.add(Shape::mesh(mesh));
    }

    /// A convex polygon, each corner coloured by `colour(x, y)`.
    pub fn shaded_polygon(&self, points: &[(f32, f32)], colour: impl Fn(f32, f32) -> Color32) {
        let mut mesh = Mesh::default();
        for &(x, y) in points {
            mesh.vertices.push(Vertex {
                pos: self.p(x, y),
                uv: WHITE_UV,
                color: colour(x, y),
            });
        }
        for i in 1..points.len().saturating_sub(1) as u32 {
            mesh.add_triangle(0, i, i + 1);
        }
        self.add(Shape::mesh(mesh));
    }

    pub fn font(&self, face: Face, size: f32) -> FontId {
        let family = match face {
            Face::Sans => FontFamily::Proportional,
            Face::Mono => FontFamily::Monospace,
            Face::Marker => FontFamily::Name(MARKER.into()),
            Face::Display => FontFamily::Name(DISPLAY.into()),
        };
        // Fonts are bound a frame after they're installed.
        let bound = self
            .painter
            .ctx()
            .fonts(|fonts| fonts.families().contains(&family));
        FontId::new(
            size * self.k,
            if bound {
                family
            } else {
                FontFamily::Proportional
            },
        )
    }

    /// Text placed by `align` at `x, y`.
    pub fn text(
        &self,
        text: &str,
        x: f32,
        y: f32,
        face: Face,
        size: f32,
        colour: Color32,
        align: Align2,
    ) -> Rect {
        self.painter
            .text(self.p(x, y), align, text, self.font(face, size), colour)
    }

    /// Text that starts at `x, y` (its left middle) and runs at `angle`.
    pub fn text_at_angle(
        &self,
        text: &str,
        x: f32,
        y: f32,
        face: Face,
        size: f32,
        colour: Color32,
        angle: f32,
    ) {
        let galley = self
            .painter
            .layout_no_wrap(text.into(), self.font(face, size), colour);
        let half = galley.size().y / 2.0;
        // Rotated round its top left: start half a line above the middle.
        let at = self.p(x, y) + egui::Vec2::angled(angle - std::f32::consts::FRAC_PI_2) * half;
        self.add(TextShape::new(at, galley, colour).with_angle(angle));
    }

    /// Text centred on `x, y`, running at `angle`, shrunk to fit `width`.
    pub fn text_centred_at_angle(
        &self,
        text: &str,
        x: f32,
        y: f32,
        face: Face,
        size: f32,
        colour: Color32,
        angle: f32,
        width: f32,
    ) {
        let natural = self
            .painter
            .layout_no_wrap(text.into(), self.font(face, size), colour)
            .size()
            .x
            / self.k;
        let size = if natural > width {
            size * width / natural
        } else {
            size
        };
        let half = natural.min(width) / 2.0;
        let (start_x, start_y) = (x - angle.cos() * half, y - angle.sin() * half);
        self.text_at_angle(text, start_x, start_y, face, size, colour, angle);
    }

    /// Text with a `edge`-coloured outline round its letters.
    pub fn outlined(
        &self,
        text: &str,
        x: f32,
        y: f32,
        face: Face,
        size: f32,
        spacing: f32,
        fill: Color32,
        edge: Color32,
    ) {
        for (dx, dy) in [
            (-1.0, 0.0),
            (1.0, 0.0),
            (0.0, -1.0),
            (0.0, 1.0),
            (-0.7, -0.7),
            (0.7, -0.7),
            (-0.7, 0.7),
            (0.7, 0.7),
        ] {
            self.spaced(
                text,
                x + dx,
                y + dy,
                face,
                size,
                spacing,
                edge,
                Align2::LEFT_CENTER,
            );
        }
        self.spaced(text, x, y, face, size, spacing, fill, Align2::LEFT_CENTER);
    }

    /// Text spaced out by `spacing` units between letters.
    pub fn spaced(
        &self,
        text: &str,
        x: f32,
        y: f32,
        face: Face,
        size: f32,
        spacing: f32,
        colour: Color32,
        align: Align2,
    ) {
        let font = self.font(face, size);
        let widths: Vec<f32> = text
            .chars()
            .map(|c| {
                self.painter
                    .layout_no_wrap(c.to_string(), font.clone(), colour)
                    .size()
                    .x
            })
            .collect();
        let total = widths.iter().sum::<f32>() + spacing * self.k * (widths.len() as f32 - 1.0);
        let start = self.p(x, y).x - total * align.x().to_factor();
        let mut at = start;
        for (c, width) in text.chars().zip(widths) {
            self.painter.text(
                pos2(at, self.p(x, y).y),
                Align2::LEFT_CENTER,
                c,
                font.clone(),
                colour,
            );
            at += width + spacing * self.k;
        }
    }
}
