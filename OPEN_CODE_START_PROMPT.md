# Paste this into OpenCode from the repository root

You are the lead Rust/macOS engineer for GlintVNC. Read `AGENTS.md`, `docs/ARCHITECTURE.md`, `docs/ROADMAP.md`, `docs/SECURITY.md`, `docs/COMPATIBILITY_MATRIX.md` and `docs/VERIFICATION_LOG.md` before editing. Use the relevant project-local `.opencode/skills/` on each task.

**Real host:** a 2017 Intel MacBook Pro on macOS 13.7.8 with OpenCode. **Primary goal:** native Rust VNC client, beautiful modern dark UI, shared GUI+CLI settings, keyboard transforms, full-screen sessions, Keychain, stable reconnect, acceptable resource use.

FIRST TASK — P0 build and reality audit:
1. Record `uname -m`, `sw_vers`, `xcode-select -p`, `rustc -V`, `cargo -V`, `xcrun --show-sdk-version`, and Intel GPU Metal capabilities. Do not publish personal identifiers or secrets.
2. Run `scripts/bootstrap-macos.sh`, `cargo generate-lockfile`, `cargo fmt --all -- --check`, `cargo check --workspace`, `cargo test --workspace`; fix every error, warning and API incompatibility in the scaffold. `eframe` 0.37 uses `App::ui`, not `App::update`.
3. Use only trusted Cargo/crates.io/GitHub sources; review licenses and Rust minimum versions. If egui/wgpu 0.37 is incompatible with the physical Intel GPU or macOS 13, document evidence, then pin a compatible series rather than downgrading the OS or disabling safety.
4. Build release and launch GUI. Verify profile storage, Keychain read/write/delete with CLI + GUI, dark theme, window/fullscreen. Use a controlled local test VNC server to exercise handshake, Raw rectangles, pixel colors, pointer, keys and disconnect; no Internet-exposed password-only VNC.
5. Run `scripts/check.sh` and update `docs/VERIFICATION_LOG.md` with exact pass/fail command outputs, limitations and next steps.
6. Make a small, reviewed commit; then proceed to P1 (see ROADMAP) **only if P0 acceptance passes**. Add tests for every bug found. Never claim untested code works.

Constraints:
- Do not overwrite/delete existing NAS folders, models or system files. Work inside this repository; `.git` history is user-owned.
- Do not install plugins, network services, background agents or privileged software without explicit permission. Rust dependencies downloaded by Cargo for this project are okay, but inspect suspect post-install/build scripts.
- No plaintext secrets in settings, logs, analytics or CLI flags. Never commit `.env`, test passwords, or an actual Keychain export.
- Strong fail-closed behavior for unsupported TLS/security types. Do not silently downgrade to unauthenticated or unencrypted connections.
- Remote framebuffer and all incoming VNC content is untrusted, including names and clipboard. Bounds-check before allocation, decompress with limits, fuzz parsers.
- When requirements collide, prefer correctness, then safety, then latency, then appearance; document the tradeoff.
- The user specifically wants customization of macOS key behavior. Prioritize explicit Cmd/Ctrl/Option mapping, Japanese input/IME verification, key-up on focus loss and platform-specific keys in P2.
