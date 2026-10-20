//! Converts a classic Winamp skin (.wsz) into a Spotless skin folder.
//!
//! `cargo run --example convert_skin -- base-2.91.wsz ~/.config/spotless/skins/winamp-classic "Winamp Classic"`

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use spotless::skin::Skin;
use spotless::skin::format::convert::convert;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let (Some(input), Some(output)) = (args.next(), args.next()) else {
        bail!("usage: convert_skin <skin.wsz> <output folder> [name]");
    };
    let input = PathBuf::from(input);
    let output = PathBuf::from(output);
    let name = args.next().unwrap_or_else(|| {
        input
            .file_stem()
            .map_or_else(|| "Skin".into(), |stem| stem.to_string_lossy().into_owned())
    });
    let skin = Skin::load(&input).with_context(|| format!("reading {}", input.display()))?;
    let missing = skin.missing_art();
    if !missing.is_empty() {
        println!("only its colours: it has no {}", missing.join(", "));
    }
    for (path, bytes) in convert(&skin, &name) {
        let to = output.join(&path);
        if let Some(dir) = to.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&to, bytes).with_context(|| format!("writing {}", to.display()))?;
        println!("{path}");
    }
    Ok(())
}
