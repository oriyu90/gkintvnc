# Reference application comparative analysis (verified links 2026-11-18)

**Important:** README feature claims are statements by the repositories/authors; they are not independently benchmarked on the 2017 Intel MacBook Pro. License inheritance and source import require separate review.

| Product | UI / protocol | Strengths relevant to our goal | Costs/risks | Decision |
|---|---|---|---|---|
| [RV](https://github.com/madeye/rv) | Rust + GPUI, custom RFB | native fullscreen, address book, native dark/light, Mac ARD, Keychain, Tight/ZRLE/TRLE/Raw, VeNCrypt | GPUI/Metal and exact macOS 13 Intel compatibility unverified; young repo; tight coupling to GPUI possible | PRIMARY prototype and architecture reference; evaluate before reimplementing auth |
| [IronVNC](https://github.com/hkder/ironvnc) | Rust + egui | Raw/CopyRect/Hextile/ZRLE/Tight, RA2 crypto, desktop size and cursor, early partial update optimizations | relatively small project; stores sessions beside executable according to README, verify password storage safety before reuse; feature claims unverified | compare decoder and egui handling only after review |
| [DeskVNC](https://github.com/psmux/DeskVNC) | Rust protocol core + Tauri 2 | claimed VeNCrypt/RA2/ARD/continuous updates, Keychain, VNC alongside RDP/SSH, broad compatibility | heavier WebView/JS frontend and broader scope; inspect actual module licensing/API | study protocol and resilience, avoid complex extra feature scope |
| [TigerVNC](https://github.com/TigerVNC/tigervnc) | C++/FLTK | mature reference in protocols, encoding performance and multi-platform interoperability | GPL-2.0; cannot simply copy code into MIT proprietary-compatible core without complying with GPL | gold-standard compatibility/performance oracle |
| [RealVNC Viewer](https://www.realvnc.com/en/connect/download/viewer/macos/) | proprietary | UX and commercial security expectations | licensing/account restrictions, external auth support depends on version/plan | UX inspiration only, no copying |
| [Jump Desktop](https://jumpdesktop.com/) | commercial | Mac desktop usability and fullscreen workflow | not a reusable codebase; features depend on current version | visual and workflow inspiration only |
| Apple Screen Sharing | system macOS app | baseline Mac behavior, native input and spaces | not general-purpose API/library | integration/keyboard comparison |

## Scorecard — project fit (qualitative estimate, not benchmark)

| Option | Native feel | Ease of VNC prototyping | Intel13 confidence | Protocol reuse | Startup scope |
|---|---|---|---|---|---|
| Existing RV fork | High potential | High if target builds | MUST verify | High | Low–medium |
| Custom egui+Rust (chosen) | Good potential | Medium | MUST verify | Flexible | Medium |
| Tauri2 | WebView-styled | Medium | Tauri claims macOS10.15+, verify VNC canvas speed | Flexible | Medium |
| winit/wgpu from scratch | Excellent canvas control | Low | MUST verify | Flexible | High |
| SwiftUI+Rust bridge | native OS UI | Medium | SwiftUI deployment version matters | Flexible | High integration cost |

## Decision

Build separate shared config/CLI+Keychain, start with egui because this maintains GUI/CLI code in Rust with minimal frontend overhead. **The protocol should not be permanently hand-written by default**: P0 Raw reader serves as a controlled baseline; before P2 investigate extracting a permissive Rust engine or referencing RV, DeskVNC, IronVNC modules. Do not import a GPL implementation into this MIT app without appropriate license decision.

## License checklist for any code reuse

1. Inspect repository root and EACH crate/subdirectory `LICENSE` plus `NOTICE` and vendored dependencies.
2. Record exact revision SHA and link, author attribution, SPDX conditions and dependency license.
3. Prefer importing a crate with an explicit stable API; minimize wholesale vendoring.
4. Audit unsafe, TLS and secret handling, compression bombs, long-lived maintenance, automated tests, open issues.
5. If GPL is chosen, consciously change project distribution obligations and bundle corresponding source/license; don't silently mix.
