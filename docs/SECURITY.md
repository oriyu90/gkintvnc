# Threat model / credential policy / network security

## Risk classification

| Vector | Threat | Mitigation / test |
|---|---|---|
| Untrusted RFB server | memory exhaustion, malformed pixels, compression bombs | strict checked math, message caps, decompression limits, **per-update RGBA damage budget (128 MiB)**, 16 Mpx canvas cap, 15s per-read timeout, fuzz + ASAN where toolchain supports |
| Plain RFB | password auth DES and unencrypted screen/input leakage | P0 localhost-only default; remote opt-in is visibly labeled insecure; P2 authenticated TLS or external SSH tunnel |
| MITM on TLS | forged certificate / downgrade | verified PKIX/hostname or user-managed pinned CA; abort on mismatch, never auto-accept |
| Saved passwords | disclosure via JSON, logs, process args, backups | Keychain only; no CLI password flags; prompt without echo; configs redact |
| Binary identity | repeated Keychain prompts after unsigned build changes | sign stable app identity; verify ACL/partition; never blanket-authorize another app |
| Remote clipboard/name | control code injection, HTML/script or excessive data | treat as bytes/plain text, no shell/eval, cap lengths, opt-in clipboard |
| Key events | stuck modifiers after disconnect/focus loss | tracked held-state set, keyup release on state transitions, force reconnect reset |
| Reconnect | endless wrong-password attempts and lockouts | authentication and cert failures terminal, bounded backoff/jitter, manual retry |
| GUI process crash | config loss, incomplete secrets | atomic file replace, schema migrations, backups, recoverable error overlay |
| DNS/redirect | unintended external host | make actual endpoint visible; TLS verifies target hostname; do not auto-accept redirects |

## Security design requirements

1. `LocalhostOnly` **does not encrypt RFB**. It merely restricts direct destination to localhost/loopback. For LAN/WAN encryption use verified TLS (P2) or an independently configured encrypted tunnel. The P0 client does not establish SSH or WireGuard tunnels by itself.
2. `AllowLegacyUnencrypted` requires intentional per-profile opt-in, displayed prominently; do not use on public Wi-Fi or open Internet. VNC Authentication uses DES challenge-response; it is not a secure channel.
3. Passwords are never stored in JSON, plaintext config, `.env`, crash reports, arguments, tests or screenshot filenames. Avoid storing connection passwords in UI text state longer than necessary. Prefer secret zeroization after use when feasible, acknowledging OS snapshots and immutable strings are not a formal zeroization guarantee.
4. Credentials live in Keychain under `dev.glintvnc.viewer.v1` service and random profile ID account; deletion best-effort with clear error if Keychain is locked. **: all Keychain calls (get/set/delete) run on worker threads; the UI thread never waits for an interactive Keychain prompt.**
5. CLI `secret set PROFILE` requests a hidden password prompt; `--stdin` is for controlled automation only. Do not use `echo password` in shell history or CI logs. A saved password never silently downgrades to None-auth when the server offers only VNC type 1.
6. UI and CLI must not accidentally create two different Keychain identities for the same logical profile once a packaged `.app` is signed; package one binary and test its signed identity.
7. Store only what is necessary. User telemetry is disabled by default and no telemetry backend exists in this kit.
8. `unwrap`/`expect` in parsing/network path prohibited except provably invariant local constants. Preserve `#![forbid(unsafe_code)]` on core.
9. Limit all remote text fields, rectangle count, pixel sizes, UTF-8 replacement handling. ZRLE is implemented (2026-11-19) with hard caps: zlib input length <= 64 MiB per rectangle, decompressed output <= 64 MiB per rectangle, run length bounded by remaining tile cells, rectangle bounds checked before any canvas write, declared trailing bytes rejected, persistent per-connection zlib stream (RFC 6143). ** (2026-11-19): the total RGBA bytes buffered from a single FramebufferUpdate are also capped (128 MiB) before any allocation, so a flood of full-frame CopyRects cannot OOM the process. The framebuffer is capped at 16 Mpx (64 MiB RGBA).** For Tight/Hextile (not implemented): enforce the same class of decoded-size ratio, absolute byte caps and CPU budget before merge.
10. Do not silently fall back to None/VNC Auth after an unsuccessful TLS negotiation. Explicit refusal is a feature, not a compatibility bug.

## P0 limitations / red flags

- Updates are now applied as bounded dirty rectangles onto a persistent framebuffer and sent to the GPU via `TextureHandle::set_partial`; full re-upload happens only at first frame / resize. 4K remains heavy but bounded by actual changed area.
- No TLS, VeNCrypt, Apple ARD, RA2, certificate pinning, tunnel management or verified certificate paths implemented.
- Key-up on disconnect/focus loss **is implemented** (release on focus loss, explicit disconnect, connection close). Trackpad scroll accumulation (buttons 4/5/6/7) and ⌘R full repaint are implemented. Native IME and all modifiers remain limited (Unicode keysyms are server-dependent).
- Storing metadata in JSON and Keychain does not guarantee crash-proof updates or perfect privacy; test real macOS Keychain access controls.
- Avoid logging desktop names or clipboard if they contain sensitive information.

## Pre-release audit

`cargo deny check` license/advisories review; `cargo audit`; `cargo fuzz` inputs; manual grep against password fixtures; packet mutation; signing/entitlements review; reconnect abuse; CI secret scan. Document audit evidence, not just a checkmark.
