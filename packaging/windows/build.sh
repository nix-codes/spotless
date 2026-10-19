#!/bin/sh
# Builds Spotless for Windows, from Linux, and packs it for handing out:
#   target/dist/spotless-<version>-windows-x64.zip
# A Spotless folder with Spotless.exe, install.cmd, uninstall.cmd and
# README.txt in it.
#
# Needs the x86_64-pc-windows-gnullvm Rust target and llvm-mingw's bin
# folder on PATH, or LLVM_MINGW pointing at llvm-mingw (docs/PLAN.md).
set -eu

root=$(cd "$(dirname "$0")/../.." && pwd)
export PATH="$HOME/.cargo/bin:$PATH"
if [ -n "${LLVM_MINGW:-}" ]; then
    export PATH="$LLVM_MINGW/bin:$PATH"
fi
if ! command -v x86_64-w64-mingw32-clang >/dev/null; then
    echo "llvm-mingw isn't on PATH: set LLVM_MINGW to its folder" >&2
    exit 1
fi

target=x86_64-pc-windows-gnullvm
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)
cargo build --release --target "$target" --manifest-path "$root/Cargo.toml"

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir "$stage/Spotless"
cp "$root/target/$target/release/spotless.exe" "$stage/Spotless/Spotless.exe"
# The version filled in, and Windows line endings: cmd misreads LF ones.
for file in install.cmd uninstall.cmd README.txt; do
    sed -e "s/@VERSION@/$version/g" -e 's/$/\r/' \
        "$root/packaging/windows/$file" > "$stage/Spotless/$file"
done

out="$root/target/dist/spotless-$version-windows-x64.zip"
mkdir -p "$root/target/dist"
rm -f "$out"
(cd "$stage" && zip -qr "$out" Spotless)
echo "Built $out"
