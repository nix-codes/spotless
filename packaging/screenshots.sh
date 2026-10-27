#!/bin/sh
# Draws the README's screenshots in docs/screenshots again, from the core's
# made-up library and artist (core/src/demo.rs), and lists those that
# changed. The same code always draws the same pixels, so a listed file
# means the interface looks different.
#   packaging/screenshots.sh
#   packaging/screenshots.sh browse android   only the files starting so
#
# The desktop's are drawn by examples/screenshots.rs, with a GPU wgpu can
# draw with (Vulkan, Metal or DirectX 12) or Mesa's software renderer,
# the deck's (deck-*.png) hearing a made-up moment of the song. The
# Android app's by its ScreenshotTest (android/app/src/test), on this
# computer with Robolectric, from the core built for it here; it needs the
# Android SDK the app builds with.
set -eu

cd "$(dirname "$0")/.."
out="$PWD/docs/screenshots"

# Whether files starting with $1 are asked for: all are without arguments.
wanted() {
    [ $# -eq 1 ] && return 0
    prefix=$1
    shift
    for name in "$@"; do
        case $prefix in "$name"*) return 0 ;; esac
        case $name in "$prefix"*) return 0 ;; esac
    done
    return 1
}

cargo run --quiet --release --example screenshots -- "$out" "$@" >/dev/null
if wanted android "$@"; then
    cargo build --quiet --release -p spotless-ffi
    (cd android && ./gradlew --quiet testDebugUnitTest \
        --tests dev.spotless.ScreenshotTest -Pscreenshots="$out")
fi

changed=$(git status --porcelain -- docs/screenshots)
if [ -z "$changed" ]; then
    echo "The screenshots are up to date."
else
    echo "Changed:"
    echo "$changed"
fi
