#!/bin/bash
# Builds both halves in release mode and assembles build/Tilde.app.
set -euo pipefail
cd "$(dirname "$0")/.."

swift build -c release --package-path app
cargo build --release --manifest-path tui/Cargo.toml

APP=build/Tilde.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Helpers" "$APP/Contents/Resources"
cp app/.build/release/Tilde "$APP/Contents/MacOS/Tilde"
cp tui/target/release/tilde-tui "$APP/Contents/Helpers/tilde-tui"
cp app/Info.plist "$APP/Contents/Info.plist"
cp app/Tilde.icns "$APP/Contents/Resources/Tilde.icns"

# Ad-hoc signature: enough to run locally. Passkeys and autofill will need a
# real identity and entitlements.
codesign --force --sign - "$APP/Contents/Helpers/tilde-tui"
codesign --force --sign - "$APP"
echo "built $APP"
