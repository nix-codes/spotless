//! The colour of the lists and the display, after old monitors' phosphors
//! (and a hi-fi's fluorescent display). Each interface draws with these
//! shades in place of its skin's greens.

/// An RGB colour.
pub type Rgb = [u8; 3];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phosphor {
    #[default]
    Green,
    Amber,
    PaperWhite,
    Vfd,
    IceBlue,
    DeepBlue,
    Purple,
}

/// A phosphor's shades.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shades {
    /// The songs.
    pub text: Rgb,
    /// Dimmer text: counts, hints.
    pub dim: Rgb,
    /// The display, and what is lit: buttons that are on, the tab shown.
    pub lit: Rgb,
    /// The playing song.
    pub current: Rgb,
    /// The selection's bar.
    pub selected: Rgb,
}

impl Phosphor {
    pub const ALL: [Phosphor; 7] = [
        Phosphor::Green,
        Phosphor::Amber,
        Phosphor::PaperWhite,
        Phosphor::Vfd,
        Phosphor::IceBlue,
        Phosphor::DeepBlue,
        Phosphor::Purple,
    ];

    /// Its key in the settings file.
    pub fn key(self) -> &'static str {
        match self {
            Phosphor::Green => "green",
            Phosphor::Amber => "amber",
            Phosphor::PaperWhite => "paper-white",
            Phosphor::Vfd => "vfd",
            Phosphor::IceBlue => "ice-blue",
            Phosphor::DeepBlue => "deep-blue",
            Phosphor::Purple => "purple",
        }
    }

    /// The phosphor saved as `key`; green for none or one unknown.
    pub fn from_key(key: Option<&str>) -> Self {
        Self::ALL
            .into_iter()
            .find(|p| Some(p.key()) == key)
            .unwrap_or_default()
    }

    pub fn name(self) -> &'static str {
        match self {
            Phosphor::Green => "Green",
            Phosphor::Amber => "Amber",
            Phosphor::PaperWhite => "Paper-white",
            Phosphor::Vfd => "VFD (Vacuum Fluorescent Display)",
            Phosphor::IceBlue => "Ice blue",
            Phosphor::DeepBlue => "Deep blue",
            Phosphor::Purple => "Purple",
        }
    }

    /// Where the colour comes from.
    pub fn origin(self) -> Option<&'static str> {
        match self {
            Phosphor::Green => Some("P1 and P39 phosphors: green-screen terminals and monitors"),
            Phosphor::Amber => Some("P3 phosphor: amber monitors of the 1980s"),
            Phosphor::PaperWhite => Some("P4 phosphor: paper-white monitors"),
            Phosphor::Vfd => Some("Vacuum fluorescent displays: hi-fi decks, car stereos"),
            _ => None,
        }
    }

    pub fn shades(self) -> Shades {
        let (text, dim, lit, current, selected) = match self {
            Phosphor::Green => (
                [0x00, 0xe6, 0x39],
                [0x14, 0xa8, 0x3a],
                [0x2b, 0xff, 0x62],
                [0xff, 0xff, 0xff],
                [0x0e, 0x4a, 0x1f],
            ),
            Phosphor::Amber => (
                [0xff, 0xb0, 0x00],
                [0xa8, 0x74, 0x0a],
                [0xff, 0xc2, 0x3a],
                [0xff, 0xf1, 0xd6],
                [0x57, 0x39, 0x00],
            ),
            Phosphor::PaperWhite => (
                [0xc3, 0xcd, 0xd2],
                [0x7b, 0x85, 0x8a],
                [0xe9, 0xf0, 0xf3],
                [0xff, 0xff, 0xff],
                [0x35, 0x3d, 0x42],
            ),
            Phosphor::Vfd => (
                [0x2e, 0xf2, 0xc8],
                [0x1c, 0x9a, 0x80],
                [0x5c, 0xff, 0xdc],
                [0xea, 0xff, 0xf9],
                [0x0b, 0x4a, 0x3e],
            ),
            Phosphor::IceBlue => (
                [0x6c, 0xc8, 0xff],
                [0x3d, 0x82, 0xad],
                [0x94, 0xdb, 0xff],
                [0xf0, 0xfa, 0xff],
                [0x10, 0x3c, 0x63],
            ),
            Phosphor::DeepBlue => (
                [0x5b, 0x8c, 0xff],
                [0x3a, 0x5f, 0xae],
                [0x7a, 0xa4, 0xff],
                [0xe8, 0xef, 0xff],
                [0x0f, 0x1f, 0x55],
            ),
            Phosphor::Purple => (
                [0xc3, 0x8b, 0xff],
                [0x7f, 0x5a, 0xad],
                [0xd6, 0xa8, 0xff],
                [0xf6, 0xed, 0xff],
                [0x3d, 0x22, 0x66],
            ),
        };
        Shades {
            text,
            dim,
            lit,
            current,
            selected,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phosphors_are_saved_by_key_and_green_is_the_default() {
        for phosphor in Phosphor::ALL {
            assert_eq!(Phosphor::from_key(Some(phosphor.key())), phosphor);
        }
        assert_eq!(Phosphor::from_key(None), Phosphor::Green);
        assert_eq!(Phosphor::from_key(Some("magenta")), Phosphor::Green);
    }
}
