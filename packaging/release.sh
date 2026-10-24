#!/bin/sh
# Releases the Android app or the desktop one: tags main's latest commit
# android-v<version> or desktop-v<version> and pushes the tag, after
# checking that it is what GitHub has, that CI passed on it and that the
# version goes up. The Android Release workflow then builds and signs the
# APK, or the Desktop Release one builds for Linux and Windows
# (.github/workflows/), and puts it in a draft release, to try and publish
# by hand.
#   packaging/release.sh android 0.4.0
#   packaging/release.sh desktop 0.1.0
# Needs gh, signed in with access to the repository.
set -eu

if [ $# -ne 2 ] || ! echo "$2" | grep -Eqx '[0-9]+\.[0-9]{1,2}\.[0-9]{1,2}'; then
    echo "usage: $0 android|desktop <x.y.z> (y and z below 100)" >&2
    exit 1
fi
version=$2
# Each app's tag, workflow, name, and the paths its changes are listed from.
case $1 in
android)
    workflow=android-release.yml
    name="Spotless for Android"
    set -- android core
    ;;
desktop)
    workflow=desktop-release.yml
    name="Spotless (desktop)"
    set -- src core build.rs Cargo.toml Cargo.lock packaging ':!packaging/android'
    ;;
*)
    echo "usage: $0 android|desktop <x.y.z>" >&2
    exit 1
    ;;
esac
kind=${workflow%-release.yml}
tag=$kind-v$version

cd "$(dirname "$0")/.."
fail() {
    echo "$*" >&2
    exit 1
}

git fetch -q --tags origin main
[ "$(git branch --show-current)" = main ] || fail "Not on main"
[ -z "$(git status --porcelain)" ] || fail "There are uncommitted changes"
[ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] ||
    fail "main isn't the same as GitHub's: push or pull first"
! git rev-parse -q --verify "refs/tags/$tag" >/dev/null || fail "$tag already exists"

# The previous release, by version; the new one must come after it.
previous=$(git tag --list "$kind-v*" | sort -V | tail -n 1)
if [ -n "$previous" ]; then
    highest=$(printf '%s\n%s\n' "${previous#"$kind"-v}" "$version" | sort -V | tail -n 1)
    [ "$highest" = "$version" ] || fail "$version doesn't come after ${previous#"$kind"-v}"
fi

# Every Build run on this commit finished and passed.
commit=$(git rev-parse HEAD)
runs=$(gh run list --commit "$commit" --workflow build.yml --json status,conclusion \
    --jq '[.[] | "\(.status) \(.conclusion)"] | join("\n")')
[ -n "$runs" ] || fail "CI hasn't run on $commit"
if echo "$runs" | grep -qv '^completed success$'; then
    fail "CI hasn't passed on $commit (yet): see gh run list --commit $commit"
fi

if [ -n "$previous" ]; then
    echo "Changes since ${previous#"$kind"-v}:"
    git log --no-merges --format='  %h %s' "$previous..HEAD" -- "$@"
else
    echo "The first release."
fi
printf 'Release %s %s? [y/N] ' "$name" "$version"
read -r answer
[ "$answer" = y ] || fail "Not released"

git tag -a "$tag" -m "$name $version"
git push -q origin "$tag"
echo "Pushed $tag; the $workflow workflow builds it."

printf 'Wait for the draft release? [Y/n] '
read -r answer
[ "$answer" != n ] || exit 0
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
echo "Draft ready: $url"
echo "Try it, then publish it: gh release edit $tag --draft=false"
