# About Spotless

[Back to Spotless](../README.md)

## The name

**Spotless** is *Spotify* with *less*:

- **Less Spotify**: less of Spotify's design, its screens, its feeds and its
  way of doing things.
- **Less**, as in less of everything: a minimal player with only what you use.
- And **spotless** as in clean and neat: an interface without the noise and
  clutter.

## Why

Spotify's own app has frustrated me for years:

- **It's slow.**
- **It's noisy.** It's cluttered with things I don't need or rarely use:
  artist promotions, recommendations, recent listens and more.
- **What I do use is hard to reach.** Listening to albums means going back and
  forth between screens, and coming back often loses the place you scrolled
  to.
- **It can't be navigated with the keyboard.** There are a few shortcuts,
  but getting around (searching, opening an artist, picking an album,
  adding it) takes the mouse.
- **The queue works against you.** It's one queue for all your devices, so
  a computer can't play its own without overwriting your phone's. Press Play
  on an album and the rest of its songs go behind your queue, where nothing
  clears them (someone made a silent track just for that). And you can't put
  an album first for a while without wrecking the queue you had.

Spotless is the player I wanted instead.

## Who it's for

- **People who miss Winamp**, or players like it (AIMP, foobar2000,
  MediaMonkey): one compact window, a playlist you build yourself, and a
  player that does what you tell it.
- **Anyone who wants a simpler Spotify**: fewer screens, nothing competing
  for attention, just finding music and playing it.
- **Album listeners**, who want an artist's albums in order and whole
  albums in their playlists, without the detours.
- **Keyboard users**: every action has a key.
- **Anyone using several devices** who wants this computer's music to stay
  separate from the phone's.

It isn't for you if you rely on Spotify's own playlists, recommendations
or podcasts, or if you don't have Premium: Spotless leaves those out, and
Spotify only allows playback in other apps for Premium accounts.

## Features

- **Local playlists in tabs.** As many as you like, saved between sessions. A
  *Default* playlist is always there. Rename a tab by double-clicking it;
  reorder songs and tabs by dragging.
- **Playlists aren't used up.** Playing a song doesn't remove it: each
  playlist has its own place, which you can move back and forth, and
  viewing another tab never changes what plays. At the end, playback stops
  and goes back to the top (or loops, with Repeat).
- **Play next.** Mark songs with `Q` to play them next, in order, without
  moving them. Marks clear once they've played.
- **Shuffle and Repeat**, where Previous goes back through what actually
  played.
- **Gapless playback** at 320 kbps, with optional volume levelling.
- **One stream at a time**, as Spotify allows. Start playing on your phone
  and Spotless pauses; press play in Spotless and it pauses your phone.
  Spotless watches your devices through Spotify Connect without showing up
  in their lists, and its own playback stays out of your account's queue.
- **The Spotify tab**, optional: your account's queue, as Spotify's own
  apps show it, with Spotless as a Spotify Connect device to play it on.
  What played, the song playing, then the songs queued and the rest of the
  album or playlist. Play a song of it to take the queue over; while another
  device holds it, the tab only shows it until you **TAKE OVER**, and the
  player's buttons control that device. Add songs from Browse, remove them,
  clear the queue, or copy its songs to a playlist. Close the tab and
  Spotless leaves Connect; *Settings > Playback* turns it off.
- **Browse**: search shows Artists, Albums and Songs separately. An artist's
  page is just the discography: All, Albums, Singles & EPs, Appears on;
  Albums first, oldest first, songs opening inline. **+ ADD** adds a whole
  album to the playlist shown alongside. Pasting a Spotify link adds it too.
- **Pics are optional**: album covers and artists' pictures. Off by default
  on the desktop, on by default on Android; either way, they show in the
  catalogue only. Playlists stay text.
- **Last.fm scrobbling**, optional, sent by Spotless itself. Spotify's
  Last.fm link only hears of what plays through Spotify Connect, which
  Spotless's playlists stay out of so they never take over your account's
  queue.
  Listens made offline are sent later. The same API key gives an artist's
  page **Similar**: the artists Last.fm's listeners play alongside it.
- **Made for the keyboard.** Everything works without the mouse: playback,
  moving between tabs and through playlists, searching and browsing an
  artist's albums, adding them, and Settings. F1 lists the keys.
- **Your desktop's controls**: media keys, the system's now-playing display
  (MPRIS on Linux, the media overlay on Windows), and a tray icon; closing
  the window keeps Spotless playing there.
- **Two looks**: Spotless Classic, and Winamp Classic, drawn with Winamp's
  own buttons, digits, font, seek bar and volume. Pick one in Settings >
  Display.
- **Monitor colours**: the songs, the time and the song's name in the green
  of old terminals, or in amber, paper-white, a hi-fi's fluorescent
  teal, ice blue, deep blue or purple.
- **An equaliser** (desktop, EQ or `Alt+G`), as Winamp's: a preamp and ten
  bands from 60 Hz to 16 kHz, ±12 dB each, with Winamp's presets and your
  own. It changes what Spotless plays, not another device's playback.
- **The deck** (desktop, VIS or `Alt+V`): the player's buttons become the
  keys of an '80s Walkman whose cassette turns as the song plays, of a
  reel-to-reel deck whose tape winds from one reel to the other, of a
  portable CD player whose disc holds the whole playlist, or of a
  turntable that changes the single for each song; click it to switch.
  Beside it, the music's spectrum and its left and right levels, as it
  plays.
- **Settings** (F10): account, search, Last.fm, audio output, volume levelling,
  interface size, skin and colour, tray, and the audio and cover caches.
- **One window, one copy**: starting Spotless again brings the running one
  forward.

## Last.fm

**Why an API account of your own?** Linking Last.fm in your Spotify
account doesn't help here: Spotify only scrobbles what plays through
Spotify Connect, and a player that joins Connect takes over your
account's playback, replacing the queue on your other devices. So
Spotless's playlists stay out of Connect and Spotless scrobbles them by
itself. To do that, Last.fm requires every app to sign its requests
with an API key and a shared secret; unlike Spotify, it has no sign-in
that works without a secret. A key built into Spotless would put that
secret in public source code, where anyone could copy and misuse it,
and every Spotless user would share its limits and lose scrobbling if
Last.fm revoked it. Your own API account is used only by your copy of
Spotless.

## Your data

Your settings and playlists stay on your computer:

| | Linux | Windows |
|---|---|---|
| Settings, playlists, sign-in | `~/.config/spotless` | `%APPDATA%\spotless` |
| Audio and cover caches | `~/.cache/spotless` | `%LOCALAPPDATA%\spotless` |

Uninstalling keeps these; delete the folders to remove them too.

## Status

Spotless is young. Linux is its main platform; the Windows build is new and
has only been tested under Wine and on GitHub's build machines so far. The
Android app is newer still, and has been tried on one phone.

Not there on purpose: Spotify's own playlists, podcasts, and anything
social. Maybe later: importing Spotify playlists.
