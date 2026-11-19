# Quality gates and fault injection

## Quality gates

- Compile gate: `cargo check --workspace`, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace` pass on target host; dependencies pinned and licenses reviewed.
- Function gate: each of GUI, CLI, Keychain, connection, fullscreen, input, reconnect, and high-DPI tests has recorded expected/actual.
- Security gate: parser rejects malicious lengths, auth failures terminal, encrypted mode never downgrades, Keychain not leaking, no panic on invalid packets.
- Performance gate: timed measurements on physical Intel hardware; full-frame copies eliminated at 4K before general release.

## Cases (IDs usable by AI / CI)

| ID | Scenario | Expected |
|---|---|---|
| TC-001 | add profile in GUI | appears in CLI list after restart |
| TC-002 | CLI changes fullscreen/security settings | GUI reflects after reopen |
| TC-003 | save password in CLI; connect GUI | retrieves correct Keychain entry, no text file leak |
| TC-004 | reject Keychain permission | actionable error, no fallback plaintext |
| TC-005 | RFB 3.8 raw server | pixel-perfect known color rectangle |
| TC-006 | server VNC Auth wrong password | explicit auth error, no auto retry |
| TC-007 | disconnect mid-rectangle | no crash; resource cleanup |
| TC-008 | 0×0, max dimensions, huge name length | reject without overflow/allocation |
| TC-009 | resize during stream | correct pointer coordinates, no stale texture |
| TC-010 | Wi-Fi off/on | backoff and reconnect only if allowed |
| TC-011 | TLS server certificate mismatch | fatal; no silent insecure fallback (P2) |
| TC-012 | full screen then Cmd-Tab and Exit | no input trap, window state restored |
| TC-013 | JIS keyboard + IME `かな`, `英数`, Option, Cmd | intended mapping; no double events (P2) |
| TC-014 | press modifier, lose focus/reconnect | keyup + modifiers reset (P2) |
| TC-015 | 1080p/1440p/4K/5K, external Retina | correct scaling & letterbox (P3) |
| TC-016 | static 4K for 10min | no sustained CPU spin, no RSS growth (P3) |
| TC-017 | changed-area 4K scroll 10min | measured p95 and no unbounded buffers (P3) |
| TC-018 | crafted compression bomb | controlled error, not OOM (P2/P4) |
| TC-019 | multiple profiles with same username | secrets isolated by immutable ID |
| TC-020 | force quit during config write | original or complete new file, no partial JSON |
| TC-021 (R8) | server sends 16k full-frame CopyRects | typed Protocol error, no OOM (128 MiB damage budget) |
| TC-022 (R8) | server stalls after greeting / mid-rectangle | worker exits with Timeout within 15s; Stop/close does not leak a thread |
| TC-023 (R8) | server closes after a rendered frame | frozen frame is dropped; reconnect/close overlay visible |
| TC-024 (R8) | hold left button + scroll pulses | every PointerEvent carries the held button bit |
| TC-025 (R8) | two profiles with the same name | save rejected with localized error; CLI `connect` unambiguous |
| TC-026 (R8) | DesktopSize rect then follow-up | next FramebufferUpdateRequest has incremental=0 |
| TC-027 (R8) | Keychain locked / "Always Allow" prompt | UI keeps repainting (Keychain on worker thread) |

## Minimal mock harness contract

Rust test harness starts localhost TCP server on ephemeral port (`127.0.0.1:0`) and sends:
- RFB 003.008 header, SecurityTypes `[1]`, SecurityResult 0, server initialization with 2×2 pixels, UTF-8 name.
- Accept SetPixelFormat(32bpp) and SetEncodings Raw/CopyRect/ZRLE/DesktopSize/Cursor/LED State (order advertised by the client).
- Send FramebufferUpdate with four known BGRA pixels. Assert final RGBA colors match.
- Send truncated header or malformed oversized rectangle in separate tests; assert typed errors.
- Cover 3.3 with VNCAuth, 3.7 None variant, unusual cursor and DesktopSize.
- Also ship: mock RFB server (`tests/mock_rfb.py`) with pixel-exact Raw/CopyRect/ZRLE rectangles, Cursor/LED State pseudo-encodings, wrong-password and truncation cases; plus unit coverage for ZRLE sub-encodings.

Note: the harness is implemented and passing; Hextile/Tight coverage is not (encodings not implemented).
