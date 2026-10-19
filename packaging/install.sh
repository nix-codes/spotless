#!/bin/sh
# Builds Spotless and installs it for the current user:
#   ~/.local/bin/spotless
#   ~/.local/share/applications/spotless.desktop
#   ~/.local/share/icons/hicolor/{scalable,256x256}/apps/spotless.{svg,png}
# Run it again to update. packaging/uninstall.sh removes the same files.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
data=${XDG_DATA_HOME:-$HOME/.local/share}
bin=$HOME/.local/bin
export PATH="$HOME/.cargo/bin:$PATH"

cargo build --release --manifest-path "$root/Cargo.toml"

install -Dm755 "$root/target/release/spotless" "$bin/spotless"
install -Dm644 "$root/packaging/icons/spotless.svg" \
    "$data/icons/hicolor/scalable/apps/spotless.svg"
install -Dm644 "$root/packaging/icons/spotless-256.png" \
    "$data/icons/hicolor/256x256/apps/spotless.png"
# An absolute Exec, because ~/.local/bin is not on every session's PATH.
mkdir -p "$data/applications"
sed "s|^Exec=spotless|Exec=$bin/spotless|" \
    "$root/packaging/applications/spotless.desktop" \
    > "$data/applications/spotless.desktop"

update-desktop-database "$data/applications" 2>/dev/null || true
gtk-update-icon-cache -q -t "$data/icons/hicolor" 2>/dev/null || true
echo "Installed Spotless to $bin/spotless"
