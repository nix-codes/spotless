//! Album covers: fetched once, kept on disk, shrunk for the catalog.
//!
//! The cache file is named after the image's ID in its URL (Spotify's
//! `i.scdn.co/image/<hex>`), so the same cover is downloaded once whichever
//! list it shows in.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

/// Covers are kept at this many pixels square: the large cover in an
/// opened album, and thumbnails drawn smaller from it.
pub const COVER_SIZE: u32 = 160;

/// The cache file for `url`, or `None` for a URL that isn't an image ID.
pub fn cache_path(dir: &Path, url: &str) -> Option<PathBuf> {
    let id = url.trim_end_matches('/').rsplit('/').next()?;
    let safe = !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric());
    safe.then(|| dir.join(format!("{id}.jpg")))
}

/// The cover at `url`, from the cache or downloaded into it, at
/// [`COVER_SIZE`].
pub async fn cover(http: &reqwest::Client, dir: &Path, url: &str) -> Result<egui::ColorImage> {
    let path = cache_path(dir, url).context("not a cover URL")?;
    let bytes = match tokio::fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(_) => {
            let bytes = download(http, url).await?;
            if let Err(error) = save(&path, &bytes).await {
                log::warn!("unable to cache {}: {error:#}", path.display());
            }
            bytes
        }
    };
    tokio::task::spawn_blocking(move || shrink(&bytes)).await?
}

async fn download(http: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    if !url.starts_with("https://") {
        bail!("not an https URL");
    }
    let response = http
        .get(url)
        .send()
        .await
        .context("cover download failed")?;
    if !response.status().is_success() {
        bail!("cover download failed: HTTP {}", response.status());
    }
    Ok(response.bytes().await?.to_vec())
}

async fn save(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    let temporary = path.with_extension("jpg.tmp");
    tokio::fs::write(&temporary, bytes).await?;
    tokio::fs::rename(&temporary, path).await?;
    Ok(())
}

fn shrink(bytes: &[u8]) -> Result<egui::ColorImage> {
    let image = image::load_from_memory(bytes).context("not an image")?;
    let image = image
        .resize_to_fill(
            COVER_SIZE,
            COVER_SIZE,
            image::imageops::FilterType::Triangle,
        )
        .to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        size,
        image.as_raw(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_files_are_named_after_the_image_id() {
        let dir = Path::new("/cache");
        assert_eq!(
            cache_path(
                dir,
                "https://i.scdn.co/image/ab67616d00001e02ff9ca10b55ce82ae553c8228"
            ),
            Some(dir.join("ab67616d00001e02ff9ca10b55ce82ae553c8228.jpg"))
        );
        // Only the last segment is used, so a path can't leave the folder.
        assert_eq!(
            cache_path(dir, "https://i.scdn.co/image/../../etc"),
            Some(dir.join("etc.jpg"))
        );
        assert_eq!(cache_path(dir, "https://i.scdn.co/image/a.b"), None);
    }

    #[test]
    fn covers_shrink_to_a_square() {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(400, 300, image::Rgba([200, 10, 10, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let cover = shrink(&png).unwrap();
        assert_eq!(cover.size, [COVER_SIZE as usize, COVER_SIZE as usize]);
    }
}
