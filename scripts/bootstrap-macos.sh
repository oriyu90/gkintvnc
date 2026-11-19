#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
echo '== GlintVNC host audit (read-only) =='
if [[ "$(uname -s)" != "Darwin" ]]; then echo 'ERROR: run this on the intended macOS host.' >&2; exit 2; fi
arch="$(uname -m)"; os="$(sw_vers -productVersion)"
echo "machine: $arch | macOS: $os"
if [[ "$arch" != "x86_64" ]]; then echo 'WARNING: this is not the target Intel Mac.' >&2; fi
if [[ "$os" != 13.* ]]; then echo 'WARNING: test target is macOS Ventura 13.7.8.' >&2; fi
for tool in xcode-select xcrun rustc cargo rustup opencode; do
  if command -v "$tool" >/dev/null 2>&1; then echo "OK: $tool => $(command -v "$tool")"; else echo "MISSING: $tool"; fi
done
if command -v xcode-select >/dev/null; then xcode-select -p || true; fi
if command -v xcrun >/dev/null; then xcrun --show-sdk-version || true; fi
if command -v rustc >/dev/null; then rustc -V; fi
if command -v cargo >/dev/null; then cargo -V; fi
if command -v system_profiler >/dev/null; then system_profiler SPDisplaysDataType | grep -E 'Chipset Model|Metal|VRAM' || true; fi
printf '\nNo software was installed. If rustup or Xcode tools are missing, install from official sources after review.\n'
