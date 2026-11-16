# Tilde: build, bundle, run, test. Each recipe is a thin wrapper, so the
# scripts in scripts/ work without `just` installed.

# Debug builds of both halves
build:
    swift build --package-path app
    cargo build --manifest-path tui/Cargo.toml

# Release build assembled into build/Tilde.app
bundle:
    ./scripts/bundle.sh

# Disk image for drag-to-Applications install (build/Tilde-<version>.dmg)
dmg:
    ./scripts/dmg.sh

# Build the app bundle and open it
run: bundle
    open build/Tilde.app

# Debug run straight from the build directories (logs in ~/Library/Logs/tilde)
dev: build
    TILDE_TUI="$PWD/tui/target/debug/tilde-tui" app/.build/debug/Tilde

# The TUI alone, against a fake engine, in the current terminal
mock:
    cargo run --manifest-path tui/Cargo.toml -- --mock

# Unit tests of both halves
test:
    ./scripts/test.sh

# End-to-end checks against build/Tilde.app (opens a window, needs network)
e2e: bundle
    python3 scripts/e2e.py

# Redraw the app icon (app/Tilde.icns) from scripts/make-icon.swift
icon:
    ./scripts/make-icon.sh
