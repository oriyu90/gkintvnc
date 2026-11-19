#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ "$(uname -s)" != "Darwin" ]]; then echo 'macOS only' >&2; exit 2; fi
export MACOSX_DEPLOYMENT_TARGET=13.0
cargo build --release --target x86_64-apple-darwin -p glintvnc-app
file target/x86_64-apple-darwin/release/glintvnc
printf 'Built unsigned binary. See docs/SECURITY.md before distributing or storing Keychain secrets in signed apps.\n'
