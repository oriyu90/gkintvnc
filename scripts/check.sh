#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
command -v cargo >/dev/null || { echo 'Rust/Cargo not installed'; exit 2; }
echo '== Formatting =='
cargo fmt --all -- --check
echo '== Compilation =='
cargo check --workspace
echo '== Unit tests =='
cargo test --workspace
echo '== Clippy =='
cargo clippy --workspace --all-targets -- -D warnings
printf '\nPASS: Rust static/test gates (manual GPU/Keychain/VNC physical tests still required)\n'
