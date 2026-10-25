#!/bin/sh
# Draws the desktop app's screenshots in docs/screenshots again, from
# made-up playlists and a made-up artist (examples/screenshots.rs), and
# lists those that changed. The same code always draws the same pixels,
# so a listed file means the interface looks different.
#   packaging/screenshots.sh
#   packaging/screenshots.sh browse phosphor-   only the files starting so
#
# The Android screenshot (docs/screenshots/android.png) is still taken by
# hand. Needs a GPU wgpu can draw with (Vulkan, Metal or DirectX 12), or
# Mesa's software renderer.
set -eu

cd "$(dirname "$0")/.."
cargo run --quiet --release --example screenshots -- docs/screenshots "$@" >/dev/null
changed=$(git status --porcelain -- docs/screenshots)
if [ -z "$changed" ]; then
    echo "The screenshots are up to date."
else
    echo "Changed:"
    echo "$changed"
fi
