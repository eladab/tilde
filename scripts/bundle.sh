#!/bin/bash
# Builds both halves in release mode and assembles build/Tilde.app.
set -euo pipefail
cd "$(dirname "$0")/.."

swift build -c release --package-path app
cargo build --release --manifest-path tui/Cargo.toml

APP=build/Tilde.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Helpers" "$APP/Contents/Resources" "$APP/Contents/Frameworks"
cp app/.build/release/Tilde "$APP/Contents/MacOS/Tilde"
cp tui/target/release/tilde-tui "$APP/Contents/Helpers/tilde-tui"
cp app/Info.plist "$APP/Contents/Info.plist"
cp app/Tilde.icns "$APP/Contents/Resources/Tilde.icns"
# Sparkle, the updater. ditto keeps the framework's symlinks.
ditto app/.build/release/Sparkle.framework "$APP/Contents/Frameworks/Sparkle.framework"

# Sign with a real identity when there is one. The keychain remembers "Always
# Allow" for an app by its signature, and an ad-hoc signature is different in
# every build, so with one the question (for instance about the "Tilde
# WebCrypto Master Key" that WebKit keeps for sites like WhatsApp Web) comes
# back after each rebuild. TILDE_SIGN_IDENTITY picks the identity; "-" forces
# an ad-hoc signature. Passkeys and autofill will also need entitlements.
IDENTITY="${TILDE_SIGN_IDENTITY:-}"
if [ -z "$IDENTITY" ]; then
    IDENTITY=$(security find-identity -v -p codesigning 2>/dev/null | awk '/^ *[0-9]+\)/ { print $2; exit }')
fi
IDENTITY="${IDENTITY:--}"
# Inside out: Sparkle's helpers, the framework, the TUI, then the app.
sign() {
    local sparkle="$APP/Contents/Frameworks/Sparkle.framework/Versions/Current"
    codesign --force --sign "$1" "$sparkle/XPCServices/Installer.xpc" &&
    codesign --force --sign "$1" "$sparkle/XPCServices/Downloader.xpc" &&
    codesign --force --sign "$1" "$sparkle/Autoupdate" &&
    codesign --force --sign "$1" "$sparkle/Updater.app" &&
    codesign --force --sign "$1" "$APP/Contents/Frameworks/Sparkle.framework" &&
    codesign --force --sign "$1" "$APP/Contents/Helpers/tilde-tui" &&
    codesign --force --sign "$1" "$APP"
}
if ! sign "$IDENTITY"; then
    echo "signing with $IDENTITY failed; falling back to an ad-hoc signature" >&2
    IDENTITY=-
    sign -
fi
if [ "$IDENTITY" = "-" ]; then
    echo "signed ad hoc: keychain permissions will not carry over to the next build"
fi
echo "built $APP"
