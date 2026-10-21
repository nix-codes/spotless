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
