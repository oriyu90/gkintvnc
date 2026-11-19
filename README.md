# GlintVNC — OpenCode development kit

**Date of research:** 2026-11-19. **Target:** 2017 Intel MacBook Pro, macOS Ventura 13.7.8, x86_64-apple-darwin.

**Author:** Yuki_Orita (折田悠希 / おりたゆうき) · **License:** MIT

**Repository:** https://github.com/oriyu90/gkintvnc · **Release:** https://github.com/oriyu90/gkintvnc/releases/tag/v0.1.0 · **Website:** https://studio-rizi.pages.dev/projects/gkintvnc/

Detailed maintenance notes are kept in the private `oriyu90/common-rules-document` repository (`gkintvnc.md`), not in this public repository.

This is a working Rust VNC client plus its full specification and verification workspace. It builds and runs on the target Intel Mac. **The owner tested the app hands-on on the physical machine and declared the production environment complete on 2026-11-19.** The ad-hoc signing / notarization caveats below still apply.

## Start in 3 steps

1. Unzip into a writable working directory on the actual 2017 MacBook Pro.
2. Run `bash scripts/bootstrap-macos.sh`, then `bash scripts/check.sh`. All scripts are reviewable and deliberately non-destructive.
3. Start `opencode` at the project root; paste `OPEN_CODE_START_PROMPT.md`. Work in the order of `docs/ROADMAP.md` and maintain `docs/VERIFICATION_LOG.md`.

## What is delivered

- One Rust binary `glintvnc`: native GUI when invoked without arguments, CLI on subcommands.
- Shared JSON profile/settings store and macOS Keychain secret adapter; dark UI and saved connections.
- RFB 3.3/3.7/3.8 implementation (None / VNCAuth): Raw, CopyRect and ZRLE decoding, dirty-rect streaming (no full-framebuffer clones), partial GPU texture uploads (`TextureHandle::set_partial`), integrated keyboard/pointer input.
- Start fullscreen after connection; manual reconnect; bounded memory checks, optional reconnect for network errors.
- Design specification, security review, protocol/reliability/resolution/benchmark plans, reference applications, the project-local `frontend-design` skill, and build/package scripts.

## What is verified on the physical Intel Mac (2026-11-19)

- Release build works on macOS 13.7.8 (x86_64, Metal 3); GUI launches and stays up (window verified via CGWindowList).
- **LAN VNC session connects**: x11vnc server (LibVNCServer, `-forever -shared -usepw -bg -noscrollcopyrect`, user-authorized, 1920x1080, VNC auth) handshake + framebuffer init verified via `probe`. (Long mistaken for TigerVNC/Xtigervnc; corrected 2026-11-19 from `ps -ef` on the remote.)
- Raw + CopyRect + ZRLE encodings decode correctly (mock integration tests, pixel-exact; ZRLE added 2026-11-19); dirty-rect streaming verified live on the LAN server (one 8.3MB full frame, then 3-13 rect updates of ~50-97KB each); input path (Key/Pointer) byte-verified by integration tests.
- **GUI rendering verified in-app** via the built-in `GLINTVNC_SCREENSHOT` hook (no screen-recording permission needed): home/settings/mock-frame/LAN-frame captures all render correctly, in both dark (ffffff on near-black) and light (000000 on white) themes, with Japanese (Hiragino) text.
- **UI redesigned** (2026-11-18, refined Round 4/8): launcher with profile list + centered form and an in-app **Settings** window (language ja/en, theme dark/light/system, scroll speed, clipboard). Session controls live in an auto-hiding floating bar (Connections / Full screen / CAPS / Refresh / Settings / Disconnect), toggleable with **F8**; Esc leaves fullscreen when the bar is hidden. Display scale (Fit / 1:1 / Stretch) and keymap are set per profile in the editor (and `--scale` on the CLI).
- **Full-remote connection windows** (2026-11-18, Round 2): each connection opens its own window — remote fills the entire window, no side panels; an auto-hiding control bar appears on top-edge hover (Connections / Full screen / Settings / Disconnect). Multiple connections to different hosts open multiple windows simultaneously.
- **Black-screen bug fixed** (2026-11-18): incremental updates were applied onto a discarded zeroed canvas; the client now keeps a persistent framebuffer (regression test added). Live 192.0.2.10:5900 session renders the full remote desktop correctly.
- **Clipboard sharing** (2026-11-18): bidirectional text clipboard sync with the remote desktop (RFB ServerCutText/ClientCutText), RealVNC-style (no sync on connect, 256 KiB cap, echo suppression). Can be disabled via Settings or `settings set clipboard_sync false`.
- **Lag fixed** (2026-11-18, Round 3): frame events now wake the UI immediately (they previously waited for a 250 ms timer); the mouse cursor is the OS cursor itself swapped to the server-supplied image (RFB Cursor pseudo-encoding, -239) so local and remote pointers overlap exactly, with zero extra latency.
- **Drag & lag round 5** (2026-11-19): server updates now stream as dirty rectangles (full-framebuffer clones removed) and the GPU receives only those rects (`TextureHandle::set_partial`); the floating bar hides while a remote button is held and its pixels are local-only input (never leak into the remote); session window repaint is event-driven again; ZRLE encoding implemented for much smaller updates. Verified live against the LAN server (192.0.2.10:5900): full session renders correctly; the update stream after the initial 8.3MB frame carried only small dirty rects (3-13 rects, ~50-97KB each). **Input round 6 (2026-11-19) additionally hardens the pointer pipeline (never-drop button releases, 16ms motion throttle with latest-wins, command dispatch between rectangles mid-update).** **Round 7 (2026-11-19):** ghost pixels on window drags root-caused to the server being **x11vnc** (LibVNCServer; long misidentified as TigerVNC/Xtigervnc) with a polling framebuffer (`-wait/-defer 20ms`) and imperfect damage accounting — the client now advertises CompressLevel 1 (-255) and the floating bar gains a **Refresh (再描画)** button / ⌘R to force a full (non-incremental) framebuffer update, clearing ghosts on demand. **Note (2026-11-19): the laggy-drag root cause on this project's LAN server was compositor-induced full re-encodes; the remote xfwm4 compositor was disabled (user-authorized, read-back `false`). For other Xfce/x11vnc setups, disable compositing the same way. Drag feel must still be hand-verified by the user — CGEvent injection cannot test it on this machine.**
- **Input round 4** (2026-11-18): drag-outside-canvas no longer breaks (buttons stream clamped edge coordinates like TigerVNC; releases are delivered discretely); `Ctrl`+click acts as right-click on MacTo* presets; long-press autorepeat follows rfbproto (client-side, repeated down events); F1–F35/Insert/Space/lock keys (Caps/Num/Scroll Lock, PrintScreen, Pause, Menu) are mapped and sent as proper X11 keysyms; a CAPS button in the floating bar toggles the **remote** Caps Lock and shows the server-reported state via the LED State pseudo-encoding (-261). The UI was redesigned (Phosphor icons, instrument-panel dark, hairline dividers, 4-step type scale, segmented control, brand mark) following the frontend-design skill (installed in `.opencode/skills/frontend-design`, Apache-2.0).
- CLI (profiles/settings/secret/probe, incl. `settings get/set scroll_speed`), Keychain set/get/delete, config backup/restore all work.
- DMG packaging (ad-hoc signed) works: `dist/GlintVNC-<version>-macOS-x86_64.dmg`.
- **Round 8 audit (2026-11-19):** four parallel reviews of all source/docs found and fixed a CopyRect-flood OOM (128 MiB per-update RGBA budget, 16 Mpx cap), unbounded network reads (15 s timeout + joinable workers), a frozen-frame overlay bug, scroll pulses breaking drags, UI-thread Keychain calls, the DesktopSize spec violation, signed cursor hotspots, LibVNCServer LED State, a silent auth downgrade, duplicate profile names, missing keyboard access to the session bar, stale i18n keys, and a wrong-viewport repaint. 55 tests green. Details: `docs/DESIGN_ROUND8_AUDIT.md`.
- **Owner hands-on confirmation (2026-11-19):** the app was tested on the physical Intel Mac and the production environment was declared complete. Automated gates (55 tests, release build, DMG mount, LAN probe) passed beforehand; the ad-hoc signature still requires first-launch approval.

**Signing status:** ad-hoc signature only. NOT notarized; Gatekeeper will require right-click → Open. Developer ID signing + notarization are required before real distribution. See `docs/VERIFICATION_LOG.md` for exact results and limitations.

## NOT yet implemented / NOT proven

- Intel Mac 60fps or latency targets, VeNCrypt/TLS, RA2, ARD Apple authentication, Tight/Hextile.
- Full macOS IME/dead key modifier interception, server-side remote resize.
- Formal performance benchmark of ZRLE vs Raw/CopyRect on this hardware (heuristic only).
- Production reconnect jitter/recovery is in place for transport errors only; auth failures are terminal.
- Unsigned-keychain-ACL behavior across rebuilds; multi-screen and high-DPI ergonomics; accessibility audits.

The app currently **blocks non-loopback RFB unless the user explicitly selects `legacy`** in the profile. VNCAuth does NOT encrypt the video stream. Use a trusted tunnel or wait until verified TLS is implemented. See `docs/SECURITY.md`.

## Commands

```sh
cargo run --release -p glintvnc-app                      # GUI
cargo run -p glintvnc-app -- --help
cargo run -p glintvnc-app -- profiles add "Local" 127.0.0.1 --port 5900
cargo run -p glintvnc-app -- profiles list --json
cargo run -p glintvnc-app -- secret set Local           # hidden prompt / Keychain
cargo run -p glintvnc-app -- connect Local
cargo run -p glintvnc-app -- probe Local --timeout-secs 12
cargo run -p glintvnc-app -- settings list
cargo run -p glintvnc-app -- settings set theme dark
```

To connect to a non-local address in the **temporary legacy-only milestone**, use `profiles set Local --security legacy` after reviewing security implications. Do not mistake this for encryption.

## Local test target

A self-contained loopback-only Python RFB mock server is supplied. See `tests/README.md`. It can display four color bars without another VNC server installation. It does **not** test Apple authentication or encrypted VNC.

## Product goals

Native, small-ish, ergonomic; intelligent resize, robust connectivity, keyboard remapping with explicit rules, Keychain, predictable CLI, dark UI, memory-safe networking, low-latency remote framebuffer and security by default. See `docs/PRODUCT_SPEC.md` for exact targets and exclusions.

## License and prior art

New scaffold source is MIT licensed (`LICENSE`). **No third-party project source has been copied into this kit.** RV, IronVNC and DeskVNC are cited as analysis references and integration candidates; examine their upstream licenses before vendor/forking anything.
