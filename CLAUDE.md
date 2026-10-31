# Spotless

A minimal Spotify player in the spirit of Winamp, as two apps on one Rust
core. README.md is for people using it: what it is, how to install it,
first start, credits. docs/screenshots.md holds the rest of the
screenshots and docs/about.md the longer story (why, every feature, your
data, status). DEVELOPMENT.md is building, checks and releasing;
android/README.md is building the Android app.

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

## Architecture

How the parts fit, to keep to as features are added:

- The core has no interface. An app reads the controller's `State`,
  hands it `Action`s and calls `receive` and `tick` when woken; it never
  talks to the backend (`Command` and `Event` are the core's own).
- The controller is a file per part under `core/src/controller/`
  (playback, playlists, browse, sync…); `mod.rs` only routes. An
  action is of its part's kind (`PlaybackAction`… in `action.rs`), and
  each part's `apply_*` names every one of its kind, with no catch-all;
  `receive` names every event the same way. A new action goes in its
  part's kind and file, a new event in the router and its part's file,
  tests in `controller/tests/` beside the others. What the Spotify tab
  or a song waiting offline takes from another part is the first thing
  that part's `apply_*` does.
- Rules that decide something (what plays next, how a sync merges, when
  to reconnect) are plain code that changes the model and returns what to
  do, like `Transport` and `Retry`, tested without a network or a clock.
  The worker (`backend/worker/`, a file per part) only carries them out.
- No process-wide mutable state: what the controller and the audio share
  is handed to them (`AudioLink`), so each controller, test and
  screenshot has its own.
- The core's `Settings` holds what both apps use; an app's own settings
  are its own type, kept in the same file through `Settings::extra`
  (the desktop's: `src/settings.rs`).
- A new core action must be named in `android/ffi`'s `on_android`, as
  one the app has or not yet; the tests don't compile otherwise. The
  FFI's types are shaped for Kotlin, not copied from the core.
- The Spotify tab's Connect device is built only with the `spotify-tab`
  feature; CI checks it builds and passes both ways.
- What the apps don't use stays private to the core, and no public type
  is left unnameable outside it (`unnameable_types` warns). An app names
  what it uses as `spotless_core::<topic>::<Item>`, never deeper: a
  topic module (`playback`, `spotify`, `lastfm`, `settings`…) re-exports
  what the apps use, and its submodules are the core's own (`internal!`
  in `lib.rs`).

## Checks

What CI runs (`.github/workflows/build.yml`), to run before committing:

```sh
cargo fmt --all --check
cargo clippy --workspace --release --all-targets -- -D warnings
cargo test --workspace --release
```

The Spotify tab is built only with the `spotify-tab` feature, off by
default; CI also runs clippy and the tests with it
(`--features spotless-core/spotify-tab`). The core's live probes
(`core/examples`) reach its internals, built public only with the
`probes` feature; CI checks they build
(`cargo clippy -p spotless-core --release --examples --features spotify-tab,probes`).

For the Android app: `cd android && ./gradlew assembleDebug`, and its
`ScreenshotTest` on the core built for this computer:
`cargo build --release -p spotless-ffi`, then
`cd android && ./gradlew testDebugUnitTest --tests dev.spotless.ScreenshotTest`.

## Commits

The subject is one plain sentence saying what changed for the user, no
full stop, prefixed `Desktop:` or `Android:` when it's for one app only.
The body, when there is one, says why and how, in a short paragraph.

## Words

- **context**: the album, playlist or artist Spotify plays from.
- **queue**: only the songs added with Add to queue.
- **Spotify tab**: Spotless's whole view of Spotify's playback (now
  playing, queue, context). Never "the Spotify queue".

## Screenshots

The screenshots (the desktop's and the Android app's side by side at
the top of the README, the rest in `docs/screenshots.md`) are drawn, never taken by hand, from the core's
made-up library (`core/src/demo.rs`, `Controller::demo`):
`packaging/screenshots.sh` runs `examples/screenshots.rs` for the desktop
and the Android app's `ScreenshotTest` (Robolectric, on the core built for
this computer), and lists the files that changed. The `/screenshots` skill
(`.claude/skills/screenshots/`) runs it and checks the results; a desktop
or Android release runs it too.

## Releases

Each app is released on its own, from `main`: `packaging/release.sh`
(see its header) tags `desktop-v<x.y.z>` or `android-v<x.y.z>` and the
release workflows build it. The `/release` skill
(`.claude/skills/release/`) sorts the changes by app and drafts the notes
with the `release-notes` agent (`.claude/agents/`). It suggests each
app's version; the user confirms it before anything is released.
