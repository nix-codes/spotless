# spotless-ffi

Spotless's core for the Android app: `libspotless_ffi.so`, with Kotlin
bindings made by [UniFFI](https://mozilla.github.io/uniffi-rs/).

Kotlin calls `dev.spotless.core.Native.init(context)` once (a plain JNI
function: logging, certificates, cpal's context), then makes one `Spotless`
with its app folders and a `Host`. It hands the `Spotless` `Action`s, and
reads `session()`, `player()`, `tabs()`, `playlist(id)`, `browse()` and
`settings()` again after each `Host.wake`, calling `update()` first, on
its own thread.

## Build

For the phone (with the NDK and `cargo install cargo-ndk`):

```sh
cargo ndk -t arm64-v8a -P 29 build -p spotless-ffi --release
```

The Kotlin bindings, from a build for this machine:

```sh
cargo build -p spotless-ffi
cargo run -p spotless-ffi --features bindgen --bin uniffi-bindgen -- \
    generate --library target/debug/libspotless_ffi.so --language kotlin \
    --out-dir <kotlin source folder>
```
