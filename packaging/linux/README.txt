Spotless @VERSION@ for Linux
A minimal Spotify player in the spirit of Winamp. It needs Spotify Premium.

Install
  Run ./install.sh. It installs Spotless for you only (no root needed):
  ~/.local/bin/spotless and a launcher with its icon.

Update
  Spotless says when a newer release is out and, once you agree, installs
  it (F10, About). Running install.sh from a newer release's folder updates
  it too.

  Or don't install it: ./spotless runs from this folder on its own.

  Choosing the audio output needs PipeWire or PulseAudio (pactl). The tray
  icon needs a panel that shows StatusNotifierItem icons (KDE and most
  others; on GNOME, the AppIndicator extension).

Uninstall
  Run ./uninstall.sh. Your settings, playlists and sign-in are kept in
  ~/.config/spotless; delete that folder to remove them too.

Keys
  F1 lists the keys, F10 opens the settings. Closing the window keeps
  Spotless playing in the tray; Ctrl+Q quits.
