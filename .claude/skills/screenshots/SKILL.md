---
name: screenshots
description: Draws the README's desktop screenshots again from made-up data (packaging/screenshots.sh), shows the user which changed and how, checks the README shows exactly the files drawn, and says whether the hand-taken Android one may be out of date. Use when the user wants the screenshots refreshed or checked, after interface changes, and as part of a desktop release (the release skill runs it).
---

# The README's screenshots

The desktop screenshots in `docs/screenshots/` are drawn by
`examples/screenshots.rs`: the real interface, without a window, from
made-up playlists and a made-up artist with drawn covers, so nobody's
account or library is in them. Its `SHOTS` list says which files there are
and what each shows; the README places them. `android.png` is still taken
by hand.

Arguments: file name starts (e.g. `browse phosphor-`) to draw only those.

## 1. Draw them

Run `packaging/screenshots.sh` (with the arguments, if any). It rewrites the
files and lists, as `git status` does, the ones that differ from the last
commit. The drawing is exact, so only a real change in the interface lists
a file. If none is listed, say the screenshots are up to date and go to
step 3.

## 2. Look at what changed

For each listed file, Read the new image. For a modified one, also Read
the committed one, saved to the scratchpad with
`git show HEAD:docs/screenshots/<file> > <scratchpad>/old-<file>`, and
say in a line what is different (the selection's colour, a new button…).
Check each new image too: the screen it should show, nothing cut off or
missing, covers drawn. A screenshot that looks wrong points at
`examples/screenshots.rs` or at a real bug in the interface: say which,
don't commit it.

## 3. The README

Redrawing changes no text: the README shows the files by name, so a
redrawn one shows there as it is. The README needs editing only when the
files themselves change. Check that every image the README shows is in
`docs/screenshots/` and that every file there is shown in the README (or
`SHOTS` draws it for nothing), and fix any difference. A file that's no
longer drawn is deleted.

If the interface gained something a reader of the README would want to
see that no shot shows, suggest adding one to `SHOTS` and a place for it
in the README. Edit the README only with the user's go-ahead. Keep its
shape: one or two images at the top for a first look, the rest under
Screenshots, and at most two colours (green and amber).

## 4. Android

The Android screenshot isn't drawn. Compare
`git log -1 --format=%h -- docs/screenshots/android.png` with the
Android interface's changes since then
(`git log --oneline <that commit>.. -- android/app/src/main/java/dev/spotless/ui`)
and, if any change what it looks like, say the screenshot may be out of
date: it needs taking by hand.

## 5. Commit, if the user wants

Commit only the screenshots (and the README or `SHOTS`, if changed with
them), with a subject such as `README screenshots show the new Spotify
tab`. Push only when asked.
