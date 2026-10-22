#!/bin/sh
# Releases the Android app: tags main's latest commit android-v<version> and
# pushes the tag, after checking that it is what GitHub has, that CI passed
# on it and that the version goes up. The Android Release workflow
# (.github/workflows/android-release.yml) then builds and signs the APK and
# puts it in a draft release, to try and publish by hand.
#   packaging/android/release.sh 0.2.0
# Needs gh, signed in with access to the repository.
set -eu

if [ $# -ne 1 ] || ! echo "$1" | grep -Eqx '[0-9]+\.[0-9]{1,2}\.[0-9]{1,2}'; then
    echo "usage: $0 <x.y.z> (y and z below 100)" >&2
    exit 1
fi
version=$1
tag=android-v$version

cd "$(dirname "$0")/../.."
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
previous=$(git tag --list 'android-v*' | sort -V | tail -n 1)
if [ -n "$previous" ]; then
    highest=$(printf '%s\n%s\n' "${previous#android-v}" "$version" | sort -V | tail -n 1)
    [ "$highest" = "$version" ] || fail "$version doesn't come after ${previous#android-v}"
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
    echo "Changes since ${previous#android-v}:"
    git log --no-merges --format='  %h %s' "$previous..HEAD" -- android core
else
    echo "The first release."
fi
printf 'Release %s? [y/N] ' "$version"
read -r answer
[ "$answer" = y ] || fail "Not released"

git tag -a "$tag" -m "Spotless for Android $version"
git push -q origin "$tag"
echo "Pushed $tag; the Android Release workflow builds it."

printf 'Wait for the draft release? [Y/n] '
read -r answer
[ "$answer" != n ] || exit 0
run=
for _ in 1 2 3 4 5 6 7 8 9 10; do
    run=$(gh run list --workflow android-release.yml --branch "$tag" --json databaseId \
        --jq '.[0].databaseId // empty')
    [ -z "$run" ] || break
    sleep 3
done
[ -n "$run" ] || fail "The workflow hasn't started: see gh run list --workflow android-release.yml"
gh run watch "$run" --exit-status >/dev/null || fail "The build failed: gh run view $run --log-failed"
echo "Draft ready: $(gh release view "$tag" --json url --jq .url)"
echo "Try the APK, then publish it: gh release edit $tag --draft=false"
