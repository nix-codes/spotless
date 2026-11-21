//! A public MEGA folder read as playlists, as plain code: its files by
//! where they are in it, an M3U's songs found among them, and the songs
//! a folder with no M3U has.
//!
//! An M3U names its songs by their place relative to itself; extended,
//! each comes after an `#EXTINF:<seconds>,<artist> - <title>` line, which
//! gives what the page shows before any song is downloaded.

use std::collections::HashMap;

use super::api::Node;
use super::link::FileKey;
use crate::model::catalog::{FolderPlaylist, FolderSong, MegaFolder};
use crate::model::{AlbumRef, ArtistRef, SongUri, TrackRef};

/// The files a folder's audio is taken from, by their names' extensions:
/// what the player decodes.
const AUDIO: [&str; 5] = ["ogg", "oga", "mp3", "flac", "wav"];
const PLAYLISTS: [&str; 2] = ["m3u", "m3u8"];

/// A file of the folder, its name opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    pub handle: String,
    /// Where it is in the folder: its subfolders' names and its own,
    /// joined with `/`.
    pub path: String,
    pub size: u64,
}

impl Listed {
    fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    /// Its name without its extension.
    fn stem(&self) -> &str {
        let name = self.name();
        name.rsplit_once('.').map_or(name, |(stem, _)| stem)
    }

    fn is(&self, extensions: &[&str]) -> bool {
        let name = self.name().to_lowercase();
        (name.rsplit_once('.')).is_some_and(|(_, extension)| extensions.contains(&extension))
    }

    pub fn is_playlist(&self) -> bool {
        self.is(&PLAYLISTS)
    }

    /// The folder its path is in, with its `/`, or nothing at the top.
    fn dir(&self) -> &str {
        self.path.rsplit_once('/').map_or("", |(dir, _)| &self.path[..=dir.len()])
    }
}

/// The public folder's name and its files, from its listing (itself
/// first). Those whose names don't open are left out.
pub fn files(nodes: &[Node], folder_key: &[u8; 16]) -> (String, Vec<Listed>) {
    let names: HashMap<&str, (String, &str)> = (nodes.iter())
        .filter_map(|node| {
            let key = match node.folder {
                true => FileKey::open_folder(&node.key, folder_key),
                false => FileKey::open(&node.key, folder_key),
            };
            let name = key?.name(&node.attributes)?;
            Some((node.handle.as_str(), (name, node.parent.as_str())))
        })
        .collect();
    let root = nodes.first().map(|node| node.handle.as_str());
    let folder_name = (root.and_then(|root| names.get(root)))
        .map(|(name, _)| name.clone())
        .unwrap_or_default();
    // A file's path, from its folder up to the public folder.
    let path = |handle: &str| {
        let mut parts = Vec::new();
        let mut at = handle;
        while Some(at) != root {
            let (name, parent) = names.get(at)?;
            parts.push(name.as_str());
            at = parent;
            // A listing that loops never ends here.
            if parts.len() > nodes.len() {
                return None;
            }
        }
        parts.reverse();
        Some(parts.join("/"))
    };
    let files = (nodes.iter())
        .filter(|node| !node.folder)
        .filter_map(|node| {
            Some(Listed {
                handle: node.handle.clone(),
                path: path(&node.handle)?,
                size: node.size,
            })
        })
        .collect();
    (folder_name, files)
}

/// A song an M3U names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Named {
    /// As written, relative to the M3U.
    pub path: String,
    pub artist: Option<String>,
    pub title: Option<String>,
    pub seconds: Option<u32>,
}

/// An M3U's songs, in order, with what its `#EXTINF` lines say of them.
pub fn read_m3u(text: &str) -> Vec<Named> {
    let mut songs = Vec::new();
    let mut about: Option<Named> = None;
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if let Some(info) = line.strip_prefix("#EXTINF:") {
            about = Some(extinf(info));
        } else if !line.is_empty() && !line.starts_with('#') {
            let mut song = about.take().unwrap_or_default();
            song.path = line.replace('\\', "/");
            songs.push(song);
        }
    }
    songs
}

/// `<seconds>[ attributes],<artist> - <title>`; a title alone without the
/// ` - `.
fn extinf(info: &str) -> Named {
    let (length, name) = info.split_once(',').unwrap_or((info, ""));
    let seconds = length.split_whitespace().next().and_then(|seconds| {
        let seconds: f64 = seconds.parse().ok()?;
        (seconds > 0.0).then_some(seconds.round() as u32)
    });
    let name = name.trim();
    let (artist, title) = match name.split_once(" - ") {
        Some((artist, title)) => (Some(artist.trim()), title.trim()),
        None => (None, name),
    };
    let text = |text: &str| (!text.is_empty()).then(|| text.to_string());
    Named {
        path: String::new(),
        artist: artist.and_then(text),
        title: text(title),
        seconds,
    }
}

/// `path` relative to the folder `dir` (with its `/`), its `.` and `..`
/// gone. None for what's outside the folder: an absolute path, a URL.
fn resolve(dir: &str, path: &str) -> Option<String> {
    if path.starts_with('/') || path.contains("://") || path.contains(":/") {
        return None;
    }
    let mut parts: Vec<&str> = dir.split('/').filter(|part| !part.is_empty()).collect();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

/// The public folder's playlists: one for each M3U, by name, given its
/// text; with none, one of its audio files by where they are, named after
/// the folder. `folder` and `key` are the link's.
pub fn playlists(
    folder: &str,
    key: &str,
    folder_name: &str,
    files: &[Listed],
    m3us: &[(Listed, String)],
) -> MegaFolder {
    let song = |file: &Listed, named: Option<&Named>| {
        let named = named.cloned().unwrap_or_default();
        FolderSong {
            track: track(
                SongUri::mega(folder, &file.handle, key),
                named.title.unwrap_or_else(|| file.stem().to_string()),
                named.artist,
                named.seconds,
            ),
            found: true,
        }
    };
    let mut playlists: Vec<FolderPlaylist> = (m3us.iter())
        .map(|(m3u, text)| {
            let songs = (read_m3u(text).iter())
                .map(|named| match find(files, m3u.dir(), &named.path) {
                    Some(file) => song(file, Some(named)),
                    None => FolderSong {
                        track: track(
                            SongUri::default(),
                            (named.title.clone()).unwrap_or_else(|| file_stem(&named.path)),
                            named.artist.clone(),
                            named.seconds,
                        ),
                        found: false,
                    },
                })
                .collect();
            FolderPlaylist {
                uri: SongUri::mega(folder, &m3u.handle, key).to_string(),
                name: m3u.stem().to_string(),
                songs,
            }
        })
        .collect();
    playlists.sort_by_key(|playlist| playlist.name.to_lowercase());
    if m3us.is_empty() {
        let mut audio: Vec<&Listed> = files.iter().filter(|file| file.is(&AUDIO)).collect();
        audio.sort_by_key(|file| file.path.to_lowercase());
        if !audio.is_empty() {
            playlists.push(FolderPlaylist {
                uri: SongUri::mega(folder, folder, key).to_string(),
                name: folder_name.to_string(),
                songs: audio.into_iter().map(|file| song(file, None)).collect(),
            });
        }
    }
    MegaFolder {
        name: folder_name.to_string(),
        playlists,
    }
}

/// The file an M3U in `dir` names, its case as written or, failing that,
/// any.
fn find<'a>(files: &'a [Listed], dir: &str, path: &str) -> Option<&'a Listed> {
    let path = resolve(dir, path)?;
    (files.iter().find(|file| file.path == path)).or_else(|| {
        let path = path.to_lowercase();
        files.iter().find(|file| file.path.to_lowercase() == path)
    })
}

fn file_stem(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.').map_or(name, |(stem, _)| stem).to_string()
}

/// A song as the folder tells it, before its file's tags are read: no
/// album yet.
fn track(uri: SongUri, title: String, artist: Option<String>, seconds: Option<u32>) -> TrackRef {
    TrackRef {
        uri,
        title,
        artists: artist.iter().map(|name| ArtistRef::by_name(name)).collect(),
        album: AlbumRef::by_name("", ""),
        duration_ms: seconds.unwrap_or_default().saturating_mul(1000),
        source: None,
        in_album: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(handle: &str, path: &str) -> Listed {
        Listed {
            handle: handle.into(),
            path: path.into(),
            size: 1,
        }
    }

    #[test]
    fn an_m3u_gives_its_songs_with_what_extinf_says() {
        let text = "\u{feff}#EXTM3U\r\n#EXTINF:252,Metrópoli - Song A\r\n01 Song A.ogg\r\n\
                    \r\n# a note\r\nsub\\02 B.ogg\r\n#EXTINF:-1 tvg-id=\"x\",Only a title\r\nC.mp3\r\n";
        assert_eq!(
            read_m3u(text),
            vec![
                Named {
                    path: "01 Song A.ogg".into(),
                    artist: Some("Metrópoli".into()),
                    title: Some("Song A".into()),
                    seconds: Some(252),
                },
                Named {
                    path: "sub/02 B.ogg".into(),
                    ..Named::default()
                },
                Named {
                    path: "C.mp3".into(),
                    artist: None,
                    title: Some("Only a title".into()),
                    seconds: None,
                },
            ]
        );
    }

    #[test]
    fn paths_are_relative_to_the_m3u_and_stay_in_the_folder() {
        assert_eq!(resolve("", "./a/b.ogg").as_deref(), Some("a/b.ogg"));
        assert_eq!(resolve("lists/", "../music/b.ogg").as_deref(), Some("music/b.ogg"));
        assert_eq!(resolve("", "../b.ogg"), None);
        assert_eq!(resolve("", "/home/me/b.ogg"), None);
        assert_eq!(resolve("", "C:/Music/b.ogg"), None);
        assert_eq!(resolve("", "https://example.com/b.ogg"), None);
        assert_eq!(listed("x", "lists/mine.m3u").dir(), "lists/");
        assert_eq!(listed("x", "mine.m3u").dir(), "");
    }

    #[test]
    fn an_m3us_songs_are_found_in_the_folder_or_shown_missing() {
        let files = [
            listed("a", "01 Song A.ogg"),
            listed("b", "Sub/02 B.ogg"),
            listed("l", "lists/mine.m3u"),
        ];
        let text = "#EXTINF:252,Metrópoli - Song A\n../01 Song A.ogg\n../sub/02 b.ogg\n\
                    #EXTINF:301,Someone - Gone\n../03 Gone.ogg\n";
        let folder = playlists("F", "K", "Shared", &files, &[(files[2].clone(), text.into())]);
        assert_eq!(folder.name, "Shared");
        let [list] = folder.playlists.as_slice() else {
            panic!("one playlist: {folder:?}");
        };
        assert_eq!(list.name, "mine");
        assert_eq!(list.uri, "mega:F:l#K");
        let songs: Vec<_> = (list.songs.iter())
            .map(|s| (s.track.uri.as_str(), s.track.title.as_str(), s.found))
            .collect();
        assert_eq!(
            songs,
            [("mega:F:a#K", "Song A", true), ("mega:F:b#K", "02 B", true), ("", "Gone", false)]
        );
        let first = &list.songs[0].track;
        assert_eq!(first.artists, [ArtistRef::by_name("Metrópoli")]);
        assert_eq!(first.duration_ms, 252_000);
        assert!(first.album.uri.is_empty());
        // Only the songs found are added, each from the playlist.
        let added = list.addable();
        assert_eq!(added.len(), 2);
        assert_eq!(added[0].source.as_ref().map(|s| s.name.as_str()), Some("mine"));
    }

    #[test]
    fn a_folder_with_no_m3u_is_its_audio_files_by_name() {
        let files = [
            listed("b", "b/2.ogg"),
            listed("c", "cover.jpg"),
            listed("a", "A.MP3"),
        ];
        let folder = playlists("F", "K", "Shared", &files, &[]);
        let [list] = folder.playlists.as_slice() else {
            panic!("one playlist: {folder:?}");
        };
        assert_eq!(list.name, "Shared");
        let titles: Vec<_> = list.songs.iter().map(|s| s.track.title.as_str()).collect();
        assert_eq!(titles, ["A", "2"]);
        assert!(playlists("F", "K", "Empty", &files[1..2], &[]).playlists.is_empty());
    }

    #[test]
    fn several_m3us_are_a_playlist_each_by_name() {
        let files = [listed("a", "a.ogg"), listed("z", "Zed.m3u8"), listed("y", "best.m3u")];
        let m3us = [(files[1].clone(), "a.ogg".into()), (files[2].clone(), "a.ogg".into())];
        let folder = playlists("F", "K", "Shared", &files, &m3us);
        let names: Vec<_> = folder.playlists.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["best", "Zed"]);
    }
}
