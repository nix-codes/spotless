#!/bin/sh
# Builds Spotless for macOS, on a Mac, and packs it for handing out:
#   target/dist/spotless-<version>-macos.zip
# A Spotless folder with Spotless.app, README.txt and LICENSE in it. The
# app holds one program for both Apple silicon and Intel Macs.
#
# Needs Xcode's command line tools, and the aarch64-apple-darwin and
# x86_64-apple-darwin Rust targets. The app is signed only ad hoc (no
# Apple account), as Apple silicon needs to run it at all.
set -eu

root=$(cd "$(dirname "$0")/../.." && pwd)
export PATH="$HOME/.cargo/bin:$PATH"

# The oldest macOS it runs on: the oldest Apple silicon shipped with.
minimum=11.0
export MACOSX_DEPLOYMENT_TARGET=$minimum

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)
for target in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --release --target "$target" --manifest-path "$root/Cargo.toml"
done

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
app=$stage/Spotless/Spotless.app
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
lipo -create -output "$app/Contents/MacOS/spotless" \
    "$root/target/aarch64-apple-darwin/release/spotless" \
    "$root/target/x86_64-apple-darwin/release/spotless"
sed -e "s/@VERSION@/$version/g" -e "s/@MINIMUM@/$minimum/g" \
    "$root/packaging/macos/Info.plist" > "$app/Contents/Info.plist"

# The icon, every size macOS asks for, from the 1024 px one.
icons=$stage/spotless.iconset
mkdir "$icons"
for size in 16 32 128 256 512; do
    sips -z $size $size "$root/packaging/icons/spotless-1024.png" \
        --out "$icons/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z $double $double "$root/packaging/icons/spotless-1024.png" \
        --out "$icons/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns -o "$app/Contents/Resources/spotless.icns" "$icons"

codesign --force --sign - "$app"

sed -e "s/@VERSION@/$version/g" -e "s/@MINIMUM@/$minimum/g" \
    "$root/packaging/macos/README.txt" > "$stage/Spotless/README.txt"
cp "$root/LICENSE" "$stage/Spotless/LICENSE"

out="$root/target/dist/spotless-$version-macos.zip"
mkdir -p "$root/target/dist"
rm -f "$out"
# ditto keeps the app's signature and attributes, as Finder's own zip does.
ditto -c -k --keepParent "$stage/Spotless" "$out"
echo "Built $out"
