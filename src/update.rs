//! Updates of the desktop app, from its GitHub releases.
//!
//! Once a day, and when asked, Spotless reads the repository's releases and
//! picks the newest `desktop-v` one. When it is newer and the user says so,
//! it downloads this platform's package, checks it against the release's
//! SHA256SUMS, and puts the program in it in place of the running one. The
//! running Spotless goes on as it was; the new one starts the next time
//! Spotless does, or at once with Restart.
//!
//! Only release builds update themselves (`SPOTLESS_RELEASE`, which the
//! Desktop Release workflow sets): a build from source is updated with git.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// This build looks for updates and installs them.
pub const ENABLED: bool = option_env!("SPOTLESS_RELEASE").is_some() && PLATFORM.is_some();

/// The argument a restart into an update starts Spotless with: the one
/// restarting is still quitting.
pub const RESTARTED: &str = "--restarted";

/// This platform's package, after `spotless-<version>-`, and the program's
/// path inside it.
const PLATFORM: Option<(&str, &str)> = if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
    Some(("linux-x64.tar.gz", "Spotless/spotless"))
} else if cfg!(all(windows, target_arch = "x86_64")) {
    Some(("windows-x64.zip", "Spotless/Spotless.exe"))
} else {
    None
};

/// The release's checksums, one `sha256sum` line per package.
const SUMS: &str = "SHA256SUMS";
const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
/// The first look, a little after Spotless starts, then one a day.
const FIRST_CHECK: Duration = Duration::from_secs(10);
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// The most a package may unpack to. The program is a few tens of
/// megabytes; this stops a broken one from filling the memory.
const UNPACKED_LIMIT: u64 = 256 << 20;

/// A release's version, as in its tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(u32, u32, u32);

impl Version {
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('.').map(|part| part.parse().ok());
        let version = Self(parts.next()??, parts.next()??, parts.next()??);
        parts.next().is_none().then_some(version)
    }

    /// This Spotless's.
    pub fn current() -> Self {
        Self::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Self(0, 0, 0))
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// A newer release, with what installing it takes.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: Version,
    /// Its page on GitHub, with its notes and packages.
    pub page: String,
    /// This platform's package: its name and where to download it.
    package: (String, String),
    /// Where its SHA256SUMS is. Without one the package can't be checked,
    /// so it is only downloaded by hand, from the page.
    sums: Option<String>,
}

impl Release {
    /// Spotless can install it itself.
    pub fn installable(&self) -> bool {
        self.sums.is_some()
    }
}

/// Where updating stands.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Status {
    /// Not looked yet.
    #[default]
    Unknown,
    Checking,
    /// No release is newer than this Spotless.
    Current,
    Available(Release),
    /// Being downloaded and put in place.
    Installing(Release),
    /// In place: it starts the next time Spotless does.
    Installed(Release),
    /// Looking or installing failed, and why; installing, with the
    /// release.
    Failed {
        release: Option<Release>,
        error: String,
    },
}

/// Looks for updates and installs them, on a thread of their own.
pub struct Updater {
    status: Status,
    sender: Sender<Status>,
    receiver: Receiver<Status>,
    /// Called when a look or an install is done.
    wake: Arc<dyn Fn() + Send + Sync>,
    /// When to look next, while looking once a day.
    next_check: Instant,
    /// The program an update went in place of, to restart into.
    program: Option<PathBuf>,
}

impl Updater {
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        let (sender, receiver) = channel();
        Self {
            status: Status::Unknown,
            sender,
            receiver,
            wake: Arc::new(wake),
            next_check: Instant::now() + FIRST_CHECK,
            program: None,
        }
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    /// Takes in what a look or an install found, and looks when it is
    /// time to, if `daily`. Says when to call again.
    pub fn tick(&mut self, daily: bool) -> Option<Duration> {
        if let Some(status) = self.receiver.try_iter().last() {
            self.status = status;
        }
        if !ENABLED || !daily {
            return None;
        }
        let now = Instant::now();
        if now >= self.next_check {
            self.next_check = now + CHECK_EVERY;
            // An update found or on its way is left as it is.
            if matches!(
                self.status,
                Status::Unknown | Status::Current | Status::Failed { release: None, .. }
            ) {
                self.check();
            }
        }
        Some(self.next_check - now)
    }

    /// Looks for a newer release.
    pub fn check(&mut self) {
        if !ENABLED
            || matches!(
                self.status,
                Status::Checking | Status::Installing(_) | Status::Installed(_)
            )
        {
            return;
        }
        self.status = Status::Checking;
        self.spawn(async {
            match latest().await {
                Ok(Some(release)) => Status::Available(release),
                Ok(None) => Status::Current,
                Err(error) => {
                    log::warn!("unable to check for updates: {error:#}");
                    Status::Failed {
                        release: None,
                        error: format!("{error:#}"),
                    }
                }
            }
        });
    }

    /// Downloads the release found and puts it in place of this program.
    pub fn install(&mut self) {
        let release = match &self.status {
            Status::Available(release)
            | Status::Failed {
                release: Some(release),
                ..
            } if release.installable() => release.clone(),
            _ => return,
        };
        // Found now: once replaced, Linux names the running program as
        // deleted.
        let program = match std::env::current_exe() {
            Ok(program) => program,
            Err(error) => {
                self.status = Status::Failed {
                    release: Some(release),
                    error: format!("unable to find Spotless's program: {error}"),
                };
                return;
            }
        };
        self.program = Some(program.clone());
        self.status = Status::Installing(release.clone());
        self.spawn(async move {
            match install(&release, &program).await {
                Ok(()) => {
                    log::info!("installed Spotless {}", release.version);
                    Status::Installed(release)
                }
                Err(error) => {
                    log::warn!("unable to update: {error:#}");
                    Status::Failed {
                        release: Some(release),
                        error: format!("{error:#}"),
                    }
                }
            }
        });
    }

    /// The program to restart into, once an update is in place.
    pub fn restart_into(&self) -> Option<&Path> {
        match self.status {
            Status::Installed(_) => self.program.as_deref(),
            _ => None,
        }
    }

    fn spawn(&mut self, work: impl Future<Output = Status> + Send + 'static) {
        let sender = self.sender.clone();
        let wake = Arc::clone(&self.wake);
        let spawned = std::thread::Builder::new()
            .name("spotless-update".into())
            .spawn(move || {
                let status = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime.block_on(work),
                    Err(error) => Status::Failed {
                        release: None,
                        error: error.to_string(),
                    },
                };
                let _ = sender.send(status);
                wake();
            });
        if let Err(error) = spawned {
            self.status = Status::Failed {
                release: None,
                error: error.to_string(),
            };
        }
    }
}

/// Starts the updated program at `program`, as this one quits.
pub fn restart(program: &Path) {
    if let Err(error) = std::process::Command::new(program).arg(RESTARTED).spawn() {
        log::warn!("unable to start {}: {error}", program.display());
    }
}

/// Removes the program an update put aside on Windows, which ran until
/// this start.
pub fn clean_up() {
    #[cfg(windows)]
    if let Ok(program) = std::env::current_exe() {
        let _ = std::fs::remove_file(put_aside(&program));
    }
}

/// Where an update on Windows moves the running program, which can be
/// renamed but not overwritten.
#[cfg(windows)]
fn put_aside(program: &Path) -> PathBuf {
    program.with_extension("old.exe")
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    html_url: String,
    #[serde(default)]
    assets: Vec<GitHubAsset>,
}

#[derive(Deserialize)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
}

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        // GitHub's API turns away requests without one.
        .user_agent(concat!("Spotless/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(300))
        .build()?)
}

/// The newest desktop release for this platform, if newer than this
/// Spotless.
async fn latest() -> Result<Option<Release>> {
    let Some((package, _)) = PLATFORM else {
        return Ok(None);
    };
    let repository = REPOSITORY.trim_start_matches("https://github.com/");
    let url = format!("https://api.github.com/repos/{repository}/releases?per_page=30");
    let releases: Vec<GitHubRelease> = client()?
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(newest(releases, Version::current(), package))
}

/// Among `releases`, the newest desktop one newer than `current` with
/// `package`. Drafts and pre-releases are passed over.
fn newest(releases: Vec<GitHubRelease>, current: Version, package: &str) -> Option<Release> {
    releases
        .into_iter()
        .filter(|release| !release.draft && !release.prerelease)
        .filter_map(|release| {
            let version = Version::parse(release.tag_name.strip_prefix("desktop-v")?)?;
            let name = format!("spotless-{version}-{package}");
            let url = |name: &str| {
                release
                    .assets
                    .iter()
                    .find(|asset| asset.name == name)
                    .map(|asset| asset.browser_download_url.clone())
            };
            Some(Release {
                version,
                page: release.html_url.clone(),
                package: (name.clone(), url(&name)?),
                sums: url(SUMS),
            })
        })
        .filter(|release| release.version > current)
        .max_by_key(|release| release.version)
}

async fn download(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let response = client.get(url).send().await?.error_for_status()?;
    Ok(response.bytes().await?.to_vec())
}

async fn install(release: &Release, program: &Path) -> Result<()> {
    let Some((_, inside)) = PLATFORM else {
        bail!("Spotless doesn't update itself here");
    };
    let (name, url) = &release.package;
    let Some(sums) = &release.sums else {
        bail!("its release has no {SUMS} to check it against");
    };
    let client = client()?;
    let sums = download(&client, sums).await?;
    let expected = checksum(&String::from_utf8_lossy(&sums), name)
        .with_context(|| format!("{SUMS} doesn't list {name}"))?;
    let package = download(&client, url).await?;
    if format!("{:x}", Sha256::digest(&package)) != expected {
        bail!("{name} isn't the file its release lists");
    }
    let bytes = if name.ends_with(".zip") {
        unzip(&package, inside)?
    } else {
        untar_gz(&package, inside)?
    };
    replace(program, &bytes, release.version)
        .with_context(|| format!("unable to replace {}", program.display()))
}

/// `name`'s checksum in `sums`, in lower case, as `sha256sum` writes them.
fn checksum(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.split_once(char::is_whitespace)?;
        // A star before the name: read in binary mode.
        let file = file.trim_start().trim_start_matches('*');
        (file == name).then(|| hash.to_ascii_lowercase())
    })
}

/// The file at `path` in a zip archive.
fn unzip(package: &[u8], path: &str) -> Result<Vec<u8>> {
    let archive = crate::skin::zip::Archive::parse(package)?;
    let entry = archive
        .entries()
        .iter()
        .find(|entry| entry.name == path)
        .with_context(|| format!("the package has no {path}"))?;
    Ok(archive.read(entry)?)
}

/// The file at `path` in a gzipped tar archive.
fn untar_gz(package: &[u8], path: &str) -> Result<Vec<u8>> {
    let mut tar = Vec::new();
    flate2::read::GzDecoder::new(package)
        .take(UNPACKED_LIMIT)
        .read_to_end(&mut tar)
        .context("the package can't be unpacked")?;
    untar(&tar, path)
        .map(<[u8]>::to_vec)
        .with_context(|| format!("the package has no {path}"))
}

/// The regular file at `path` in a tar archive: the headers are read one
/// after another, each followed by its file's blocks.
fn untar<'a>(tar: &'a [u8], path: &str) -> Option<&'a [u8]> {
    const BLOCK: usize = 512;
    let text = |field: &[u8]| {
        let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
        String::from_utf8_lossy(&field[..end]).into_owned()
    };
    let mut at = 0;
    while let Some(header) = tar.get(at..at + BLOCK) {
        // Two empty blocks end the archive.
        if header.iter().all(|&b| b == 0) {
            return None;
        }
        let size = usize::from_str_radix(text(&header[124..136]).trim(), 8).ok()?;
        let mut name = text(&header[..100]);
        // POSIX archives may split a long path; GNU ones keep other things
        // there.
        if &header[257..263] == b"ustar\0" {
            let prefix = text(&header[345..500]);
            if !prefix.is_empty() {
                name = format!("{prefix}/{name}");
            }
        }
        let start = at + BLOCK;
        let regular = matches!(header[156], b'0' | 0);
        if regular && name.trim_start_matches("./") == path {
            return tar.get(start..start.checked_add(size)?);
        }
        at = start.checked_add(size.div_ceil(BLOCK) * BLOCK)?;
    }
    None
}

/// Puts `bytes` in place of the program at `program`, which runs on from
/// the file it started from.
#[cfg(unix)]
fn replace(program: &Path, bytes: &[u8], _version: Version) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;

    let temporary = program.with_file_name(".spotless-update");
    let _ = std::fs::remove_file(&temporary);
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            file.set_permissions(std::fs::Permissions::from_mode(0o755))?;
            file.sync_all()
        })
        .and_then(|()| std::fs::rename(&temporary, program));
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    Ok(written?)
}

/// Puts `bytes` in place of the program at `program`: Windows lets the
/// running one be renamed, not overwritten, so it is put aside until the
/// next start.
#[cfg(windows)]
fn replace(program: &Path, bytes: &[u8], version: Version) -> Result<()> {
    let aside = put_aside(program);
    let _ = std::fs::remove_file(&aside);
    std::fs::rename(program, &aside)?;
    if let Err(error) = std::fs::write(program, bytes) {
        let _ = std::fs::remove_file(program);
        let _ = std::fs::rename(&aside, program);
        return Err(error.into());
    }
    record_version(program, version);
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn replace(_program: &Path, _bytes: &[u8], _version: Version) -> Result<()> {
    bail!("Spotless doesn't update itself here")
}

/// The version Settings › Apps shows, when install.cmd listed Spotless
/// there from this folder.
#[cfg(windows)]
fn record_version(program: &Path, version: Version) {
    use std::os::windows::process::CommandExt;

    const KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Spotless";
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let reg = |args: &[&str]| {
        std::process::Command::new("reg")
            .args(args)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()
            .filter(|output| output.status.success())
    };
    let Some(folder) = program.parent() else {
        return;
    };
    let folder = folder.to_string_lossy().to_lowercase();
    let listed = reg(&["query", KEY, "/v", "InstallLocation"]).is_some_and(|output| {
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .any(|line| line.trim().to_lowercase().ends_with(&folder))
    });
    if listed {
        let version = version.to_string();
        reg(&["add", KEY, "/f", "/v", "DisplayVersion", "/d", &version]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, assets: &[&str]) -> GitHubRelease {
        GitHubRelease {
            tag_name: tag.into(),
            draft: false,
            prerelease: false,
            html_url: format!("https://github.com/nix-codes/spotless/releases/tag/{tag}"),
            assets: assets
                .iter()
                .map(|name| GitHubAsset {
                    name: name.to_string(),
                    browser_download_url: format!("https://example.com/{name}"),
                })
                .collect(),
        }
    }

    #[test]
    fn versions_compare_by_number() {
        let v = |text| Version::parse(text).unwrap();
        assert!(v("1.10.0") > v("1.9.3"));
        assert!(v("2.0.0") > v("1.99.99"));
        assert_eq!(v("1.2.3").to_string(), "1.2.3");
        assert_eq!(Version::parse("1.2"), None);
        assert_eq!(Version::parse("1.2.3.4"), None);
        assert_eq!(Version::parse("1.2.x"), None);
    }

    #[test]
    fn the_newest_desktop_release_with_this_package_is_picked() {
        let package = "linux-x64.tar.gz";
        let releases = vec![
            release("android-v9.0.0", &["spotless-9.0.0.apk", SUMS]),
            release("desktop-v1.3.0", &["spotless-1.3.0-windows-x64.zip", SUMS]),
            release("desktop-v1.2.0", &["spotless-1.2.0-linux-x64.tar.gz", SUMS]),
            GitHubRelease {
                draft: true,
                ..release("desktop-v1.4.0", &["spotless-1.4.0-linux-x64.tar.gz", SUMS])
            },
        ];
        let found = newest(releases, Version(1, 1, 0), package).unwrap();
        assert_eq!(found.version, Version(1, 2, 0));
        assert_eq!(found.package.0, "spotless-1.2.0-linux-x64.tar.gz");
        assert_eq!(
            found.sums.as_deref(),
            Some("https://example.com/SHA256SUMS")
        );
        assert!(found.installable());
        assert!(found.page.ends_with("/desktop-v1.2.0"));
    }

    #[test]
    fn a_release_without_checksums_is_found_but_not_installable() {
        let releases = vec![
            release("desktop-v1.2.0", &["spotless-1.2.0-linux-x64.tar.gz", SUMS]),
            release("desktop-v1.2.5", &["spotless-1.2.5-linux-x64.tar.gz"]),
        ];
        let found = newest(releases, Version(1, 1, 0), "linux-x64.tar.gz").unwrap();
        assert_eq!(found.version, Version(1, 2, 5));
        assert!(!found.installable());
    }

    #[test]
    fn nothing_is_found_when_this_spotless_is_the_newest() {
        let releases = vec![release(
            "desktop-v1.1.0",
            &["spotless-1.1.0-linux-x64.tar.gz", SUMS],
        )];
        assert_eq!(newest(releases, Version(1, 1, 0), "linux-x64.tar.gz"), None);
    }

    #[test]
    fn checksums_are_read_as_sha256sum_writes_them() {
        let sums = "ABC123  spotless-1.2.0-linux-x64.tar.gz\n\
                    def456 *spotless-1.2.0-windows-x64.zip\n";
        assert_eq!(
            checksum(sums, "spotless-1.2.0-linux-x64.tar.gz").as_deref(),
            Some("abc123")
        );
        assert_eq!(
            checksum(sums, "spotless-1.2.0-windows-x64.zip").as_deref(),
            Some("def456")
        );
        assert_eq!(checksum(sums, "spotless-1.2.0.apk"), None);
    }

    /// A tar archive as GNU tar writes one, with a folder and its files.
    fn tar(entries: &[(&str, &[u8], u8)]) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, bytes, kind) in entries {
            let mut header = [0u8; 512];
            header[..name.len()].copy_from_slice(name.as_bytes());
            let size = format!("{:011o}\0", bytes.len());
            header[124..136].copy_from_slice(size.as_bytes());
            header[156] = *kind;
            header[257..265].copy_from_slice(b"ustar  \0");
            out.extend_from_slice(&header);
            out.extend_from_slice(bytes);
            out.resize(out.len().div_ceil(512) * 512, 0);
        }
        out.extend_from_slice(&[0; 1024]);
        out
    }

    #[test]
    fn the_program_is_found_in_a_tar_archive() {
        let program = vec![7u8; 1300];
        let archive = tar(&[
            ("Spotless/", b"", b'5'),
            ("Spotless/README.txt", b"read me", b'0'),
            ("Spotless/spotless", &program, b'0'),
            ("Spotless/install.sh", b"#!/bin/sh", b'0'),
        ]);
        assert_eq!(untar(&archive, "Spotless/spotless"), Some(&program[..]));
        assert_eq!(
            untar(&archive, "Spotless/README.txt"),
            Some(&b"read me"[..])
        );
        assert_eq!(untar(&archive, "Spotless/missing"), None);
        assert_eq!(untar(&archive[..700], "Spotless/spotless"), None);
    }

    #[test]
    fn the_program_is_found_in_a_gzipped_package() {
        use std::io::Write;

        let archive = tar(&[("Spotless/spotless", b"program", b'0')]);
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gzip.write_all(&archive).unwrap();
        let package = gzip.finish().unwrap();
        assert_eq!(untar_gz(&package, "Spotless/spotless").unwrap(), b"program");
        assert!(untar_gz(b"not gzip", "Spotless/spotless").is_err());
    }

    #[test]
    fn the_program_is_found_in_a_zip_package() {
        let package = crate::skin::zip::write(&[
            ("Spotless/README.txt", b"read me", false),
            ("Spotless/Spotless.exe", b"program", true),
        ]);
        assert_eq!(
            unzip(&package, "Spotless/Spotless.exe").unwrap(),
            b"program"
        );
        assert!(unzip(&package, "Spotless/missing.exe").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn the_program_is_replaced_and_stays_runnable() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!("spotless-update-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let program = dir.join("spotless");
        std::fs::write(&program, b"old").unwrap();
        replace(&program, b"new", Version(1, 2, 0)).unwrap();
        assert_eq!(std::fs::read(&program).unwrap(), b"new");
        let mode = std::fs::metadata(&program).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111);
        assert!(!dir.join(".spotless-update").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
