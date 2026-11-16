#!/bin/bash
# Builds build/Tilde-<version>.dmg: a disk image that opens on Tilde.app next
# to a shortcut to Applications, for the usual drag-to-install.
#
#     ./scripts/dmg.sh              # rebuilds build/Tilde.app first
#     ./scripts/dmg.sh --no-build   # packages the build/Tilde.app that is there
#
# Laying out the window (icon positions, background) is done by asking the
# Finder, which macOS may ask you to allow once. If that is refused the image
# is still made and still installs; it just opens as a plain window.
set -euo pipefail
cd "$(dirname "$0")/.."

if [ "${1:-}" != "--no-build" ]; then
    ./scripts/bundle.sh
fi
APP=build/Tilde.app
[ -d "$APP" ] || { echo "there is no $APP; run without --no-build" >&2; exit 1; }

VOLUME=Tilde
VERSION=$(/usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$APP/Contents/Info.plist")
DMG="build/Tilde-$VERSION.dmg"
if [ -e "/Volumes/$VOLUME" ]; then
    echo "a volume called $VOLUME is mounted; eject it first" >&2
    exit 1
fi

work=$(mktemp -d)
device=
cleanup() {
    [ -n "$device" ] && hdiutil detach "$device" -quiet -force 2>/dev/null || true
    rm -rf "$work"
}
trap cleanup EXIT

# What the image holds.
stage="$work/stage"
mkdir -p "$stage/.background"
cp -R "$APP" "$stage/Tilde.app"
ln -s /Applications "$stage/Applications"
swift scripts/make-dmg-background.swift "$stage/.background/background.png"
sips -s dpiWidth 144 -s dpiHeight 144 "$stage/.background/background.png" >/dev/null

# A writable image first, so the Finder can record how its window looks.
hdiutil create -quiet -srcfolder "$stage" -volname "$VOLUME" -fs HFS+ -format UDRW -ov "$work/rw.dmg"
device=$(hdiutil attach "$work/rw.dmg" -readwrite -noverify -noautoopen 2>/dev/null | awk '/^\/dev\// { print $1; exit }')

layout() {
    osascript <<APPLESCRIPT
tell application "Finder"
    tell disk "$VOLUME"
        open
        set current view of container window to icon view
        set toolbar visible of container window to false
        set statusbar visible of container window to false
        set the bounds of container window to {200, 120, 840, 548}
        set opts to the icon view options of container window
        set arrangement of opts to not arranged
        set icon size of opts to 128
        set text size of opts to 13
        set background picture of opts to file ".background:background.png"
        set position of item "Tilde.app" of container window to {170, 190}
        set position of item "Applications" of container window to {470, 190}
        update without registering applications
        delay 1
        close
    end tell
end tell
APPLESCRIPT
}
# The Finder can take a moment to notice the volume, and may not answer at
# all while a permission question is on screen: give it a minute, no more.
layout >"$work/layout.log" 2>&1 &
layout_pid=$!
( sleep 60; kill "$layout_pid" 2>/dev/null ) &
watchdog=$!
disown "$watchdog"
if wait "$layout_pid"; then
    laid_out=yes
else
    laid_out=no
fi
kill "$watchdog" 2>/dev/null || true

sync
hdiutil detach "$device" -quiet
device=

rm -f "$DMG"
hdiutil convert -quiet "$work/rw.dmg" -format UDZO -imagekey zlib-level=9 -o "$DMG"

# Signed like the app, when there is an identity to sign with.
IDENTITY="${TILDE_SIGN_IDENTITY:-}"
if [ -z "$IDENTITY" ]; then
    IDENTITY=$(security find-identity -v -p codesigning 2>/dev/null | awk '/^ *[0-9]+\)/ { print $2; exit }')
fi
if [ -n "$IDENTITY" ] && [ "$IDENTITY" != "-" ]; then
    codesign --force --sign "$IDENTITY" "$DMG" || echo "could not sign $DMG" >&2
fi

if [ "$laid_out" = no ]; then
    echo "the Finder did not lay out the window ($(tr '\n' ' ' <"$work/layout.log")); the image opens as a plain window" >&2
fi
echo "built $DMG ($(du -h "$DMG" | cut -f1 | tr -d ' '))"
