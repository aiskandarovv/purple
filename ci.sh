#!/bin/sh
# Local CI: mirrors .github/workflows/ci.yml and enforces the full
# pre-commit checklist. Run before pushing to catch failures early.
set -e
cd "$(dirname "$0")"

# Read MSRV from Cargo.toml
MSRV=$(grep '^rust-version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
if [ -z "$MSRV" ]; then
    printf "Could not read rust-version from Cargo.toml\n"
    exit 1
fi

printf "=== 1/13 Format ===\n"
cargo fmt --check

printf "\n=== 2/13 Clippy ===\n"
cargo clippy --locked --all-targets -- -D warnings

printf "\n=== 3/13 Build ===\n"
cargo build --locked

printf "\n=== 4/13 Test ===\n"
cargo test --locked

printf "\n=== 5/13 Deny ===\n"
if ! command -v cargo-deny >/dev/null 2>&1; then
    printf "cargo-deny not installed. Install with: cargo install cargo-deny\n"
    exit 1
fi
cargo deny check

printf "\n=== 6/13 MSRV (%s) ===\n" "$MSRV"
if ! rustup run "$MSRV" rustc --version >/dev/null 2>&1; then
    printf "MSRV %s not installed. Install with: rustup toolchain install %s\n" "$MSRV" "$MSRV"
    exit 1
fi
rustup run "$MSRV" cargo check --locked

printf "\n=== 7/13 Doc ===\n"
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked

printf "\n=== 8/13 Site sync ===\n"
(cd site && sh check-sync.sh)

printf "\n=== 9/13 TUI smoke ===\n"
if command -v tmux >/dev/null 2>&1; then
    ./tests/smoke_tui.sh
else
    printf "tmux not installed; skipping TUI smoke test.\n"
fi

printf "\n=== 10/13 Design system ===\n"
./scripts/check-design-system.sh

printf "\n=== 11/13 Messages ===\n"
./scripts/check-messages.sh

printf "\n=== 12/13 Keybindings ===\n"
./scripts/check-keybindings.sh

printf "\n=== 13/13 Visual regression ===\n"
cargo test --locked --bin purple visual_regression

printf "\nAll checks passed.\n"
