#!/bin/sh
# Releases the Android app or the desktop one: tags main's latest commit
# android-v<version> or desktop-v<version> and pushes the tag, after
# checking that it is what GitHub has, that CI passed on it and that the
# version goes up. The Android Release workflow then builds and signs the
# APK, or the Desktop Release one builds for Linux and Windows
# (.github/workflows/), and puts it in a release: a draft to try and
# publish by hand, or published at once with --publish.
#   packaging/release.sh changes
#   packaging/release.sh android 0.4.0
#   packaging/release.sh desktop 0.1.0 --notes notes.md --publish
#   packaging/release.sh desktop 0.1.0 --notes notes.md --dry-run
#
#   changes        lists each app's commits since its last release, marked
#                  with what they touch: the app's own code or the shared
#                  core (and dependencies, which both apps build with)
#   --notes FILE   the release notes, in Markdown; without it they list the
#                  subjects of the commits since the last release
#   --publish      publishes the release once its files are up
#   --yes          asks nothing: releases, and waits for the build
#   --dry-run      runs the checks, warning rather than stopping, and shows
#                  the notes the release would have; tags and pushes nothing
#   notes <tag>    prints a tag's release notes (the release workflows use it)
#
# The notes and --publish travel in the tag's message, which the workflow
# reads. Needs gh, signed in with access to the repository, to release.
set -eu

cd "$(dirname "$0")/.."
fail() {
    echo "$*" >&2
    exit 1
}
usage() {
    fail "usage: $0 changes
       $0 android|desktop <x.y.z> [--notes FILE] [--publish] [--yes] [--dry-run]
         (y and z below 100)
       $0 notes <tag>"
}

# What each app is built from (this script isn't). Shared: the core and
# the workspace's dependencies and toolchain.
shared='core Cargo.toml Cargo.lock rust-toolchain.toml'
own_android='android'
own_desktop="src build.rs packaging :!packaging/android :!packaging/release.sh"

# The paths an app's changes are listed from.
paths() {
    eval "echo \$own_$1 $shared"
}

# The highest tag of an app's, or the highest below a version.
previous_tag() {
    if [ $# -eq 1 ]; then
        git tag --list "$1-v*" | sort -V | tail -n 1
    else
        { git tag --list "$1-v*"; echo "$1-v$2"; } | sort -uV |
            grep -B1 -x "$1-v$2" | head -n 1 | grep -vx "$1-v$2" || true
    fi
}

# Whether a commit touches the shared paths or an app's own; "own shared"
# for both.
touches() {
    commit=$1 kind=$2 own= common=
    for file in $(git diff-tree --no-commit-id --name-only -r "$commit"); do
        case $file in
        core/* | Cargo.toml | Cargo.lock | rust-toolchain.toml) common=shared ;;
        packaging/android/* | packaging/release.sh) ;;
        android/*) [ "$kind" = android ] && own=own ;;
        src/* | build.rs | packaging/*) [ "$kind" = desktop ] && own=own ;;
        esac
    done
    echo $own $common
}

changes() {
    git fetch -q --tags origin main 2>/dev/null || echo "(Couldn't reach GitHub: local tags only)"
    for kind in desktop android; do
        previous=$(previous_tag $kind)
        echo
        if [ -n "$previous" ]; then
            range=$previous..HEAD
            echo "$kind, since ${previous#"$kind"-v} ($previous):"
        else
            range=HEAD
            echo "$kind, never released:"
        fi
        # shellcheck disable=SC2046 # the paths are split on purpose
        commits=$(git log --no-merges --format=%h "$range" -- $(paths $kind))
        [ -n "$commits" ] || echo "  nothing"
        for commit in $commits; do
            printf '  %s %-11s %s\n' "$commit" "$(touches "$commit" $kind | tr ' ' +)" \
                "$(git log -1 --format=%s "$commit")"
        done
    done
}

# A tag's notes: the ones its message carries, or the commits since the
# app's previous release.
notes() {
    tag=$1 kind=${1%%-v*} version=${1#*-v}
    git rev-parse -q --verify "refs/tags/$tag" >/dev/null || fail "No tag $tag"
    previous=$(previous_tag "$kind" "$version")
    body=$(git tag --list --format='%(contents:body)' "$tag" | grep -vx 'Publish: yes' || true)
    [ -z "$previous" ] || printf '## Changes since %s\n\n' "${previous#"$kind"-v}"
    if [ -n "$(echo "$body" | tr -d '[:space:]')" ]; then
        printf '%s\n' "$body"
    elif [ -n "$previous" ]; then
        # shellcheck disable=SC2046
        git log --no-merges --format='- %s' "$previous..$tag" -- $(paths "$kind")
    fi
}

[ $# -ge 1 ] || usage
case $1 in
changes)
    [ $# -eq 1 ] || usage
    changes
    exit
    ;;
notes)
    [ $# -eq 2 ] || usage
    notes "$2"
    exit
    ;;
android)
    workflow=android-release.yml
    name="Spotless for Android"
    ;;
desktop)
    workflow=desktop-release.yml
    name="Spotless (desktop)"
    ;;
*) usage ;;
esac
kind=$1
[ $# -ge 2 ] && echo "$2" | grep -Eqx '[0-9]+\.[0-9]{1,2}\.[0-9]{1,2}' || usage
version=$2
shift 2
notes_file= publish= yes= dry=
while [ $# -gt 0 ]; do
    case $1 in
    --notes)
        [ $# -ge 2 ] || usage
        notes_file=$2
        shift
        ;;
    --publish) publish=y ;;
    --yes) yes=y ;;
    --dry-run) dry=y ;;
    *) usage ;;
    esac
    shift
done
if [ -n "$notes_file" ]; then
    [ -s "$notes_file" ] || fail "$notes_file is missing or empty"
fi
tag=$kind-v$version

# What would stop a release: in a dry run, only a warning.
check() {
    if [ -n "$dry" ]; then
        echo "Would stop: $*" >&2
    else
        fail "$*"
    fi
}

git fetch -q --tags origin main
[ "$(git branch --show-current)" = main ] || check "Not on main"
[ -z "$(git status --porcelain)" ] || check "There are uncommitted changes"
[ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] ||
    check "main isn't the same as GitHub's: push or pull first"
! git rev-parse -q --verify "refs/tags/$tag" >/dev/null || fail "$tag already exists"

# The previous release, by version; the new one must come after it.
previous=$(previous_tag "$kind")
if [ -n "$previous" ]; then
    highest=$(printf '%s\n%s\n' "${previous#"$kind"-v}" "$version" | sort -V | tail -n 1)
    [ "$highest" = "$version" ] || check "$version doesn't come after ${previous#"$kind"-v}"
fi

# Every Build run on this commit finished and passed.
commit=$(git rev-parse HEAD)
runs=$(gh run list --commit "$commit" --workflow build.yml --json status,conclusion \
    --jq '[.[] | "\(.status) \(.conclusion)"] | join("\n")') || runs=
[ -n "$runs" ] || check "CI hasn't run on $commit"
if [ -n "$runs" ] && echo "$runs" | grep -qv '^completed success$'; then
    check "CI hasn't passed on $commit (yet): see gh run list --commit $commit"
fi

# The tag's message: the name, the notes, and whether to publish.
message=$(mktemp)
trap 'rm -f "$message"' EXIT
{
    echo "$name $version"
    if [ -n "$notes_file" ]; then
        echo
        cat "$notes_file"
    fi
    if [ -n "$publish" ]; then
        echo
        echo "Publish: yes"
    fi
} > "$message"

# A dry run tags here only, to show the notes the release workflow would
# read off the tag, then takes the tag away again.
if [ -n "$dry" ]; then
    trap 'rm -f "$message"; git tag -d "$tag" >/dev/null 2>&1 || true' EXIT
    git tag -a --cleanup=verbatim -F "$message" "$tag"
    if [ -n "$publish" ]; then state=published; else state="a draft"; fi
    echo "The release would be $state, with these notes (and the downloads' checksums):"
    echo
    notes "$tag"
    exit
fi

if [ -n "$notes_file" ]; then
    echo "Release notes:"
    sed 's/^/  /' "$notes_file"
elif [ -n "$previous" ]; then
    echo "Changes since ${previous#"$kind"-v}, for the notes:"
    # shellcheck disable=SC2046
    git log --no-merges --format='  %h %s' "$previous..HEAD" -- $(paths "$kind")
else
    echo "The first release."
fi
if [ -n "$publish" ]; then state=published; else state="a draft"; fi
if [ -z "$yes" ]; then
    printf 'Release %s %s (%s)? [y/N] ' "$name" "$version" "$state"
    read -r answer
    [ "$answer" = y ] || fail "Not released"
fi

# Verbatim, or git would drop the notes' Markdown headings as comments.
git tag -a --cleanup=verbatim -F "$message" "$tag"
git push -q origin "$tag"
echo "Pushed $tag; the $workflow workflow builds it."

if [ -z "$yes" ]; then
    printf 'Wait for the release? [Y/n] '
    read -r answer
    [ "$answer" != n ] || exit 0
fi
# This commit's run: an earlier, deleted release may have had the same tag.
run=
for _ in 1 2 3 4 5 6 7 8 9 10; do
    run=$(gh run list --workflow "$workflow" --branch "$tag" --json databaseId,headSha \
        --jq "[.[] | select(.headSha == \"$commit\")][0].databaseId // empty")
    [ -z "$run" ] || break
    sleep 3
done
[ -n "$run" ] || fail "The workflow hasn't started: see gh run list --workflow $workflow"
gh run watch "$run" --exit-status >/dev/null || fail "The build failed: gh run view $run --log-failed"
url=$(gh release view "$tag" --json url --jq .url) || fail "The build passed, but $tag has no release"
if [ -n "$publish" ]; then
    echo "Published: $url"
else
    echo "Draft ready: $url"
    echo "Try it, then publish it: gh release edit $tag --draft=false"
fi
