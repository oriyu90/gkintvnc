# Build roadmap and OpenCode task board

Every task is finished only with code, tests, measured evidence on the real 2017 Intel Ventura machine, and an entry in `VERIFICATION_LOG.md`.

## P0 — get a minimal product running; audit existing scaffold (priority 0)

| Task | Deliverable | Gate |
|---|---|---|
| P0-01 | Verify Intel host, SDK, Xcode/CLT, Rust MSRV and actual `eframe 0.37.0` support | `docs/VERIFICATION_LOG.md` filled |
| P0-02 | Compile existing workspace and fix all compile errors | `cargo fmt`, `cargo check`, `cargo test`, `cargo clippy` pass |
| P0-03 | Run app with no args; add/edit/delete profile; toggle theme | GUI usable on Retina 13-inch screen |
| P0-04 | CLI profile add/list/show/set/remove and settings | GUI sees CLI changes after restart |
| P0-05 | Test Keychain set/get/delete, signed vs unsigned behavior | no password in config or logs |
| P0-06 | Controlled RFB server test (local-only, no WAN exposure), Raw frame | known 2×2 color test and 1080p demo |
| P0-07 | Test fullscreen entry/escape, disconnect, input click and scroll readiness | no trapped fullscreen or stuck keys |
| P0-08 | Pin dependency versions in Cargo.lock after checks | clean checkout builds |

## P1 — reliability and test infrastructure

- P1-01 session controller state machine with cancel tokens and connection-generation IDs.
- P1-02 reconnect on EOF/socket reset/timeouts only; decorrelated exponential jitter 1–30s, optional max attempts, manual retry; auth/cert/protocol failure is terminal.
- P1-03 test mock RFB server under `tests/mock-rfb/`: incomplete header, oversized size, EOF mid-frame, delayed rectangles, rapid resize, wrong password, reconnect.
- P1-04 bounded channel latest-frame semantics; never deadlock on UI close.
- P1-05 structured logs (tracing), redaction, support diagnostics with explicit consent. Local log rotation size and retention caps.
- P1-06 report p50/p95 client decoding/frame pipeline times and process RSS/CPU baseline on Intel hardware.

## P2 — actual everyday compatibility

- P2-01 protocol `RfbTransport` trait; reuse audited permissively licensed implementation or add incremental CopyRect/ZRLE (done 2026-11-19; no fuzz yet) / Hextile/Tight with fuzz.
- P2-02 VeNCrypt TLS/VNC + X509Vnc and certificate verification, no downgrade. Apple ARD security optional once tested.
- P2-03 macOS keyboard adapter with AppKit NSEvent events (keyDown, keyUp, flagsChanged), keymap editor, position-independent physical scancodes where supported, IME/Unicode tests.
- P2-04 all scroll directions and buttons, relative/absolute pointer mapping, multi-monitor and scaling.
- P2-05 1:1, Fit, Stretch and pan/zoom settings; Retina and odd remote aspect ratio tests.
- P2-06 username/password flows + Keychain application signing ACL behavior; resolve Keychain prompts.
- P2-07 high-confidence fallback state and UI for servers lacking requested features.

## P3 — visual polish and accelerated rendering

- P3-01 renderer abstraction and partial GPU upload (Metal via wgpu), row alignment and double-buffer; measure 2K/4K/5K.
- P3-02 pinned/autohide session toolbar and coherent fullscreen controls, high contrast UI, accessible hit targets.
- P3-03 saved host search, tagging, connection status, optional local thumbnails; no unencrypted previews at rest.
- P3-04 clipboard sync (opt-in, size cap, loop detection, no untrusted text eval).
- P3-05 low latency tuning (TCP_NODELAY, request cadence, adaptive quality; don't spam server).

## P4 — hardening and delivery

- P4-01 fuzz all packet decoders, compression bombs, oversize color formats, pointer and auth negotiation.
- P4-02 injected failures: wake/sleep, Wi-Fi roaming, DNS changes, force-quit, app restart, Window close under load.
- P4-03 profile schema migration/backups, fsync strategy, corruption recovery, no data loss.
- P4-04 signed and notarized x86_64 `.app`, documented entitlements, independent CLI or symlink.
- P4-05 accessible labels, keyboard navigation, Japanese localization, light/dark/system.
- P4-06 license inventory, SBOM, `cargo deny` / `cargo audit` review and threat model update.

## P5 — release gate

- Automated CI macOS x86_64 test build if runner available; physical 2017 Intel smoke, real TigerVNC/x11vnc/Windows/macOS Apple Screen Sharing matrix.
- User-facing docs, permissions rationale, code-sign/notarization, crash regression tests.
- Do not ship with legacy unencrypted auth as the default for LAN. Verify named test cases in `QA_PLAN.md`.

## Recommended work unit

One task = one small commit containing implementation + unit/integration tests + `VERIFICATION_LOG.md` entry + measured output. Do not ask an agent to implement all P0–P5 in one giant speculative code change. Never let a failing compile roll forward to the next phase.
