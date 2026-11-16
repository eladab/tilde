#!/bin/bash
# Regenerates app/Tilde.icns from scripts/make-icon.swift.
set -euo pipefail
cd "$(dirname "$0")/.."

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
swift scripts/make-icon.swift "$work/icon.png"

set="$work/Tilde.iconset"
mkdir "$set"
for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$work/icon.png" --out "$set/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" "$work/icon.png" --out "$set/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$set" -o app/Tilde.icns
echo "wrote app/Tilde.icns"
