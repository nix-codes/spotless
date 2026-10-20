//! The skins compiled into Spotless.

use super::{LoadError, MANIFEST, Skin};

/// Spotless's own look, which is used when no other skin is chosen.
const SPOTLESS_CLASSIC: &[(&str, &[u8])] = &[(
    MANIFEST,
    include_bytes!("../built_in/spotless-classic/skin.toml"),
)];

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
}
