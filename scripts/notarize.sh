#!/bin/bash
# Has Apple notarize build/Tilde-<version>.dmg and staples the ticket to it,
# so that macOS opens Tilde on another Mac without the "Open Anyway" step.
#
# Needs a paid Apple Developer account:
#   - the app signed with its Developer ID Application certificate and the
#     hardened runtime:
#       TILDE_HARDENED=1 TILDE_SIGN_IDENTITY="Developer ID Application: …" ./scripts/dmg.sh
#   - APPLE_ID, APPLE_TEAM_ID and APPLE_APP_PASSWORD (an app-specific
#     password made at account.apple.com) in the environment.
#
# Not part of the release workflow yet, and not exercised: there has been no
# Developer ID to try it with.
set -euo pipefail
cd "$(dirname "$0")/.."

: "${APPLE_ID:?set APPLE_ID}" "${APPLE_TEAM_ID:?set APPLE_TEAM_ID}" "${APPLE_APP_PASSWORD:?set APPLE_APP_PASSWORD}"
VERSION=$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" app/Info.plist)
DMG="build/Tilde-$VERSION.dmg"
[ -f "$DMG" ] || { echo "$DMG not found: run scripts/dmg.sh first" >&2; exit 1; }

codesign -d --verbose=2 build/Tilde.app 2>&1 | grep -q "flags=.*runtime" ||
    { echo "build/Tilde.app is not signed with the hardened runtime: build it with TILDE_HARDENED=1" >&2; exit 1; }

xcrun notarytool submit "$DMG" --apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_APP_PASSWORD" --wait
xcrun stapler staple "$DMG"
echo "notarized $DMG"
