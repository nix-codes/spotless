#!/bin/sh
# Installs Spotless for the current user:
#   ~/.local/bin/spotless
#   ~/.local/share/applications/spotless.desktop
#   ~/.local/share/icons/hicolor/{scalable,256x256}/apps/spotless.{svg,png}
# From a release's folder, it installs the spotless beside it; from the
# source, it builds Spotless first. Run it again to update.
# uninstall.sh removes the same files.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
if [ -f "$here/spotless" ]; then
    program=$here/spotless
    files=$here
else
    root=$(cd "$here/.." && pwd)
    export PATH="$HOME/.cargo/bin:$PATH"
    cargo build --release --manifest-path "$root/Cargo.toml"
    program=$root/target/release/spotless
    files=$root/packaging
fi
data=${XDG_DATA_HOME:-$HOME/.local/share}
bin=$HOME/.local/bin

install -Dm755 "$program" "$bin/spotless"
install -Dm644 "$files/icons/spotless.svg" \
    "$data/icons/hicolor/scalable/apps/spotless.svg"
install -Dm644 "$files/icons/spotless-256.png" \
    "$data/icons/hicolor/256x256/apps/spotless.png"
# An absolute Exec, because ~/.local/bin is not on every session's PATH.
mkdir -p "$data/applications"
sed "s|^Exec=spotless|Exec=$bin/spotless|" \
    "$files/applications/spotless.desktop" \
    > "$data/applications/spotless.desktop"

update-desktop-database "$data/applications" 2>/dev/null || true
gtk-update-icon-cache -q -t "$data/icons/hicolor" 2>/dev/null || true
echo "Installed Spotless to $bin/spotless"
