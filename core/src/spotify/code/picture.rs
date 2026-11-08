//! Finding a Spotify Code in a picture: its logo, its 23 bars after it,
//! and how tall each is, in a screenshot or in a photo of one.

use image::GrayImage;

use super::bars::{decode, decode_doubtful};

/// A shape in the picture: its bounding box, and its pixels' count and
/// sums (for where its middle is).
#[derive(Clone, Copy, Debug)]
struct Shape {
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
    pixels: u32,
    sum_x: u64,
    sum_y: u64,
}

impl Shape {
    fn width(&self) -> f32 {
        (self.right - self.left + 1) as f32
    }
    fn height(&self) -> f32 {
        (self.bottom - self.top + 1) as f32
    }
    fn middle(&self) -> f32 {
        (self.top + self.bottom) as f32 / 2.0
    }
    /// Its centre of mass.
    fn centre(&self) -> (f32, f32) {
        let n = self.pixels as f32;
        (self.sum_x as f32 / n, self.sum_y as f32 / n)
    }
}

/// The reference of a code anywhere in the picture: a round logo with 23
/// bars after it, light on dark or dark on light. A screenshot's code is
/// level and evenly lit; a photo's may be at an angle, foreshortened and
/// lit unevenly, which the second way allows for.
pub(super) fn find(gray: &GrayImage) -> Option<u64> {
    let threshold = otsu(gray);
    let level = [true, false].into_iter().find_map(|light| {
        let (shapes, _) = shapes(gray, |luma| (luma > threshold) == light);
        shapes.iter().find_map(|logo| code_after(logo, &shapes))
    });
    level.or_else(|| find_in_photo(gray))
}

/// A code at any angle: each pixel is ink by how it compares with those
/// around it, so a shadow or a glare doesn't swallow the bars.
fn find_in_photo(gray: &GrayImage) -> Option<u64> {
    let around = local_mean(gray);
    let raw = gray.as_raw();
    // Ink differs from its surroundings by this much at least; failing
    // that, by more, which parts bars that blur has run together (where
    // they're found, not how long they are: that's measured on the grey).
    let tries = [12i16, 40].into_iter().flat_map(|contrast| [(true, contrast), (false, contrast)]);
    tries.into_iter().find_map(|(light, contrast)| {
        let (shapes, _) = shapes_at(gray.dimensions(), |at| {
            let difference = raw[at] as i16 - around[at] as i16;
            if light { difference > contrast } else { difference < -contrast }
        });
        // What might be a bar, and the largest round shapes as the logo
        // (a code fills a fair part of the picture).
        let pieces: Vec<usize> = (0..shapes.len()).filter(|i| shapes[*i].pixels >= 3).collect();
        let mut logos: Vec<usize> = pieces.iter().copied().filter(|i| logo_like(&shapes[*i])).collect();
        logos.sort_by_key(|i| std::cmp::Reverse(shapes[*i].pixels));
        logos.truncate(30);
        let photo = Photo {
            gray,
            light,
            shapes: &shapes,
            pieces: &pieces,
        };
        logos.into_iter().find_map(|logo| photo.code_from(logo))
    })
}

/// A picture, and the shapes found in it.
struct Photo<'a> {
    gray: &'a GrayImage,
    /// The code is light on dark.
    light: bool,
    shapes: &'a [Shape],
    /// The shapes large enough to be a bar, by index.
    pieces: &'a [usize],
}

impl Photo<'_> {
    /// The code whose logo shape `logo` would be, its bars in a row
    /// leading away from it in any direction.
    fn code_from(&self, logo: usize) -> Option<u64> {
        let shape = &self.shapes[logo];
        let size = shape.width().max(shape.height());
        let (cx, cy) = shape.centre();
        // What might be a bar: no larger than the logo, not too far.
        let near: Vec<(usize, f32, f32)> = (self.pieces.iter())
            .map(|i| (*i, &self.shapes[*i]))
            .filter(|(i, s)| *i != logo && s.width().max(s.height()) <= size * 1.3)
            .filter_map(|(i, s)| {
                let (x, y) = s.centre();
                let distance = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
                (distance > size * 0.5 && distance < size * 10.0).then_some((i, x - cx, y - cy))
            })
            .collect();
        if near.len() < 23 {
            return None;
        }
        // The row's direction: towards one of them, the nearest first.
        let mut seeds = near.clone();
        seeds.sort_by(|a, b| (a.1.hypot(a.2)).total_cmp(&b.1.hypot(b.2)));
        let mut tried: Vec<f32> = Vec::new();
        for (_, dx, dy) in seeds.into_iter().take(12) {
            let angle = dy.atan2(dx);
            if tried.iter().any(|t| angle_between(*t, angle) < 0.03) {
                continue;
            }
            tried.push(angle);
            if let Some(reference) = self.row(&near, angle, size) {
                return Some(reference);
            }
        }
        None
    }

    /// The code along `angle` from the logo, if 23 evenly spaced shapes lie
    /// on that line.
    fn row(&self, near: &[(usize, f32, f32)], angle: f32, size: f32) -> Option<u64> {
        let (cos, sin) = (angle.cos(), angle.sin());
        let mut on: Vec<(usize, f32)> = (near.iter())
            .filter_map(|(i, dx, dy)| {
                let along = dx * cos + dy * sin;
                let off = -dx * sin + dy * cos;
                (along > 0.0 && off.abs() <= size * 0.2).then_some((*i, along))
            })
            .collect();
        if on.len() < 23 {
            return None;
        }
        on.sort_by(|a, b| a.1.total_cmp(&b.1));
        let bars = &on[..23];
        let step = (bars[22].1 - bars[0].1) / 22.0;
        let even = bars.windows(2).all(|pair| {
            let gap = pair[1].1 - pair[0].1;
            gap > step * 0.6 && gap < step * 1.4
        });
        if step < 2.0 || !even || bars[0].1 > size * 0.5 + step * 2.5 {
            return None;
        }
        // Each bar's length across the row, by how much ink lies on its
        // middle line.
        let tallest = self.profile(bars[11].0, cos, sin, size);
        let ink = tallest.iter().copied().fold(f32::MIN, f32::max);
        // Each bar's ink, over a band as wide as the gaps beside it.
        let lengths: Vec<f32> = (0..23)
            .map(|k| {
                let before = if k > 0 { bars[k].1 - bars[k - 1].1 } else { bars[1].1 - bars[0].1 };
                let after = if k < 22 { bars[k + 1].1 - bars[k].1 } else { before };
                self.ink_in_band(bars[k].0, cos, sin, size, ink, (before + after) / 4.0)
            })
            .collect();
        heights_of(&lengths).and_then(|(first, fitted)| {
            decode_doubtful(&fitted).or_else(|| decode_doubtful(&first))
        })
    }

    /// How inky the picture is along shape `i`'s middle line across the
    /// row, a sample each half pixel, from the paper's level (0) up: as far
    /// as the logo is wide each way, past the longest bar's ends.
    fn profile(&self, i: usize, cos: f32, sin: f32, size: f32) -> Vec<f32> {
        self.profile_at(i, cos, sin, size, 0.0)
    }

    /// The same along a line `u` from the middle, along the row.
    fn profile_at(&self, i: usize, cos: f32, sin: f32, size: f32, u: f32) -> Vec<f32> {
        let (cx, cy) = self.shapes[i].centre();
        let (cx, cy) = (cx + u * cos, cy + u * sin);
        let reach = size * 0.8;
        let samples: Vec<f32> = (0..=(reach * 4.0) as i32)
            .map(|k| {
                let t = k as f32 / 2.0 - reach;
                let grey = self.grey_at(cx - t * sin, cy + t * cos);
                if self.light { grey } else { 255.0 - grey }
            })
            .collect();
        // The paper: the quarter of the samples farthest out.
        let ends = samples.len() / 8;
        let mut outside: Vec<f32> = (samples[..ends].iter())
            .chain(&samples[samples.len() - ends..])
            .copied()
            .collect();
        outside.sort_by(f32::total_cmp);
        let paper = outside.get(outside.len() / 2).copied().unwrap_or(0.0);
        samples.iter().map(|v| v - paper).collect()
    }

    /// Shape `i`'s ink, as a share of `ink` (a full bar's), over a band
    /// `half` either side of its middle along the row: blur spreads ink
    /// but doesn't change how much there is.
    fn ink_in_band(&self, i: usize, cos: f32, sin: f32, size: f32, ink: f32, half: f32) -> f32 {
        let steps = (half * 2.0).round().max(1.0) as i32;
        (-steps..=steps)
            .map(|k| {
                let u = k as f32 / 2.0;
                let profile = self.profile_at(i, cos, sin, size, u);
                profile.iter().map(|v| (v / ink.max(1.0)).clamp(0.0, 1.0)).sum::<f32>() / 2.0
            })
            .sum::<f32>()
            / 2.0
    }

    /// The grey at a point between pixels, from the four around it.
    fn grey_at(&self, x: f32, y: f32) -> f32 {
        let (w, h) = (self.gray.width() as i64, self.gray.height() as i64);
        let (x0, y0) = (x.floor() as i64, y.floor() as i64);
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let at = |x: i64, y: i64| self.gray.as_raw()[(y.clamp(0, h - 1) * w + x.clamp(0, w - 1)) as usize] as f32;
        let top = at(x0, y0) * (1.0 - fx) + at(x0 + 1, y0) * fx;
        let bottom = at(x0, y0 + 1) * (1.0 - fx) + at(x0 + 1, y0 + 1) * fx;
        top * (1.0 - fy) + bottom * fy
    }
}

/// The heights 23 measured lengths stand for, as fractions: a first
/// guess, and one fitted to them all. Farther away, a bar looks smaller:
/// the first and last bars, both of height 0, say by how much at each end,
/// and the scale is taken to change evenly between them. The fit then finds
/// length = scale × (base + height × step), the scale changing evenly too,
/// that best matches every bar with its height first guessed.
fn heights_of(lengths: &[f32]) -> Option<([f32; 23], [f32; 23])> {
    let ends = (lengths[0] + lengths[22]) / 2.0;
    if ends <= 0.0 {
        return None;
    }
    let scale = |i: usize| {
        let t = i as f32 / 22.0;
        (lengths[0] * (1.0 - t) + lengths[22] * t) / ends
    };
    let high = lengths[11] / scale(11);
    if high <= ends * 2.0 {
        return None;
    }
    let first: [f32; 23] = std::array::from_fn(|i| (lengths[i] / scale(i) - ends) / (high - ends) * 7.0);
    // length = A + B·h + C·t + D·t·h, with t the bar's place from the
    // middle: linear in A to D, so least squares finds them.
    let mut fitted = first;
    for _ in 0..2 {
        let rows: Vec<([f64; 4], f64)> = (0..23)
            .map(|i| {
                let h = fitted[i].round().clamp(0.0, 7.0) as f64;
                let t = i as f64 - 11.0;
                ([1.0, h, t, t * h], lengths[i] as f64)
            })
            .collect();
        let [a, b, c, d] = least_squares(&rows)?;
        fitted = std::array::from_fn(|i| {
            let t = i as f64 - 11.0;
            ((lengths[i] as f64 - a - c * t) / (b + d * t)) as f32
        });
    }
    Some((first, fitted))
}

/// The values that best fit these rows (the unknowns' factors, and what
/// they should add up to), by least squares.
fn least_squares<const N: usize>(rows: &[([f64; N], f64)]) -> Option<[f64; N]> {
    let mut m = [[0f64; N]; N];
    let mut v = [0f64; N];
    for (row, y) in rows {
        for r in 0..N {
            for c in 0..N {
                m[r][c] += row[r] * row[c];
            }
            v[r] += row[r] * y;
        }
    }
    for col in 0..N {
        let pivot = (col..N).max_by(|a, b| m[*a][col].abs().total_cmp(&m[*b][col].abs()))?;
        m.swap(col, pivot);
        v.swap(col, pivot);
        if m[col][col].abs() < 1e-9 {
            return None;
        }
        let (pivot_row, pivot_value) = (m[col], v[col]);
        for r in (0..N).filter(|r| *r != col) {
            let f = m[r][col] / pivot_row[col];
            for (cell, p) in m[r].iter_mut().zip(pivot_row).skip(col) {
                *cell -= f * p;
            }
            v[r] -= f * pivot_value;
        }
    }
    Some(std::array::from_fn(|i| v[i] / m[i][i]))
}

/// Round at any angle, as the logo is: about as wide as high, and mostly
/// filled (its waves cut into it).
fn logo_like(shape: &Shape) -> bool {
    let size = shape.width().max(shape.height());
    let fill = shape.pixels as f32 / (shape.width() * shape.height());
    size >= 10.0 && (shape.width() - shape.height()).abs() <= size * 0.25 && (0.35..=0.85).contains(&fill)
}

/// How far apart two directions are, in radians.
fn angle_between(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(std::f32::consts::TAU);
    d.min(std::f32::consts::TAU - d)
}

/// Each pixel's surroundings' mean grey: a square about a sixteenth of
/// the picture across, from a summed table.
fn local_mean(gray: &GrayImage) -> Vec<u8> {
    let (width, height) = (gray.width() as usize, gray.height() as usize);
    let raw = gray.as_raw();
    let mut sums = vec![0u64; (width + 1) * (height + 1)];
    for y in 0..height {
        let mut row = 0u64;
        for x in 0..width {
            row += raw[y * width + x] as u64;
            sums[(y + 1) * (width + 1) + x + 1] = sums[y * (width + 1) + x + 1] + row;
        }
    }
    let r = (width.min(height) / 16).max(8);
    let mut mean = vec![0u8; width * height];
    for y in 0..height {
        let (top, bottom) = (y.saturating_sub(r), (y + r + 1).min(height));
        for x in 0..width {
            let (left, right) = (x.saturating_sub(r), (x + r + 1).min(width));
            let total = sums[bottom * (width + 1) + right] + sums[top * (width + 1) + left]
                - sums[top * (width + 1) + right]
                - sums[bottom * (width + 1) + left];
            let count = ((bottom - top) * (right - left)) as u64;
            mean[y * width + x] = (total / count) as u8;
        }
    }
    mean
}

/// The code whose logo `logo` would be, if the 23 shapes after it are its
/// bars.
fn code_after(logo: &Shape, shapes: &[Shape]) -> Option<u64> {
    let size = logo.width().max(logo.height());
    let round = (logo.width() - logo.height()).abs() <= size * 0.15;
    if !round || size < 8.0 {
        return None;
    }
    let mut bars: Vec<&Shape> = (shapes.iter())
        .filter(|s| s.left > logo.right && (s.left - logo.right) as f32 <= size * 14.0)
        .filter(|s| (s.middle() - logo.middle()).abs() <= size * 0.15)
        .filter(|s| s.height() <= size * 1.2 && s.width() <= size * 0.35)
        .collect();
    bars.sort_by_key(|s| s.left);
    let bars = bars.get(..23)?;
    // The first bar is near the logo, the bars evenly spaced.
    let step = (bars[22].left - bars[0].left) as f32 / 22.0;
    if (bars[0].left - logo.right) as f32 > step * 2.5 {
        return None;
    }
    let low = (bars[0].height() + bars[22].height()) / 2.0;
    let high = bars[11].height();
    if high <= low * 2.0 {
        return None;
    }
    let heights: [u8; 23] = std::array::from_fn(|i| {
        ((bars[i].height() - low) / (high - low) * 7.0)
            .round()
            .clamp(0.0, 7.0) as u8
    });
    decode(&heights)
}

/// The shapes of the pixels `ink` picks by their grey, joined where they
/// touch (up, down, left, right), and which each pixel belongs to.
fn shapes(gray: &GrayImage, ink: impl Fn(u8) -> bool) -> (Vec<Shape>, Vec<u32>) {
    let raw = gray.as_raw();
    shapes_at(gray.dimensions(), |at| ink(raw[at]))
}

/// The same, `ink` picking pixels by where they are (their index).
fn shapes_at((width, height): (u32, u32), ink: impl Fn(usize) -> bool) -> (Vec<Shape>, Vec<u32>) {
    // Each pixel's shape, its index + 1; 0 for none.
    let mut labels = vec![0u32; (width * height) as usize];
    let mut found = Vec::new();
    let mut stack = Vec::new();
    for start in 0..(width * height) {
        if labels[start as usize] != 0 || !ink(start as usize) {
            continue;
        }
        let label = found.len() as u32 + 1;
        labels[start as usize] = label;
        stack.push(start);
        let mut shape = Shape {
            left: u32::MAX,
            top: u32::MAX,
            right: 0,
            bottom: 0,
            pixels: 0,
            sum_x: 0,
            sum_y: 0,
        };
        while let Some(at) = stack.pop() {
            let (x, y) = (at % width, at / width);
            shape.left = shape.left.min(x);
            shape.right = shape.right.max(x);
            shape.top = shape.top.min(y);
            shape.bottom = shape.bottom.max(y);
            shape.pixels += 1;
            shape.sum_x += x as u64;
            shape.sum_y += y as u64;
            let next = [
                (x > 0).then(|| at - 1),
                (x + 1 < width).then(|| at + 1),
                (y > 0).then(|| at - width),
                (y + 1 < height).then(|| at + width),
            ];
            for n in next.into_iter().flatten() {
                if labels[n as usize] == 0 && ink(n as usize) {
                    labels[n as usize] = label;
                    stack.push(n);
                }
            }
        }
        found.push(shape);
    }
    (found, labels)
}

/// The grey that best splits the picture in two (Otsu's method).
fn otsu(gray: &GrayImage) -> u8 {
    let mut histogram = [0u64; 256];
    for luma in gray.as_raw() {
        histogram[*luma as usize] += 1;
    }
    let total = gray.as_raw().len() as f64;
    let sum: f64 = (0..256).map(|i| i as f64 * histogram[i] as f64).sum();
    let (mut below, mut below_sum, mut best, mut threshold) = (0.0, 0.0, 0.0, 127);
    for (i, count) in histogram.iter().enumerate() {
        below += *count as f64;
        if below == 0.0 || below == total {
            continue;
        }
        below_sum += i as f64 * *count as f64;
        let mean_below = below_sum / below;
        let mean_above = (sum - below_sum) / (total - below);
        let between = below * (total - below) * (mean_below - mean_above).powi(2);
        if between > best {
            best = between;
            threshold = i as u8;
        }
    }
    threshold
}

#[cfg(test)]
mod tests {
    use super::super::bars::encode;
    use super::*;

    /// Boone's example: "Today's Top Hits".
    const REFERENCE: u64 = 57268659651;

    /// A code drawn as Spotify draws one: a round logo, then 23 rounded
    /// bars, `ink` on `paper`, in a picture `margin` pixels larger.
    fn drawn(reference: u64, ink: u8, paper: u8, margin: u32) -> GrayImage {
        let (unit, gap, logo) = (8u32, 6u32, 50u32);
        let width = 2 * margin + logo + 20 + 23 * (unit + gap);
        let height = 2 * margin + logo;
        let mut picture = GrayImage::from_pixel(width, height, image::Luma([paper]));
        let middle = (margin + logo / 2) as f32;
        let r = logo as f32 / 2.0;
        for y in 0..height {
            for x in 0..width {
                let (dx, dy) = (x as f32 - (margin as f32 + r), y as f32 - middle);
                // The logo, with a wave cut through it.
                let in_logo = dx * dx + dy * dy <= r * r && !(dy.abs() < 2.0 && dx.abs() < r * 0.6);
                if in_logo {
                    picture.put_pixel(x, y, image::Luma([ink]));
                }
            }
        }
        for (i, level) in encode(reference).into_iter().enumerate() {
            let half = (unit as f32 + level as f32 * 5.0) / 2.0;
            let left = margin + logo + 20 + i as u32 * (unit + gap);
            for y in 0..height {
                if (y as f32 - middle).abs() > half {
                    continue;
                }
                for x in left..left + unit {
                    picture.put_pixel(x, y, image::Luma([ink]));
                }
            }
        }
        picture
    }

    #[test]
    fn a_code_is_found_dark_on_light_and_light_on_dark() {
        assert_eq!(find(&drawn(REFERENCE, 0, 255, 10)), Some(REFERENCE));
        assert_eq!(find(&drawn(16862907074, 255, 0, 10)), Some(16862907074));
    }

    #[test]
    fn a_code_is_found_among_other_things() {
        let mut picture = drawn(REFERENCE, 20, 230, 60);
        // Something else above it, as in a screenshot.
        for x in 5..picture.width() - 5 {
            for y in 5..20 {
                picture.put_pixel(x, y, image::Luma([20]));
            }
        }
        assert_eq!(find(&picture), Some(REFERENCE));
    }

    /// `picture` as a phone's camera might see it: turned by `angle`
    /// (degrees), shrunk to `zoom`, the far end `keystone` smaller, blurred,
    /// lit from one side (`shade`: how much darker the other is) and grainy.
    fn photographed(
        picture: &GrayImage,
        angle: f32,
        zoom: f32,
        keystone: f32,
        blur: f32,
        shade: f32,
        noise: u8,
    ) -> GrayImage {
        let (w, h) = picture.dimensions();
        let side = (w as f32 * zoom * 1.2) as u32;
        let paper = picture.get_pixel(0, 0)[0];
        let (cos, sin) = (angle.to_radians().cos(), angle.to_radians().sin());
        let centre = side as f32 / 2.0;
        // Each pixel the average of the light over it, as a sensor's is.
        let sample = |x: f32, y: f32| {
            let (px, py) = (x - centre, y - centre);
            let (qx, qy) = (px * cos + py * sin, -px * sin + py * cos);
            let scale = zoom * (1.0 + keystone * qx / (w as f32 * zoom));
            let (ux, uy) = (qx / scale + w as f32 / 2.0, qy / scale + h as f32 / 2.0);
            let inside = ux >= 0.0 && uy >= 0.0 && (ux as u32) < w && (uy as u32) < h;
            (if inside { picture.get_pixel(ux as u32, uy as u32)[0] } else { paper }) as f32
        };
        let mut out = GrayImage::from_fn(side, side, |x, y| {
            let total: f32 = (0..16)
                .map(|k| sample(x as f32 + (k % 4) as f32 / 4.0, y as f32 + (k / 4) as f32 / 4.0))
                .sum();
            image::Luma([(total / 16.0) as u8])
        });
        if blur > 0.0 {
            out = image::imageops::blur(&out, blur);
        }
        let mut seed = 12345u32;
        for (x, _, pixel) in out.enumerate_pixels_mut() {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let grain = (seed >> 16) as i32 % (2 * noise as i32 + 1) - noise as i32;
            let light = 1.0 - shade * x as f32 / side as f32;
            pixel[0] = (pixel[0] as f32 * light + grain as f32).clamp(0.0, 255.0) as u8;
        }
        out
    }

    #[test]
    fn a_photographed_code_is_found_at_an_angle_and_in_poor_light() {
        let shots = [
            // angle, zoom, keystone, blur, shade, noise
            (0.0, 0.6, 0.0, 0.8, 0.0, 6),
            (12.0, 0.6, 0.25, 1.0, 0.4, 8),
            (-25.0, 0.55, -0.2, 1.2, 0.5, 10),
            (90.0, 0.7, 0.2, 1.0, 0.3, 8),
            (178.0, 0.6, 0.15, 1.0, 0.6, 8),
        ];
        for (light, reference) in [(false, REFERENCE), (true, 16862907074)] {
            let (ink, paper) = if light { (240, 30) } else { (25, 235) };
            let code = drawn(reference, ink, paper, 30);
            for (angle, zoom, keystone, blur, shade, noise) in shots {
                let photo = photographed(&code, angle, zoom, keystone, blur, shade, noise);
                assert_eq!(
                    find(&photo),
                    Some(reference),
                    "{} at {angle}°, zoom {zoom}, keystone {keystone}, blur {blur}, shade {shade}",
                    if light { "light on dark" } else { "dark on light" }
                );
            }
        }
    }

    #[test]
    fn grain_and_shapes_that_arent_a_code_give_nothing() {
        let mut seed = 99u32;
        let noise = GrayImage::from_fn(320, 240, |_, _| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            image::Luma([(seed >> 16) as u8])
        });
        assert_eq!(find(&noise), None);
        // A logo and bars, but not a code's bars.
        let mut fake = drawn(REFERENCE, 25, 235, 30);
        for x in 120..135 {
            for y in 0..fake.height() {
                fake.put_pixel(x, y, image::Luma([235]));
            }
        }
        assert_eq!(find(&photographed(&fake, 10.0, 0.6, 0.1, 1.0, 0.2, 6)), None);
    }
}
