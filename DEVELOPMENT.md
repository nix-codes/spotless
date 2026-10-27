# Developing Spotless

[Back to Spotless](README.md)

Spotless is two apps on one Rust core:

- `core/` (spotless-core): everything both apps share: sign-in, playback,
  playlists, Browse, Last.fm and settings. It plays through a
  [librespot fork](https://github.com/nix-codes/librespot), pinned in
  `core/Cargo.toml`.
- `src/`: the desktop app (Linux and Windows), drawn with egui.
- `android/`: the Android app, in Kotlin and Compose; building it is in
  [android/README.md](android/README.md).

## Building the desktop app

```sh
cargo run --release
```

It needs [Rust](https://rustup.rs). The toolchain version is pinned in
`rust-toolchain.toml`; rustup picks it up on its own. On Linux, building needs the ALSA headers:

- Debian, Ubuntu: `sudo apt install build-essential pkg-config libasound2-dev`
- Fedora: `sudo dnf install gcc pkgconf-pkg-config alsa-lib-devel`
- Arch: `sudo pacman -S base-devel alsa-lib`

On Windows, building also needs the Visual Studio Build Tools. The Windows
zip can be built from Linux with `packaging/windows/build.sh`, using Rust's
`x86_64-pc-windows-gnullvm` target and
[llvm-mingw](https://github.com/mstorsjo/llvm-mingw).

### Installing from source on Linux

```sh
git clone https://github.com/nix-codes/spotless.git
cd spotless
packaging/install.sh
```

This installs `~/.local/bin/spotless` and a launcher with its icon, for
your user only. Run it again after `git pull` to update;
`packaging/uninstall.sh` removes it.

## Checks

What CI runs, to run before committing:

```sh
cargo fmt --all --check
cargo clippy --workspace --release --all-targets -- -D warnings
cargo test --workspace --release
```

For the Android app: `cd android && ./gradlew assembleDebug`.

## Screenshots

The screenshots in the README and in [Screenshots](docs/screenshots.md)
are drawn, never taken by hand, from the core's made-up library
(`core/src/demo.rs`), so nobody's account is in them.
`packaging/screenshots.sh` draws them again: the desktop's with
`examples/screenshots.rs` and the Android app's with its `ScreenshotTest`
(Robolectric), and lists the files that changed.

## Releasing

Each app is released from `main` with a tag: `android-v<version>` for the
Android app, `desktop-v<version>` for Linux and Windows together.

```sh
packaging/release.sh changes      # each app's changes since its last release
packaging/release.sh android 0.4.0
packaging/release.sh desktop 0.1.0 --notes notes.md --publish
packaging/release.sh desktop 0.1.0 --notes notes.md --dry-run  # tags nothing
```

The script checks that `main` matches GitHub's, that CI passed on it and
that the version goes up, then pushes the tag. GitHub Actions builds the app
(the APK signed with the release key in the repository's secrets; the Linux
tarball and the Windows zip) and makes the release, with the notes given in
`--notes` or else the commits since the last one. It stays a draft, to try
its files and publish by hand, unless `--publish` is given. The tag is the
only place the version is set.
