---
name: screenshots
description: Draws the README's screenshots of both apps again from the core's made-up library (packaging/screenshots.sh), shows the user which changed and how, and checks the README shows exactly the files drawn. Use when the user wants the screenshots refreshed or checked, after interface changes, and as part of a release (the release skill runs it).
---

# The README's screenshots

The screenshots in `docs/screenshots/` are drawn from the core's made-up
library and artist (`core/src/demo.rs`, started by `Controller::demo`), so
nobody's account or library is in them:

- the desktop's by `examples/screenshots.rs`, the real interface drawn
  without a window; its `SHOTS` list says which files there are and what
  each shows. The `deck-*.png` ones show the deck (VIS) on each of its
  players; the example feeds the deck a made-up moment of the song
  (`demo::sound`) every frame, as the audio output would, so the
  spectrum and the levels show music rather than silence. The deck moves,
  but the harness steps a fixed time per frame, so it's drawn at the same
  moment every time;
- the Android app's (`android*.png`) by
  `android/app/src/test/java/dev/spotless/ScreenshotTest.kt`, the real
  Compose screens drawn on this computer with Robolectric and Roborazzi,
  on the core built for it; one test per file.

The README places them.

Arguments: file name starts (e.g. `browse android`) to draw only those.

## 1. Draw them

Run `packaging/screenshots.sh` (with the arguments, if any). It rewrites the
files and lists, as `git status` does, the ones that differ from the last
commit. The drawing is exact, so only a real change in the interface (or
in the demo's data) lists a file. If none is listed, say the screenshots
are up to date and go to step 3.

## 2. Look at what changed

For each listed file, Read the new image. For a modified one, also Read
the committed one, saved to the scratchpad with
`git show HEAD:docs/screenshots/<file> > <scratchpad>/old-<file>`, and
say in a line what is different (the selection's colour, a new button…).
Check each new image too: the screen it should show, nothing cut off or
missing, covers drawn in the same colours in both apps. In a deck shot:
the player it's named after, the song's names on it readable, and the
spectrum and the levels lit (all dark means the example's sound no
longer reaches the deck). A screenshot that
looks wrong points at the drawing code or at a real bug in the interface:
say which, don't commit it.

## 3. The README

Redrawing changes no text: the README shows the files by name, so a
redrawn one shows there as it is. The README needs editing only when the
files themselves change. Check that every image the README shows is in
`docs/screenshots/` and that every file there is shown in the README (or
it is drawn for nothing), and fix any difference. A file that's no
longer drawn is deleted.

If either app gained something a reader of the README would want to see
that no shot shows (a new player for the deck, say, which needs its own
`deck-*.png`), suggest adding one (to `SHOTS`, or a test in
`ScreenshotTest`) and a place for it in the README. Edit the README only
with the user's go-ahead. Keep its shape: one or two images at the top for
a first look, the rest under Screenshots, and at most two colours (green
and amber).

## 4. Commit, if the user wants

Commit only the screenshots (and the README or what draws them, if
changed with them), with a subject such as `README screenshots show the
new Spotify tab`. Push only when asked.
