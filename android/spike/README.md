# Android spike

A throwaway app that answers one question: can Spotless's own core play
Spotify on Android? It signs in, plays one song, and keeps playing with the
screen off. It uses the desktop's `session.rs`, `engine.rs`, `sink.rs`,
`store.rs`, `paths.rs` and `model/` unchanged (`src/lib.rs` includes them by
path), with one screen and a foreground service in Java.

## Build and run

With the Android SDK in `~/Android/Sdk` (`platforms;android-36`,
`build-tools;36.0.0`, an NDK), `cargo install cargo-ndk`,
`rustup target add aarch64-linux-android`, and a phone with USB debugging:

```sh
android/spike/build.sh --install
adb logcat -s spotless   # the Rust side's log
```

## What it checks

1. The core builds for Android: librespot, rodio, cpal's AAudio backend.
2. TLS: rustls finds Android's certificate authorities (`SSL_CERT_DIR`).
3. Sign-in through the browser, back to the loopback port.
4. Audio out of the phone, at 320 kbps.
5. Playback going on with the screen off and the app in the background.

## What it found (Pixel 9a, Android 16, 2026-10-21)

All five work. Along the way:

- **Android freezes a background app within seconds** (Android 14 on).
  While the browser was in front for the sign-in, the frozen app never
  answered the redirect to its loopback port; with the screen off, it lost
  its network and Spotify dropped the session. A foreground service fixes
  both, so it runs during the sign-in as well as while playing.
- **librespot presents itself as an Android client on Android**: its
  `config::OS` constant switches the client ID, version, client-token
  device data and user agent. Spotify takes the login with the desktop
  client's credential, then closes the connection and refuses every request
  (`INVALID_CREDENTIALS`). `build.sh` patches the constant to `"linux"`.
- **Sessions drop on a phone**, and neither the spike at first nor the
  desktop backend reconnected. Play now reconnects with the saved
  credential when the session is gone.
- The sign-in page can't bring the app back: you switch to it yourself.

Then on battery and in deep Doze, forced with `adb shell dumpsys battery
unplug` and `dumpsys deviceidle force-idle` (undone with `battery reset`
and `deviceidle unforce`): with the screen locked, an album played on song
after song, each fetched over the network half a minute before the one
playing ended, without a gap.

- **`Engine` tells `Followed` holding its follow lock** (`engine.rs`,
  `match lock(&follow).take()`), so answering it with `Engine::follow` from
  the notify hook deadlocks. The desktop answers later, from the UI, but a
  port that reacts in the hook has to do it on another task.
- A forced Doze stays on after the phone is unlocked, network cut and all,
  until `deviceidle unforce`.

## Stopgaps the port would replace

- `SSL_CERT_DIR` pointed at the system's CAs: `rustls-platform-verifier`.
- stdout captured for librespot-oauth's "Browse to:" line: a way in the
  fork to hand the URL to the caller.
- librespot's `OS` constant patched in a local clone: the change carried in
  the fork, or an option for it.
- Java polls the status twice a second: callbacks or UniFFI.
- The sign-in page's "you can close this tab": a redirect to a link the
  app registers, which brings it forward.
