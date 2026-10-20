//! Where a skin's files come from: a folder, a zip of one, or files
//! compiled in.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use super::{LoadError, MANIFEST};
use crate::skin::zip;

/// The most a skin may hold, so a stray huge file can't take the memory.
pub const MAX_BYTES: usize = 64 * 1024 * 1024;

pub enum Source {
    Folder(PathBuf),
    /// Files by their path from the skin's root, `/`-separated.
    Files(HashMap<String, Vec<u8>>),
}

impl Source {
    pub fn folder(dir: &Path) -> Self {
        Self::Folder(dir.to_path_buf())
    }

    pub fn files(files: &[(&str, &[u8])]) -> Self {
        Self::Files(
            files
                .iter()
                .map(|(path, bytes)| (path.to_string(), bytes.to_vec()))
                .collect(),
        )
    }

    /// A zip's files. A zip made of the skin's folder has everything one
    /// folder down; the root is wherever `skin.toml` is nearest the top.
    pub fn archive(bytes: &[u8]) -> Result<Self, LoadError> {
        let archive = zip::Archive::parse(bytes).map_err(|error| match error {
            zip::ZipError::NotAnArchive => LoadError::NotAnArchive,
            error => LoadError::Archive(error.to_string()),
        })?;
        let files: Vec<_> = archive.entries().iter().filter(|e| !e.is_dir()).collect();
        let root = files
            .iter()
            .map(|entry| entry.name.replace('\\', "/"))
            .filter_map(|name| name.strip_suffix(MANIFEST).map(String::from))
            .filter(|prefix| prefix.is_empty() || prefix.ends_with('/'))
            .min_by_key(|prefix| prefix.matches('/').count())
            .ok_or(LoadError::NoManifest)?;
        let mut read = HashMap::new();
        let mut total = 0;
        for entry in files {
            let name = entry.name.replace('\\', "/");
            let Some(path) = name.strip_prefix(&root) else {
                continue;
            };
            total += entry.size();
            if total > MAX_BYTES {
                return Err(LoadError::TooBig);
            }
            let bytes = archive
                .read(entry)
                .map_err(|error| LoadError::Archive(error.to_string()))?;
            read.insert(path.to_string(), bytes);
        }
        Ok(Self::Files(read))
    }

    /// The file at `path` from the skin's root. Paths that would leave the
    /// skin are refused.
    pub fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        let relative = safe(path).ok_or_else(|| format!("{path} isn't a path inside the skin"))?;
        match self {
            Self::Folder(dir) => {
                let full = dir.join(&relative);
                let size = std::fs::metadata(&full)
                    .map_err(|_| format!("{path} isn't in the skin"))?
                    .len();
                if size as usize > MAX_BYTES {
                    return Err(format!("{path} is too big"));
                }
                std::fs::read(&full).map_err(|error| format!("{path} can't be read: {error}"))
            }
            Self::Files(files) => {
                let key = relative.to_string_lossy().replace('\\', "/");
                files
                    .get(&key)
                    .cloned()
                    .ok_or_else(|| format!("{path} isn't in the skin"))
            }
        }
    }
}

/// `path` as a relative path with no way out of the skin's root.
fn safe(path: &str) -> Option<PathBuf> {
    let path = Path::new(path.trim());
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!clean.as_os_str().is_empty()).then_some(clean)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_stay_inside_the_skin() {
        assert_eq!(safe("img/play.png"), Some(PathBuf::from("img/play.png")));
        assert_eq!(safe("./play.png"), Some(PathBuf::from("play.png")));
        assert_eq!(safe("../secret.png"), None);
        assert_eq!(safe("/etc/passwd"), None);
        assert_eq!(safe(""), None);
    }

    #[test]
    fn a_zipped_folder_is_read_from_inside_the_folder() {
        let bytes = zip::write(&[
            ("my-skin/skin.toml", b"format = 1", true),
            ("my-skin/img/a.png", b"png", false),
            ("other/skin.toml", b"nested deeper", false),
            ("deeper/still/skin.toml", b"no", false),
        ]);
        let source = Source::archive(&bytes).unwrap();
        assert_eq!(source.read("skin.toml").unwrap(), b"format = 1");
        assert_eq!(source.read("img/a.png").unwrap(), b"png");
        assert!(source.read("../other/skin.toml").is_err());
    }

    #[test]
    fn a_zip_without_a_manifest_is_refused() {
        let bytes = zip::write(&[("main.bmp", b"bmp", false)]);
        assert!(matches!(
            Source::archive(&bytes),
            Err(LoadError::NoManifest)
        ));
        assert!(matches!(
            Source::archive(b"not a zip"),
            Err(LoadError::NotAnArchive)
        ));
    }
}
