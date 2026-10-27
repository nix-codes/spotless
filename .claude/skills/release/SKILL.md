---
name: release
description: Prepares and makes a Spotless release, of the desktop app, the Android app or both. Sorts the changes since each app's last release by platform, has the release-notes agent draft notes for each app, and releases through packaging/release.sh once the user has confirmed the version. Use when the user wants to release, asks what a release would contain, or wants release notes drafted. "/release notes" drafts the notes only.
---

# Releasing Spotless

The two apps are released on their own, each from `main` with its own tag:
`desktop-v<x.y.z>` (Linux and Windows) and `android-v<x.y.z>`.
`packaging/release.sh` does the releasing; this skill decides what to
release and writes the notes. Everything here works without you too: the
script alone releases with the commit subjects as notes.

Arguments: an app (`desktop` or `android`) to look at only that one;
`notes` to stop after the drafts, releasing nothing.

## 1. What changed, for which app

Run `packaging/release.sh changes`. For each app it lists the commits since
its last release, marked `own`, `shared` or both. If an app has nothing,
say so and leave it out.

Also follow the `screenshots` skill (`.claude/skills/screenshots/`) up to
its commit step, drawing only the released app's screenshots (`android`
for Android; `playlist browse phosphor settings deck` for the desktop), and
show what changed with the notes in step 3. Refreshed screenshots are
committed, with the user's say-so, before the release: it is made from
`main` as pushed.

## 2. Draft the notes

For each app with changes, start a `release-notes` agent (Agent tool,
`subagent_type: "release-notes"`) with a prompt naming the app: "Draft the
release notes for the desktop app." Start both in one message so they run
side by side.

## 3. Show the user

For each app: the agent's **Version** line (e.g. `minor: 1.0.0 →
1.1.0`; work it out yourself if missing) and **Worth a release?** line,
the notes, and the
left-out commits in one line each. Check the drafts first: an entry
claiming something the commits don't do is worse than a missing one.
When both apps are drafted, a shared change should read the same and sit
in the same section in both; settle any difference before showing them.

Then recommend which apps to release.

With `notes`, write each app's notes to a file in the scratchpad and
rehearse its release with the suggested version, which tags and pushes
nothing:

```sh
packaging/release.sh <app> <x.y.z> --notes <file> --dry-run
```

Show the user, in your reply, each app's notes in full as the dry run
printed them, in a fenced `markdown` block: never only the files' paths.
Then what it would stop on, if anything, and stop here.

## 4. Release, once the user says so

Ask, for each app being released:

- **The version.** Offer the suggested one; release only with a version
  the user has confirmed or given.
- **Draft or published.** A draft is there to try the files first; a
  published one is public at once. For the desktop, publishing is also
  what makes every installed Spotless offer it as an update (drafts are
  invisible to them), so a draft is the safer choice when the release
  changes anything risky.

Also ask for changes to the notes, then write each app's final notes to its
own file in the scratchpad, e.g. `<scratchpad>/notes-android.md`.

Before running the script, `main` must be pushed and CI green on it: the
script checks both and stops if not. Pushing `main` needs the user's say
so, like any push; the release itself is the push of a tag, which the
user's go-ahead in this step covers.

Run, in the background (it waits for the build, which takes minutes):

```sh
packaging/release.sh <app> <x.y.z> --notes <file> [--publish] --yes
```

When it's done, give the user the release's link, and for a draft the
command to publish it: `gh release edit <tag> --draft=false`. If the
script warned that a desktop release has no `SHA256SUMS`, say so: Spotless
will only point at that release's page, not install it.

If the build fails, give the log's last error lines
(`gh run view <run> --log-failed`). Don't delete the tag or the release
without asking.
