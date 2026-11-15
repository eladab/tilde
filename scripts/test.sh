#!/bin/bash
# Runs the unit tests of both halves.
set -euo pipefail
cd "$(dirname "$0")/.."
swift test --package-path app
cargo test --manifest-path tui/Cargo.toml
