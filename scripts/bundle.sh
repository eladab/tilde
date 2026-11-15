#!/bin/bash
# Builds both halves in release mode and assembles build/Weft.app.
set -euo pipefail
cd "$(dirname "$0")/.."

swift build -c release --package-path app
cargo build --release --manifest-path tui/Cargo.toml

APP=build/Weft.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Helpers" "$APP/Contents/Resources"
cp app/.build/release/Weft "$APP/Contents/MacOS/Weft"
cp tui/target/release/weft-tui "$APP/Contents/Helpers/weft-tui"
cp app/Info.plist "$APP/Contents/Info.plist"

# Ad-hoc signature: enough to run locally. Passkeys and autofill will need a
# real identity and entitlements.
codesign --force --sign - "$APP/Contents/Helpers/weft-tui"
codesign --force --sign - "$APP"
echo "built $APP"
