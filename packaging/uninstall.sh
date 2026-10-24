#!/bin/sh
# Removes what install.sh installed. Settings, playlists and the
# sign-in in ~/.config/spotless are kept.
set -eu

data=${XDG_DATA_HOME:-$HOME/.local/share}
rm -f "$HOME/.local/bin/spotless" \
    "$data/applications/spotless.desktop" \
    "$data/icons/hicolor/scalable/apps/spotless.svg" \
    "$data/icons/hicolor/256x256/apps/spotless.png"
update-desktop-database "$data/applications" 2>/dev/null || true
gtk-update-icon-cache -q -t "$data/icons/hicolor" 2>/dev/null || true
echo "Removed Spotless (your settings and playlists are kept)"
