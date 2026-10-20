//! Reading `skin.toml`: the skin's name and scale, its colours, fonts and
//! pieces, with each image checked against what it points into.
//!
//! Only an unreadable manifest, or one for another format, stops a skin.
//! Anything else wrong drops that one part, with a problem saying what and
//! where, and an unknown key is a warning, so a later format can add keys
//! without breaking this Spotless.

use std::collections::HashMap;

use egui::Color32;
use toml::{Table, Value};

use super::source::Source;
use super::{
    Area, FORMAT, Fill, Filter, Font, ImageRef, Kind, Levels, LoadError, MANIFEST, PIECES, Piece,
    Problem, Role, Shape, Skin, State, States,
};
use crate::skin::{Bitmap, hex};
use crate::ui::theme::Palette;

/// The largest image side Spotless takes, in pixels.
const MAX_SIDE: u32 = 8192;
const MAX_SCALE: f64 = 8.0;
/// A font file's size when the skin gives none, in points.
const FONT_SIZE: f64 = 12.0;

/// The colours a skin can set, by their manifest names.
/// Where in the palette a colour goes.
type Field = fn(&mut Palette) -> &mut Color32;

const COLORS: [(&str, Field); 19] = [
    ("desk", |p| &mut p.desk),
    ("window", |p| &mut p.frame),
    ("edge_light", |p| &mut p.hi),
    ("edge_dark", |p| &mut p.lo),
    ("chrome_text", |p| &mut p.chrome),
    ("button", |p| &mut p.button),
    ("lcd_bg", |p| &mut p.lcd_bg),
    ("lcd_text", |p| &mut p.lcd),
    ("list_bg", |p| &mut p.list_bg),
    ("list_text", |p| &mut p.list_fg),
    ("list_current", |p| &mut p.current),
    ("list_selected_bg", |p| &mut p.selected_bg),
    ("list_dim", |p| &mut p.dim),
    ("accent", |p| &mut p.accent),
    ("scroll_thumb", |p| &mut p.thumb),
    ("title_text", |p| &mut p.title),
    ("button_text", |p| &mut p.button_text),
    ("button_light", |p| &mut p.button_hi),
    ("button_dark", |p| &mut p.button_lo),
];

/// Groups of pieces under one name: `[pieces.seek.track]`.
const GROUPS: [&str; 4] = ["status", "seek", "volume", "scrollbar"];

pub fn read(source: &Source, fallback_name: &str) -> Result<Skin, LoadError> {
    let bytes = source.read(MANIFEST).map_err(|_| LoadError::NoManifest)?;
    let text =
        String::from_utf8(bytes).map_err(|_| LoadError::Manifest("it isn't UTF-8 text".into()))?;
    let table: Table = text
        .parse()
        .map_err(|error: toml::de::Error| LoadError::Manifest(error.to_string()))?;
    match table.get("format") {
        Some(Value::Integer(format)) if *format > FORMAT => {
            return Err(LoadError::NewerFormat(*format));
        }
        Some(Value::Integer(FORMAT)) => {}
        _ => return Err(LoadError::NoFormat),
    }

    let mut reader = Reader {
        source,
        bitmaps: HashMap::new(),
        problems: Vec::new(),
    };
    reader.unknown_keys(
        "",
        &table,
        &[
            "format", "name", "author", "scale", "filter", "colors", "fonts", "pieces",
        ],
    );
    let name = match table.get("name") {
        Some(Value::String(name)) if !name.trim().is_empty() => name.trim().to_string(),
        None => fallback_name.to_string(),
        Some(_) => {
            reader.problem("name", "should be some text");
            fallback_name.to_string()
        }
    };
    let author = match table.get("author") {
        Some(Value::String(author)) => Some(author.trim().to_string()),
        None => None,
        Some(_) => {
            reader.problem("author", "should be some text");
            None
        }
    };
    let scale = match table.get("scale").and_then(number) {
        Some(scale) if scale > 0.0 && scale <= MAX_SCALE => scale as f32,
        None if !table.contains_key("scale") => 1.0,
        _ => {
            reader.problem(
                "scale",
                &format!("should be a number above 0, up to {MAX_SCALE}"),
            );
            1.0
        }
    };
    let filter = match table.get("filter") {
        None => Filter::Pixel,
        Some(Value::String(filter)) if filter == "pixel" => Filter::Pixel,
        Some(Value::String(filter)) if filter == "smooth" => Filter::Smooth,
        Some(_) => {
            reader.problem("filter", "should be \"pixel\" or \"smooth\"");
            Filter::Pixel
        }
    };
    let palette = reader.colors(table.get("colors"));
    let fonts = reader.fonts(table.get("fonts"));
    let mut pieces = HashMap::new();
    match table.get("pieces") {
        Some(Value::Table(table)) => reader.pieces("", table, &mut pieces),
        Some(_) => reader.problem("pieces", "should be a table"),
        None => {}
    }
    let images = reader
        .bitmaps
        .into_iter()
        .filter_map(|(path, bitmap)| Some((path, bitmap.ok()?)))
        .collect();
    Ok(Skin {
        name,
        author,
        scale,
        filter,
        palette,
        fonts,
        pieces,
        images,
        problems: reader.problems,
    })
}

struct Reader<'a> {
    source: &'a Source,
    /// The images read so far, or why one couldn't be.
    bitmaps: HashMap<String, Result<Bitmap, String>>,
    problems: Vec<Problem>,
}

impl Reader<'_> {
    fn problem(&mut self, at: &str, message: &str) {
        self.problems.push(Problem {
            at: at.to_string(),
            message: message.to_string(),
        });
    }

    fn unknown_keys(&mut self, at: &str, table: &Table, known: &[&str]) {
        for key in table.keys().filter(|key| !known.contains(&key.as_str())) {
            let at = join(at, key);
            self.problem(&at, "isn't a key Spotless knows; ignored");
        }
    }

    /// The image at `path`'s size, reading it the first time.
    fn bitmap(&mut self, path: &str) -> Result<(u32, u32), String> {
        if !self.bitmaps.contains_key(path) {
            let bitmap = self.source.read(path).and_then(|bytes| {
                let bitmap =
                    Bitmap::decode(&bytes).ok_or_else(|| format!("{path} isn't a PNG image"))?;
                if bitmap.width > MAX_SIDE || bitmap.height > MAX_SIDE {
                    return Err(format!("{path} is bigger than {MAX_SIDE} pixels a side"));
                }
                Ok(bitmap)
            });
            self.bitmaps.insert(path.to_string(), bitmap);
        }
        match &self.bitmaps[path] {
            Ok(bitmap) => Ok((bitmap.width, bitmap.height)),
            Err(error) => Err(error.clone()),
        }
    }

    /// An image reference: a path for a whole image, or `{ image, rect }`
    /// for part of one.
    fn image(&mut self, at: &str, value: &Value) -> Option<ImageRef> {
        let (path, rect) = match value {
            Value::String(path) => (path.clone(), None),
            Value::Table(table) => {
                self.unknown_keys(at, table, &["image", "rect"]);
                let Some(Value::String(path)) = table.get("image") else {
                    self.problem(at, "needs image = \"file.png\"");
                    return None;
                };
                let rect = match table.get("rect").map(integers::<4>) {
                    Some(Some(rect)) => Some(rect),
                    Some(None) => {
                        self.problem(at, "rect should be [x, y, width, height]");
                        return None;
                    }
                    None => None,
                };
                (path.clone(), rect)
            }
            _ => {
                self.problem(at, "should be \"file.png\" or { image = \"file.png\", rect = [x, y, width, height] }");
                return None;
            }
        };
        let (width, height) = match self.bitmap(&path) {
            Ok(size) => size,
            Err(error) => {
                self.problem(at, &error);
                return None;
            }
        };
        let [x, y, w, h] = rect.unwrap_or([0, 0, width, height]);
        if w == 0 || h == 0 {
            self.problem(at, "rect has no size");
            return None;
        }
        if u64::from(x) + u64::from(w) > u64::from(width)
            || u64::from(y) + u64::from(h) > u64::from(height)
        {
            self.problem(
                at,
                &format!("rect [{x}, {y}, {w}, {h}] goes outside {path} ({width}×{height})"),
            );
            return None;
        }
        Some(ImageRef {
            image: path,
            area: Area {
                x,
                y,
                width: w,
                height: h,
            },
        })
    }

    fn colors(&mut self, value: Option<&Value>) -> Palette {
        let mut palette = Palette::CLASSIC;
        let table = match value {
            Some(Value::Table(table)) => table,
            Some(_) => {
                self.problem("colors", "should be a table");
                return palette;
            }
            None => return palette,
        };
        for (key, value) in table {
            let at = join("colors", key);
            let Some((_, field)) = COLORS.iter().find(|(name, _)| name == key) else {
                self.problem(&at, "isn't a colour Spotless knows; ignored");
                continue;
            };
            match value.as_str().and_then(hex) {
                Some(color) => *field(&mut palette) = color,
                None => self.problem(&at, "should be a colour like \"#2bff62\""),
            }
        }
        palette
    }

    fn fonts(&mut self, value: Option<&Value>) -> HashMap<Role, Font> {
        let mut fonts = HashMap::new();
        let table = match value {
            Some(Value::Table(table)) => table,
            Some(_) => {
                self.problem("fonts", "should be a table");
                return fonts;
            }
            None => return fonts,
        };
        for (key, value) in table {
            let at = join("fonts", key);
            let Some(role) = Role::ALL.into_iter().find(|role| role.key() == key) else {
                self.problem(
                    &at,
                    "isn't a text role; they are digits, display, chrome and list",
                );
                continue;
            };
            let Value::Table(font) = value else {
                self.problem(&at, "should be a table");
                continue;
            };
            if let Some(font) = self.font(&at, font) {
                fonts.insert(role, font);
            }
        }
        fonts
    }

    fn font(&mut self, at: &str, table: &Table) -> Option<Font> {
        self.unknown_keys(at, table, &["bitmap", "tint", "file", "size"]);
        match (table.get("bitmap"), table.get("file")) {
            (Some(Value::Table(bitmap)), None) => {
                let tint = match table.get("tint") {
                    None => false,
                    Some(Value::Boolean(tint)) => *tint,
                    Some(_) => {
                        self.problem(at, "tint should be true or false");
                        false
                    }
                };
                self.bitmap_font(&join(at, "bitmap"), bitmap, tint)
            }
            (None, Some(Value::String(path))) => {
                let size = match table.get("size") {
                    None => FONT_SIZE,
                    Some(size) => match number(size) {
                        Some(size) if size > 0.0 => size,
                        _ => {
                            self.problem(at, "size should be a number of points above 0");
                            FONT_SIZE
                        }
                    },
                };
                match self.source.read(path) {
                    Ok(bytes) if is_font(&bytes) => Some(Font::File {
                        bytes,
                        size: size as f32,
                    }),
                    Ok(_) => {
                        self.problem(at, &format!("{path} isn't a TrueType or OpenType font"));
                        None
                    }
                    Err(error) => {
                        self.problem(at, &error);
                        None
                    }
                }
            }
            _ => {
                self.problem(at, "needs either bitmap = { … } or file = \"font.ttf\"");
                None
            }
        }
    }

    fn bitmap_font(&mut self, at: &str, table: &Table, tint: bool) -> Option<Font> {
        self.unknown_keys(at, table, &["image", "origin", "cell", "rows", "spacing"]);
        let Some(Value::String(path)) = table.get("image") else {
            self.problem(at, "needs image = \"font.png\"");
            return None;
        };
        let Some([cell_width, cell_height]) = table
            .get("cell")
            .and_then(integers::<2>)
            .filter(|cell| cell.iter().all(|&n| n > 0))
        else {
            self.problem(at, "needs cell = [width, height], the size of one glyph");
            return None;
        };
        let origin = match table.get("origin") {
            None => [0, 0],
            Some(origin) => match integers::<2>(origin) {
                Some(origin) => origin,
                None => {
                    self.problem(at, "origin should be [x, y]");
                    return None;
                }
            },
        };
        let spacing = match table.get("spacing") {
            None => 0,
            Some(spacing) => match integer(spacing) {
                Some(spacing) => spacing,
                None => {
                    self.problem(at, "spacing should be a whole number of pixels");
                    0
                }
            },
        };
        let rows: Option<Vec<&str>> = match table.get("rows") {
            Some(Value::Array(rows)) => rows.iter().map(Value::as_str).collect(),
            _ => None,
        };
        let Some(rows) = rows.filter(|rows| !rows.is_empty()) else {
            self.problem(
                at,
                "needs rows = [\"ABC…\", …], the characters of each row of cells",
            );
            return None;
        };
        let (width, height) = match self.bitmap(path) {
            Ok(size) => size,
            Err(error) => {
                self.problem(at, &error);
                return None;
            }
        };
        let columns = rows
            .iter()
            .map(|row| row.chars().count())
            .max()
            .unwrap_or(0) as u64;
        let [x, y] = origin.map(u64::from);
        if x + columns * u64::from(cell_width) > u64::from(width)
            || y + rows.len() as u64 * u64::from(cell_height) > u64::from(height)
        {
            self.problem(
                at,
                &format!("the cells go outside {path} ({width}×{height})"),
            );
            return None;
        }
        let mut glyphs = HashMap::new();
        for (row, cells) in rows.iter().enumerate() {
            for (column, character) in cells.chars().enumerate() {
                glyphs
                    .entry(character)
                    .or_insert((column as u32, row as u32));
            }
        }
        Some(Font::Bitmap {
            image: path.clone(),
            origin,
            cell: [cell_width, cell_height],
            spacing,
            glyphs,
            tint,
        })
    }

    /// The pieces in `table`, whose names start with `prefix`.
    fn pieces(&mut self, prefix: &str, table: &Table, pieces: &mut HashMap<&'static str, Piece>) {
        for (key, value) in table {
            let name = join(prefix, key);
            let at = join("pieces", &name);
            let Value::Table(table) = value else {
                self.problem(&at, "should be a table");
                continue;
            };
            let known = PIECES.iter().find(|(piece, _)| *piece == name);
            match known {
                Some(&(name, kind)) => {
                    if let Some(piece) = self.piece(&at, name, kind, table) {
                        pieces.insert(name, piece);
                    }
                }
                None if prefix.is_empty() && GROUPS.contains(&key.as_str()) => {
                    self.pieces(&name, table, pieces);
                }
                None => self.problem(&at, "isn't a piece Spotless draws; ignored"),
            }
        }
    }

    fn piece(&mut self, at: &str, name: &str, kind: Kind, table: &Table) -> Option<Piece> {
        let mut known = vec!["image"];
        known.extend(State::ALL.iter().map(|state| state.key()));
        match kind {
            Kind::Sprite => {}
            Kind::Bar { .. } => known.extend(["slice", "middle", "levels"]),
            Kind::Frame => known.extend(["slice", "edges", "center", "padding"]),
        }
        if name == "title" {
            known.push("title_gap");
        }
        self.unknown_keys(at, table, &known);

        let mut images = HashMap::new();
        if let Some(value) = table.get("image") {
            if table.contains_key("normal") {
                self.problem(at, "has both image and normal; normal is used");
            } else if let Some(image) = self.image(&join(at, "image"), value) {
                images.insert(State::Normal, image);
            }
        }
        for state in State::ALL {
            if let Some(value) = table.get(state.key())
                && let Some(image) = self.image(&join(at, state.key()), value)
            {
                images.insert(state, image);
            }
        }
        let Some(normal) = images.get(&State::Normal).map(|image| image.area) else {
            if !table.contains_key("image") && !table.contains_key("normal") {
                self.problem(at, "needs image = … (or normal = … with its states)");
            }
            return None;
        };
        if images
            .values()
            .any(|image| image.area.size() != normal.size())
        {
            self.problem(at, "its states differ in size; each is drawn at Normal's");
        }

        let shape = match kind {
            Kind::Sprite => Shape::Sprite,
            Kind::Bar { vertical } => {
                let Some(ends) = table.get("slice").and_then(integers::<2>) else {
                    self.problem(at, "needs slice = [start, end], the fixed ends in pixels");
                    return None;
                };
                let length = if vertical {
                    normal.height
                } else {
                    normal.width
                };
                if u64::from(ends[0]) + u64::from(ends[1]) > u64::from(length) {
                    self.problem(at, "its slice is longer than the image");
                    return None;
                }
                Shape::Bar {
                    vertical,
                    ends,
                    middle: self.fill(at, table, "middle")?,
                }
            }
            Kind::Frame => {
                let Some(slice) = table.get("slice").and_then(integers::<4>) else {
                    self.problem(
                        at,
                        "needs slice = [left, top, right, bottom], the fixed edges in pixels",
                    );
                    return None;
                };
                let [left, top, right, bottom] = slice.map(u64::from);
                if left + right > u64::from(normal.width) || top + bottom > u64::from(normal.height)
                {
                    self.problem(at, "its slice is bigger than the image");
                    return None;
                }
                let padding = match table.get("padding") {
                    None => slice,
                    Some(padding) => match integers::<4>(padding) {
                        Some(padding) => padding,
                        None => {
                            self.problem(at, "padding should be [left, top, right, bottom]");
                            slice
                        }
                    },
                };
                let center = match table.get("center") {
                    Some(Value::String(center)) if center == "none" => None,
                    _ => Some(self.fill(at, table, "center")?),
                };
                Shape::Frame {
                    slice,
                    edges: self.fill(at, table, "edges")?,
                    center,
                    padding,
                }
            }
        };
        let title_gap = match table.get("title_gap") {
            None => 0,
            Some(gap) => integer(gap).unwrap_or_else(|| {
                self.problem(at, "title_gap should be a whole number of pixels");
                0
            }),
        };
        let levels = match table.get("levels") {
            None => None,
            Some(levels) => Some(self.levels(at, levels, &images)?),
        };
        Some(Piece {
            states: States { images },
            shape,
            title_gap,
            levels,
        })
    }

    /// A bar's shades: `{ count, step }`, each shade `step` pixels below
    /// the last, all of them inside the image.
    fn levels(
        &mut self,
        at: &str,
        value: &Value,
        images: &HashMap<State, ImageRef>,
    ) -> Option<Levels> {
        let table = value.as_table();
        let get = |key: &str| table.and_then(|t| t.get(key)).and_then(integer);
        let (Some(count), Some(step)) = (get("count"), get("step")) else {
            self.problem(
                at,
                "levels should be { count = shades, step = pixels from one to the next }",
            );
            return None;
        };
        if count == 0 {
            self.problem(at, "levels needs a count of 1 or more");
            return None;
        }
        for image in images.values() {
            let height = self.bitmap(&image.image).map_or(0, |(_, height)| height);
            let last = u64::from(image.area.y)
                + u64::from(count - 1) * u64::from(step)
                + u64::from(image.area.height);
            if last > u64::from(height) {
                self.problem(
                    at,
                    &format!("its {count} shades go below the bottom of {}", image.image),
                );
                return None;
            }
        }
        Some(Levels { count, step })
    }

    /// How `key` fills: stretched unless the skin says tile. A wrong value
    /// drops the piece, since the skin's art was made for the other.
    fn fill(&mut self, at: &str, table: &Table, key: &str) -> Option<Fill> {
        match table.get(key) {
            None => Some(Fill::Stretch),
            Some(Value::String(fill)) if fill == "stretch" => Some(Fill::Stretch),
            Some(Value::String(fill)) if fill == "tile" => Some(Fill::Tile),
            Some(_) => {
                let also = if key == "center" {
                    ", \"tile\" or \"none\""
                } else {
                    " or \"tile\""
                };
                self.problem(at, &format!("{key} should be \"stretch\"{also}"));
                None
            }
        }
    }
}

fn join(prefix: &str, key: &str) -> String {
    if prefix.is_empty() {
        key.to_string()
    } else {
        format!("{prefix}.{key}")
    }
}

fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Integer(n) => Some(*n as f64),
        Value::Float(n) => Some(*n),
        _ => None,
    }
}

fn integer(value: &Value) -> Option<u32> {
    value.as_integer().and_then(|n| u32::try_from(n).ok())
}

/// An array of exactly `N` whole numbers, none negative.
fn integers<const N: usize>(value: &Value) -> Option<[u32; N]> {
    let array = value.as_array()?;
    let numbers: Option<Vec<u32>> = array.iter().map(integer).collect();
    numbers?.try_into().ok()
}

/// Whether `bytes` start as a TrueType, OpenType or collection file does.
fn is_font(bytes: &[u8]) -> bool {
    matches!(
        bytes.get(..4),
        Some([0, 1, 0, 0] | b"OTTO" | b"true" | b"ttcf")
    )
}
