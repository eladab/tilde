#!/bin/bash
# Writes build/appcast.xml: the update feed that an installed Tilde reads
# (SUFeedURL in app/Info.plist), offering the disk image of one version.
#
#     ./scripts/appcast.sh            # for build/Tilde-<version>.dmg
#
# The image is signed with the private update key: the one in
# SPARKLE_PRIVATE_KEY if that is set (as in the release workflow), otherwise
# the one in your keychain under the account "tilde".
set -euo pipefail
cd "$(dirname "$0")/.."

REPO="${TILDE_REPO:-eladab/tilde}"
VERSION=$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" app/Info.plist)
MINIMUM=$(/usr/libexec/PlistBuddy -c "Print :LSMinimumSystemVersion" app/Info.plist)
DMG="build/Tilde-$VERSION.dmg"
[ -f "$DMG" ] || { echo "there is no $DMG; run ./scripts/dmg.sh" >&2; exit 1; }

SIGN=app/.build/artifacts/sparkle/Sparkle/bin/sign_update
[ -x "$SIGN" ] || swift package --package-path app resolve
if [ -n "${SPARKLE_PRIVATE_KEY:-}" ]; then
    signature=$(echo "$SPARKLE_PRIVATE_KEY" | "$SIGN" --ed-key-file - -p "$DMG")
else
    signature=$("$SIGN" --account tilde -p "$DMG")
fi
[ -n "$signature" ] || { echo "could not sign $DMG" >&2; exit 1; }

cat > build/appcast.xml <<XML
<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
    <channel>
        <title>Tilde</title>
        <item>
            <title>Tilde $VERSION</title>
            <pubDate>$(LC_ALL=C date -u "+%a, %d %b %Y %H:%M:%S +0000")</pubDate>
            <sparkle:version>$VERSION</sparkle:version>
            <sparkle:shortVersionString>$VERSION</sparkle:shortVersionString>
            <sparkle:minimumSystemVersion>$MINIMUM</sparkle:minimumSystemVersion>
            <sparkle:fullReleaseNotesLink>https://github.com/$REPO/releases/tag/v$VERSION</sparkle:fullReleaseNotesLink>
            <enclosure url="https://github.com/$REPO/releases/download/v$VERSION/Tilde-$VERSION.dmg"
                       sparkle:edSignature="$signature"
                       length="$(stat -f %z "$DMG")"
                       type="application/octet-stream"/>
        </item>
    </channel>
</rss>
XML
echo "built build/appcast.xml for $VERSION"
