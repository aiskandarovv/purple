#!/bin/sh
# Local CI: mirrors .github/workflows/ci.yml
# Run before pushing to catch failures early.
set -e
cd "$(dirname "$0")"

# Read MSRV from Cargo.toml
MSRV=$(grep '^rust-version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
if [ -z "$MSRV" ]; then
    printf "Could not read rust-version from Cargo.toml\n"
    exit 1
fi

printf "=== Format ===\n"
cargo fmt --check

printf "\n=== Clippy ===\n"
cargo clippy --locked --all-targets -- -D warnings

printf "\n=== Test ===\n"
cargo test --locked

printf "\n=== Deny ===\n"
if ! command -v cargo-deny >/dev/null 2>&1; then
    printf "cargo-deny not installed. Install with: cargo install cargo-deny\n"
    exit 1
fi
cargo deny check

printf "\n=== MSRV (%s) ===\n" "$MSRV"
if ! rustup run "$MSRV" rustc --version >/dev/null 2>&1; then
    printf "MSRV %s not installed. Install with: rustup toolchain install %s\n" "$MSRV" "$MSRV"
    exit 1
fi
rustup run "$MSRV" cargo check --locked

printf "\nAll checks passed.\n"
