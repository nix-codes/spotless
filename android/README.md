# Spotless for Android

The Android app: Kotlin and Compose screens over the Rust core, which
`ffi/` (spotless-ffi) builds as `libspotless_ffi.so` with UniFFI bindings.
Gradle builds both: the library with cargo-ndk, then the bindings off it.

## Build

Needs the Android SDK (`platforms;android-37.0`, `build-tools;37.0.0`,
`ndk;29.0.14206865`) with `sdk.dir` in `local.properties`, a JDK 17 or
later, `cargo install cargo-ndk` and `rustup target add aarch64-linux-android`.

```sh
./gradlew assembleDebug     # app/build/outputs/apk/debug/app-debug.apk
./gradlew installDebug      # onto the connected phone
adb logcat -s spotless      # the Rust side's log
```
