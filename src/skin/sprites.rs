//! Where each piece of the player lives inside a skin's bitmaps.
//!
//! A classic skin is a handful of sprite sheets with fixed coordinates that
//! every skin honours, so the same table cuts every skin. The numbers are
//! Webamp's (`skinSprites.ts`), which were measured against Winamp itself.

/// One of the skin's bitmaps, by the file it comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sheet {
    /// The main window's background, for its colours.
    Main,
    /// Transport buttons.
    CButtons,
    /// Shuffle and repeat.
    ShufRep,
    /// The seek bar and its thumb.
    PosBar,
    /// The volume slider's 28 frames and thumb.
    Volume,
    /// The play, pause, and stop status glyphs.
    PlayPaus,
    /// The time display's digits.
    Numbers,
    /// Digits with a blank cell and a minus sign; newer skins ship this.
    NumsEx,
    /// The 5x6 bitmap font.
    Text,
    /// The playlist editor, for its colours.
    PlEdit,
}

impl Sheet {
    pub const ALL: [Sheet; 10] = [
        Sheet::Main,
        Sheet::CButtons,
        Sheet::ShufRep,
        Sheet::PosBar,
        Sheet::Volume,
        Sheet::PlayPaus,
        Sheet::Numbers,
        Sheet::NumsEx,
        Sheet::Text,
        Sheet::PlEdit,
    ];

    /// The file's name, in lower case.
    pub fn file_name(self) -> &'static str {
        match self {
            Sheet::Main => "main.bmp",
            Sheet::CButtons => "cbuttons.bmp",
            Sheet::ShufRep => "shufrep.bmp",
            Sheet::PosBar => "posbar.bmp",
            Sheet::Volume => "volume.bmp",
            Sheet::PlayPaus => "playpaus.bmp",
            Sheet::Numbers => "numbers.bmp",
            Sheet::NumsEx => "nums_ex.bmp",
            Sheet::Text => "text.bmp",
            Sheet::PlEdit => "pledit.bmp",
        }
    }
}

/// A rectangle inside a sheet, in skin pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sprite {
    pub sheet: Sheet,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Sprite {
    pub const fn new(sheet: Sheet, x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            sheet,
            x,
            y,
            width,
            height,
        }
    }

    /// The part of this sprite a sheet of the given size actually holds.
    /// Skins are not always the sizes the table expects (a seek bar four
    /// pixels tall, a shorter volume sheet), and Winamp drew what was there.
    pub fn clipped_to(self, sheet_width: u32, sheet_height: u32) -> Option<Self> {
        let right = self.x.saturating_add(self.width).min(sheet_width);
        let bottom = self.y.saturating_add(self.height).min(sheet_height);
        if right <= self.x || bottom <= self.y {
            return None;
        }
        Some(Self {
            width: right - self.x,
            height: bottom - self.y,
            ..self
        })
    }
}

macro_rules! sprites {
    ($($(#[$attr:meta])* $name:ident = ($sheet:ident, $x:expr, $y:expr, $w:expr, $h:expr);)*) => {
        $($(#[$attr])* pub const $name: Sprite = Sprite::new(Sheet::$sheet, $x, $y, $w, $h);)*
    };
}

sprites! {
    PREVIOUS = (CButtons, 0, 0, 23, 18);
    PREVIOUS_PRESSED = (CButtons, 0, 18, 23, 18);
    PLAY = (CButtons, 23, 0, 23, 18);
    PLAY_PRESSED = (CButtons, 23, 18, 23, 18);
    PAUSE = (CButtons, 46, 0, 23, 18);
    PAUSE_PRESSED = (CButtons, 46, 18, 23, 18);
    STOP = (CButtons, 69, 0, 23, 18);
    STOP_PRESSED = (CButtons, 69, 18, 23, 18);
    NEXT = (CButtons, 92, 0, 22, 18);
    NEXT_PRESSED = (CButtons, 92, 18, 22, 18);

    SHUFFLE_OFF = (ShufRep, 28, 0, 47, 15);
    SHUFFLE_OFF_PRESSED = (ShufRep, 28, 15, 47, 15);
    SHUFFLE_ON = (ShufRep, 28, 30, 47, 15);
    SHUFFLE_ON_PRESSED = (ShufRep, 28, 45, 47, 15);
    REPEAT_OFF = (ShufRep, 0, 0, 28, 15);
    REPEAT_OFF_PRESSED = (ShufRep, 0, 15, 28, 15);
    REPEAT_ON = (ShufRep, 0, 30, 28, 15);
    REPEAT_ON_PRESSED = (ShufRep, 0, 45, 28, 15);

    POSITION_TRACK = (PosBar, 0, 0, 248, 10);
    POSITION_THUMB = (PosBar, 248, 0, 29, 10);
    POSITION_THUMB_PRESSED = (PosBar, 278, 0, 29, 10);

    VOLUME_THUMB = (Volume, 15, 422, 14, 11);
    VOLUME_THUMB_PRESSED = (Volume, 0, 422, 14, 11);

    /// The playlist editor's title bar, its top fourteen rows only (the
    /// frame's edge is below): the left corner, the title piece, and the
    /// stripes that tile; lit, and unlit for a window in the background.
    TITLE_LEFT = (PlEdit, 0, 0, 25, 14);
    TITLE_MIDDLE = (PlEdit, 26, 0, 100, 14);
    TITLE_TILE = (PlEdit, 127, 0, 25, 14);
    TITLE_LEFT_UNLIT = (PlEdit, 0, 21, 25, 14);
    TITLE_MIDDLE_UNLIT = (PlEdit, 26, 21, 100, 14);
    TITLE_TILE_UNLIT = (PlEdit, 127, 21, 25, 14);

    STATUS_PLAYING = (PlayPaus, 0, 0, 9, 9);
    STATUS_PAUSED = (PlayPaus, 9, 0, 9, 9);
    STATUS_STOPPED = (PlayPaus, 18, 0, 9, 9);
}

/// How many steps the volume slider is drawn in.
pub const SLIDER_FRAMES: u32 = 28;

/// The volume track at a given fill, `frame` from 0 (silent) to 27 (full).
pub fn volume_frame(frame: u32) -> Sprite {
    Sprite::new(Sheet::Volume, 0, 15 * frame.min(SLIDER_FRAMES - 1), 68, 13)
}

/// A digit of the time display, from the plain or the extended sheet.
pub fn digit(sheet: Sheet, value: u32) -> Sprite {
    Sprite::new(sheet, 9 * value.min(9), 0, 9, 13)
}

/// A cell of the bitmap font: three rows of thirty-one 5x6 glyphs.
pub fn glyph(row: u32, column: u32) -> Sprite {
    Sprite::new(Sheet::Text, 5 * column, 6 * row, 5, 6)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sprite_beyond_a_short_sheet_is_clipped_or_dropped() {
        let clipped = POSITION_TRACK.clipped_to(307, 4).unwrap();
        assert_eq!((clipped.width, clipped.height), (248, 4));
        assert!(VOLUME_THUMB.clipped_to(68, 419).is_none());
        assert_eq!(VOLUME_THUMB.clipped_to(68, 433), Some(VOLUME_THUMB));
    }

    #[test]
    fn indexed_sprites_stay_inside_their_sheets() {
        assert_eq!(volume_frame(27).y, 405);
        assert_eq!(volume_frame(99).y, 405);
        assert_eq!(digit(Sheet::Numbers, 9).x, 81);
        assert_eq!(digit(Sheet::NumsEx, 12).x, 81);
        assert_eq!(glyph(2, 30), Sprite::new(Sheet::Text, 150, 12, 5, 6));
    }
}
