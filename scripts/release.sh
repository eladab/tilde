#!/bin/bash
# Starts a release: sets the version everywhere it is written, commits that
# and tags it. Pushing the tag makes GitHub build the disk image and publish
# the release (.github/workflows/release.yml).
#
#     ./scripts/release.sh 0.2.0
#     git push origin main v0.2.0
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION="${1:-}"
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "usage: $0 <version>, for instance 0.2.0" >&2; exit 1; }
[ -z "$(git status --porcelain)" ] || { echo "there are uncommitted changes" >&2; exit 1; }
! git rev-parse -q --verify "refs/tags/v$VERSION" >/dev/null || { echo "v$VERSION is already tagged" >&2; exit 1; }

# Sparkle compares CFBundleVersion, so both keys carry the same number.
# (Edited in place: PlistBuddy would rewrite the whole file without comments.)
for key in CFBundleShortVersionString CFBundleVersion; do
    sed -i '' -E "/<key>$key<\\/key>/{n;s|<string>.*</string>|<string>$VERSION</string>|;}" app/Info.plist
done
[ "$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" app/Info.plist)" = "$VERSION" ] &&
    [ "$(/usr/libexec/PlistBuddy -c "Print :CFBundleVersion" app/Info.plist)" = "$VERSION" ] ||
    { echo "could not set the version in app/Info.plist" >&2; exit 1; }
sed -i '' -E "1,/^version = /s/^version = .*/version = \"$VERSION\"/" tui/Cargo.toml
cargo update --quiet --offline --manifest-path tui/Cargo.toml --package tilde-tui

if [ -n "$(git status --porcelain)" ]; then
    git commit --quiet -am "Tilde $VERSION"
fi
git tag -a "v$VERSION" -m "Tilde $VERSION"
echo "tagged v$VERSION; publish it with: git push origin main v$VERSION"
