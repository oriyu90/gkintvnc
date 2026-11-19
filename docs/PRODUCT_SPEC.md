# PRD — product requirements and acceptance criteria

**Target:** 2017 Intel MacBook Pro, Ventura 13.7.8. **Language:** Japanese-first UI with English-ready strings. **Name:** GlintVNC (working name). **Focus:** client-only; never expose or start a VNC server.

## P0→P5 requirement matrix

| ID | Requirement | Observable acceptance | Phase |
|---|---|---|---|
| F01 | GUI host/address book | create/edit/delete, tag/search, persist over restart | P0 then P3 search |
| F02 | CLI parity | list/add/edit/delete/probe/connect/settings using same storage | P0/P1 |
| F03 | Credentials | username as metadata; password exclusively Keychain; success/failure UI | P0/P2 |
| F04 | Dark/light/system | modern restrained UI, legible on 13-inch Retina | P0/P3 |
| F05 | Fullscreen | optional automatic native Space fullscreen; F8 toolbar toggle + Esc/toolbar escape (done 2026-11-19) | P0/P2 |
| F06 | Sizing | fit, 1:1+pan, stretch; 1080p/1440p/4K/5K and rotated sizes | P2/P3 |
| F07 | Key translation | Cmd/Ctrl/Option/Win, F1-F35, JP-IME/dead-keys/Unicode; per-host mappings; F1-F35 done, Fn home button pending | P2 |
| F08 | RFB interoperability | Raw, CopyRect, ZRLE (done); Hextile, Tight (pending); 3.3/3.7/3.8 | P0/P2 |
| F09 | Security types | VNC Auth legacy (opt-in); VeNCrypt/TLS cert verification; ARD login if feasible | P2 |
| F10 | Reliability | retry only network failures; bounded exponential jitter; never retry wrong password | P1 |
| F11 | Fast path | partial GPU uploads (done, `set_partial`), dirty-region framebuffer apply (done), never-drop pointer path + 16ms motion throttle + mid-update input dispatch (done, 2026-11-19), manual full-repaint Refresh (done, 2026-11-19); adaptive refresh; low input latency (done, event-driven) | P3 |
| F12 | Robustness | fuzz decoder, reject malformed inputs, no crash on disconnect | P1/P4 |
| F13 | Clipboard | opt-in (default off, 2026-11-19) bidirectional text sync, loop suppression, size cap | P3 |
| F14 | Diagnostic | CLI `probe`, explain errors, no credentials in logs, optional bounded diagnostics | P1 |
| F15 | Delivery | x86_64 `.app`, matching CLI, signing/hardened runtime/notarization path | P4 |

## Quantitative targets (design targets; NOT measurements)

- Idle with address book open: optimize for <120MB resident app process memory, <2% CPU average; actual framework may exceed and must be measured. Do not falsify.
- Connected 1920×1080 at 30fps, LAN: input-to-visible feedback p95 <100ms *excluding server frame generation* as aspirational goal; instrument client-side separately.
- 4K (3840×2160) framebuffer: update without allocating arbitrary unbounded rectangles; target <600MB app process peak **after P3**, measure on physical hardware.
- Time-to-first-frame local LAN: target <2s when server is responsive; not a guarantee.
- 30-minute disconnect/reconnect fault test: no leak trend; server auth failures never silently retried.
- User-facing GUI can be operated entirely via keyboard and mouse, with 125–200% display scaling.

## UI structure

**Home:** header and global search; sidebar of groups; responsive host cards (name, host, last status), one-click connect; minimal graphite palette and accent blue/teal. No crowded toolbar.

**Profile editor:** host + port, optional username, password store toggle, security mode, **scale (Fit / 1:1 / Stretch) and keymap selectors (done 2026-11-19)**, fullscreen, reconnect policy. Explain why unsafe legacy mode is dangerous. Password never shown after saving.

**Session:** native fullscreen by default; full-size remote canvas and compact autohide toolbar. Critical actions accessible by F8/menu: reconnect, scale, keyboard mapping, fullscreen exit, disconnect, stats. Overlay errors are non-modal; UI remains responsive while network is down.

**Settings:** dark/light/system; proxy/tunnel, clipboard, latency/quality, Keychain policy, diagnostics. GUI and CLI update the same model.

## Detailed acceptance checks

- No saved passwords in `config.json`; `grep -Ri` test for marker secret.
- Password update/remove in Keychain works after app restart; rejected Keychain requests surface errors.
- Option-key alternate characters and Japanese IME do not duplicate input; Mac Command is deliberately mapped for Windows/Linux and can be overridden.
- Pixel aspect and pointer coordinates remain correct on Retina display and when remote resolution changes.
- Connection interrupted during drag/held key does not leave remote pressed state after reconnect.
- Fullscreen does not trap user; Cmd-Tab can escape. Closing app gracefully terminates IO tasks.
- Certificate changes reject connection and require deliberate trust action; no "ignore TLS errors" toggle.
- Resilience under 3G/4G jitter is tested using local network shaping; never claim physically impossible zero disconnects.
