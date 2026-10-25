# Spotless

A minimal Spotify player in the spirit of Winamp, as two apps on one Rust
core. README.md says what it is and how to build and install it;
android/README.md does the same for the Android app.

## Layout

- `core/` (spotless-core): everything both apps share: sign-in, playback,
  playlists, Browse, Last.fm, settings, and the controller the apps drive
  with actions. Uses a librespot fork (nix-codes/librespot), pinned in
  `core/Cargo.toml`.
- `src/`: the desktop app (Linux and Windows), egui. `build.rs` and
  `packaging/` (except `packaging/android` and `packaging/release.sh`)
  belong to it too.
- `android/`: the Android app, Kotlin and Compose; `android/ffi`
  (spotless-ffi) exposes the core to it through UniFFI.

A change to `core/`, `Cargo.toml`, `Cargo.lock` or `rust-toolchain.toml`
reaches both apps.

## Checks

What CI runs (`.github/workflows/build.yml`), to run before committing:

```sh
cargo fmt --all --check
cargo clippy --workspace --release --all-targets -- -D warnings
cargo test --workspace --release
```

For the Android app: `cd android && ./gradlew assembleDebug`.

## Commits

The subject is one plain sentence saying what changed for the user, no
full stop, prefixed `Desktop:` or `Android:` when it's for one app only.
The body, when there is one, says why and how, in a short paragraph.

## Words

- **context**: the album, playlist or artist Spotify plays from.
- **queue**: only the songs added with Add to queue.
- **Spotify tab**: Spotless's whole view of Spotify's playback (now
  playing, queue, context). Never "the Spotify queue".

## Releases

Each app is released on its own, from `main`: `packaging/release.sh`
(see its header) tags `desktop-v<x.y.z>` or `android-v<x.y.z>` and the
release workflows build it. The `/release` skill
(`.claude/skills/release/`) sorts the changes by app and drafts the notes
with the `release-notes` agent (`.claude/agents/`). It suggests each
app's version; the user confirms it before anything is released.
