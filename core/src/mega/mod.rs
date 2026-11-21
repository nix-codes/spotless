//! MEGA as a source of songs: files in a public folder, found by its link,
//! played without an account. The download uses the listener's own
//! allowance of MEGA's traffic, not the folder owner's.

pub mod api;
pub mod audio;
pub mod folder;
pub mod link;

use std::io::Read;

use api::{Api, ApiError};
use link::{FileKey, FolderLink};

use crate::model::catalog::MegaFolder;
use crate::playback::decrypt::Decrypt;

/// An M3U bigger than this isn't read: a playlist is a few KB.
const M3U_LIMIT: u64 = 1024 * 1024;

/// Whether this text is a public MEGA folder's link.
pub fn is_folder_link(text: &str) -> bool {
    FolderLink::parse(text).is_some()
}

/// A public folder's playlists, from its link: its listing, and the text
/// of each M3U in it.
pub async fn read_folder(api: &Api, link: &str) -> Result<MegaFolder, String> {
    let link = FolderLink::parse(link).ok_or("That isn't a MEGA folder's link.")?;
    let folder_key = link.folder_key().ok_or("That MEGA link's key is wrong.")?;
    let nodes = api.list(&link.handle).await.map_err(|e| said(&e))?;
    let (name, files) = folder::files(&nodes, &folder_key);
    let mut m3us = Vec::new();
    for file in files.iter().filter(|file| file.is_playlist()) {
        let node = nodes.iter().find(|node| node.handle == file.handle);
        let Some(key) = node.and_then(|node| FileKey::open(&node.key, &folder_key)) else {
            continue;
        };
        if file.size > M3U_LIMIT {
            log::warn!("{} is too big for a playlist; left out", file.path);
            continue;
        }
        let text = read_file(api, &link.handle, &file.handle, key).await;
        match text {
            Ok(text) => m3us.push((file.clone(), text)),
            Err(error) => return Err(format!("{} couldn't be read: {}", file.path, said(&error))),
        }
    }
    let folder = folder::playlists(&link.handle, &link.key, &name, &files, &m3us);
    if folder.playlists.is_empty() {
        return Err("That MEGA folder has no songs to play.".into());
    }
    Ok(folder)
}

/// A small file's text, whole.
async fn read_file(api: &Api, folder: &str, file: &str, key: FileKey) -> Result<String, ApiError> {
    let (url, _) = api.download(folder, file).await?;
    let response = (api.downloads().get(&url).send().await)
        .map_err(|e| ApiError::Unreachable(e.without_url().to_string()))?;
    match response.status().as_u16() {
        200 | 206 => {}
        509 => return Err(ApiError::OverQuota),
        status => return Err(ApiError::Unreachable(format!("answered {status}"))),
    }
    let encrypted = (response.bytes().await)
        .map_err(|e| ApiError::Unreachable(e.without_url().to_string()))?;
    let mut text = Vec::new();
    Decrypt::new(Some(key.aes), key.iv, std::io::Cursor::new(encrypted))
        .read_to_end(&mut text)
        .map_err(|e| ApiError::Unreachable(e.to_string()))?;
    Ok(String::from_utf8_lossy(&text).into_owned())
}

/// What went wrong, as the status bar says it.
fn said(error: &ApiError) -> String {
    match error {
        ApiError::Unreachable(_) => "MEGA couldn't be reached.".into(),
        ApiError::OverQuota => {
            "This connection has used up MEGA's download allowance for now.".into()
        }
        ApiError::NotFound => "That MEGA folder isn't there: its link may be wrong or taken down.".into(),
        ApiError::Refused(code) => format!("MEGA refused it ({code})."),
    }
}
