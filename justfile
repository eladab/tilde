# Weft: build, bundle, run, test. Each recipe is a thin wrapper, so the
# scripts in scripts/ work without `just` installed.

# Debug builds of both halves
build:
    swift build --package-path app
    cargo build --manifest-path tui/Cargo.toml

# Release build assembled into build/Weft.app
bundle:
    ./scripts/bundle.sh

# Build the app bundle and open it
run: bundle
    open build/Weft.app

# Debug run straight from the build directories (logs in ~/Library/Logs/weft)
dev: build
    WEFT_TUI="$PWD/tui/target/debug/weft-tui" app/.build/debug/Weft

# The TUI alone, against a fake engine, in the current terminal
mock:
    cargo run --manifest-path tui/Cargo.toml -- --mock

# Unit tests of both halves
test:
    ./scripts/test.sh

# End-to-end checks against build/Weft.app (opens a window, needs network)
e2e: bundle
    python3 scripts/e2e.py
