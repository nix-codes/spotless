//! The skins compiled into Spotless.

use super::{LoadError, MANIFEST, Skin};

/// Spotless's own look, which is used when no other skin is chosen.
const SPOTLESS_CLASSIC: &[(&str, &[u8])] = &[(
    MANIFEST,
    include_bytes!("../built_in/spotless-classic/skin.toml"),
)];

/// Winamp's own look, from its base skin.
const WINAMP_CLASSIC: &[(&str, &[u8])] = &[
    (
        MANIFEST,
        include_bytes!("../built_in/winamp-classic/skin.toml"),
    ),
    (
        "cbuttons.png",
        include_bytes!("../built_in/winamp-classic/cbuttons.png"),
    ),
    (
        "shufrep.png",
        include_bytes!("../built_in/winamp-classic/shufrep.png"),
    ),
    (
        "posbar.png",
        include_bytes!("../built_in/winamp-classic/posbar.png"),
    ),
    (
        "volume.png",
        include_bytes!("../built_in/winamp-classic/volume.png"),
    ),
    (
        "playpaus.png",
        include_bytes!("../built_in/winamp-classic/playpaus.png"),
    ),
    (
        "text.png",
        include_bytes!("../built_in/winamp-classic/text.png"),
    ),
    (
        "pledit.png",
        include_bytes!("../built_in/winamp-classic/pledit.png"),
    ),
    (
        "digits.png",
        include_bytes!("../built_in/winamp-classic/digits.png"),
    ),
    (
        "lcd.png",
        include_bytes!("../built_in/winamp-classic/lcd.png"),
    ),
    (
        "display.png",
        include_bytes!("../built_in/winamp-classic/display.png"),
    ),
    (
        "small-display.png",
        include_bytes!("../built_in/winamp-classic/small-display.png"),
    ),
    (
        "titlebar.png",
        include_bytes!("../built_in/winamp-classic/titlebar.png"),
    ),
    (
        "title.png",
        include_bytes!("../built_in/winamp-classic/title.png"),
    ),
];

/// What Settings saves for Winamp Classic: no skin file can be called
/// this, as no file name has a colon on Windows.
pub const WINAMP_CLASSIC_KEY: &str = "built-in:winamp-classic";

pub fn winamp_classic() -> Result<Skin, LoadError> {
    Skin::built_in("Winamp Classic", WINAMP_CLASSIC)
}

/// Spotless Classic, read from its compiled-in files.
pub fn spotless_classic() -> Result<Skin, LoadError> {
    Skin::built_in("Spotless Classic", SPOTLESS_CLASSIC)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::Palette;

    #[test]
    fn spotless_classic_is_classic() {
        let skin = spotless_classic().unwrap();
        assert!(skin.problems.is_empty(), "{:?}", skin.problems);
        assert_eq!(skin.name, "Spotless Classic");
        // Every colour as Classic has it, and nothing else.
        assert_eq!(
            format!("{:?}", skin.palette),
            format!("{:?}", Palette::CLASSIC)
        );
        assert!(skin.fonts.is_empty() && skin.pieces.is_empty() && skin.images.is_empty());
    }

    #[test]
    fn winamp_classic_is_whole() {
        let skin = winamp_classic().unwrap();
        assert!(skin.problems.is_empty(), "{:?}", skin.problems);
        assert_eq!(skin.name, "Winamp Classic");
        // Every image the manifest names is compiled in.
        assert_eq!(skin.images.len(), WINAMP_CLASSIC.len() - 1);
        for piece in ["play", "titlebar", "field", "scrollbar.thumb"] {
            assert!(skin.piece(piece).is_some(), "{piece}");
        }
    }
}
