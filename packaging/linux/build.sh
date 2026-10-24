#!/bin/sh
# Builds Spotless for Linux and packs it for handing out:
#   target/dist/spotless-<version>-linux-x64.tar.gz
# A Spotless folder with spotless, install.sh, uninstall.sh, the icons and
# launcher they install, README.txt and LICENSE in it.
set -eu

root=$(cd "$(dirname "$0")/../.." && pwd)
export PATH="$HOME/.cargo/bin:$PATH"

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)
cargo build --release --manifest-path "$root/Cargo.toml"

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
dir=$stage/Spotless
mkdir -p "$dir/icons" "$dir/applications"
install -m755 "$root/target/release/spotless" "$dir/spotless"
install -m755 "$root/packaging/install.sh" "$root/packaging/uninstall.sh" "$dir/"
cp "$root/packaging/icons/spotless.svg" "$root/packaging/icons/spotless-256.png" "$dir/icons/"
cp "$root/packaging/applications/spotless.desktop" "$dir/applications/"
sed "s/@VERSION@/$version/g" "$root/packaging/linux/README.txt" > "$dir/README.txt"
cp "$root/LICENSE" "$dir/LICENSE"

out="$root/target/dist/spotless-$version-linux-x64.tar.gz"
mkdir -p "$root/target/dist"
tar -czf "$out" -C "$stage" Spotless
echo "Built $out"
