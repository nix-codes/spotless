#!/usr/bin/env bash
# Builds the spike's APK with the SDK's own tools (no Gradle), and installs
# it on the connected phone with --install.
#
# Needs ANDROID_HOME (default ~/Android/Sdk) with platforms;android-36,
# build-tools;36.0.0 and an NDK, cargo-ndk, and the aarch64-linux-android
# Rust target.
set -euo pipefail
cd "$(dirname "$0")"

ANDROID_HOME=${ANDROID_HOME:-$HOME/Android/Sdk}
export ANDROID_HOME
export ANDROID_NDK_HOME=${ANDROID_NDK_HOME:-$(ls -d "$ANDROID_HOME"/ndk/* | sort -V | tail -1)}
tools=$ANDROID_HOME/build-tools/36.0.0
platform=$ANDROID_HOME/platforms/android-36/android.jar
out=build

rm -rf "$out" && mkdir -p "$out/classes" "$out/apk/lib/arm64-v8a"

# librespot-core, patched to present itself as Linux on Android too: its
# OS constant otherwise switches to the Android client ID and device data,
# and Spotify refuses the desktop client's credential ("INVALID_CREDENTIALS").
# Cargo.toml's [patch] points here.
rev=d04729018248f11c954b50ff6a3405801ff5af08
if [ ! -d librespot ]; then
    git clone -q https://github.com/crmne/librespot librespot
    git -C librespot checkout -q "$rev"
    python3 - librespot/core/src/config.rs <<'EOF'
import sys
path = sys.argv[1]
text = open(path).read()
old = "pub const OS: &str = std::env::consts::OS;\n"
new = ('#[cfg(not(target_os = "android"))]\n' + old +
       '/// Spotless: Android presents itself as Linux, the desktop client its\n'
       '/// credential is for.\n'
       '#[cfg(target_os = "android")]\npub const OS: &str = "linux";\n')
assert old in text, "librespot's OS constant moved"
open(path, "w").write(text.replace(old, new))
EOF
fi

cargo ndk -t arm64-v8a -P 29 build --release
cp target/aarch64-linux-android/release/libspotless_spike.so "$out/apk/lib/arm64-v8a/"

javac --release 17 -nowarn -cp "$platform" -d "$out/classes" $(find java -name '*.java')
"$tools/d8" --release --min-api 29 --lib "$platform" --output "$out/apk" $(find "$out/classes" -name '*.class')

"$tools/aapt2" link -o "$out/unaligned.apk" -I "$platform" --manifest AndroidManifest.xml \
    --min-sdk-version 29 --target-sdk-version 35
(cd "$out/apk" && zip -q -r ../unaligned.apk classes.dex && zip -q -0 -r ../unaligned.apk lib)
"$tools/zipalign" -P 16 -f 4 "$out/unaligned.apk" "$out/aligned.apk"

# A throwaway signing key, made once.
keystore=$HOME/.android/spotless-spike.keystore
if [ ! -f "$keystore" ]; then
    mkdir -p "$(dirname "$keystore")"
    keytool -genkeypair -keystore "$keystore" -storepass android -keypass android \
        -alias spike -keyalg RSA -keysize 2048 -validity 10000 -dname CN=spotless-spike
fi
"$tools/apksigner" sign --ks "$keystore" --ks-pass pass:android --out "$out/spotless-spike.apk" "$out/aligned.apk"
echo "built $out/spotless-spike.apk"

if [ "${1:-}" = --install ]; then
    "$ANDROID_HOME/platform-tools/adb" install -r "$out/spotless-spike.apk"
    "$ANDROID_HOME/platform-tools/adb" shell am start -n dev.spotless.spike/.SpikeActivity
fi
