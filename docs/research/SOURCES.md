# Source log — checked 2026-11-18

Source webpages might change. This is a reference ledger, not a claim that a linked binary works on the exact Intel machine.

## Standards / OS

- RFB RFC6143 (IETF): https://www.rfc-editor.org/rfc/rfc6143
- IANA Remote Framebuffer registrations: https://www.iana.org/assignments/rfb
- macOS Ventura 13.7.8 Apple security release: https://support.apple.com/ja-jp/124929
- Apple Ventura 13 2017 MacBook Pro compatibility: https://support.apple.com/ja-jp/102853
- Apple NSWindow / native full screen: https://developer.apple.com/documentation/appkit/nswindow
- Apple NSEvent: https://developer.apple.com/documentation/appkit/nsevent
- Apple Security Framework: https://developer.apple.com/documentation/security

## Native UI / Rust

- eframe 0.37.0 Rust docs, uses `App::ui` as required method: https://docs.rs/eframe/0.37.0/eframe/trait.App.html
- egui upstream and accessibility: https://github.com/emilk/egui
- wgpu: https://wgpu.rs/
- Keyring 3.6.3 apple-native: https://docs.rs/keyring/3.6.3/keyring/
- keyring Entry password API: https://docs.rs/keyring/3.6.3/keyring/struct.Entry.html
- Rust `security-framework`: https://docs.rs/security-framework/latest/security_framework/
- DES cipher warning and implementation: https://docs.rs/des/latest/des/
- Tauri2 macOS Catalina+ prerequisite (alternative stack): https://v2.tauri.app/start/prerequisites/

## Prior software

- RV / GPUI: https://github.com/madeye/rv (author claims ARD, VeNCrypt, Keychain and full screen; MIT)
- IronVNC / egui: https://github.com/hkder/ironvnc (author claims RA2, Tight, ZRLE, MIT/Apache)
- DeskVNC / Tauri2: https://github.com/psmux/DeskVNC (author claims pure Rust VNC, VeNCrypt, ARD)
- TigerVNC: https://github.com/TigerVNC/tigervnc (GPL reference implementation)
- TigerVNC download: https://tigervnc.org/
- RealVNC viewer: https://www.realvnc.com/en/connect/download/viewer/macos/
- Apple VNC remote access warning: https://support.apple.com/ro-ro/guide/remote-desktop/apde0dd523e/mac

## Agent instructions

- OpenCode Skills: https://opencode.ai/docs/skills
- OpenCode AGENTS.md: https://opencode.ai/docs/instructions/
- OpenCode Commands: https://opencode.ai/docs/commands
- Rust Book: https://doc.rust-lang.org/book/
- Cargo Book: https://doc.rust-lang.org/cargo/

## Verification questions for OpenCode

1. Does `eframe 0.37.0` build and run with **Intel Metal** on the actual Ventura 13.7.8 host? If no, which older release works? Record evidence rather than assuming.
2. Can RV compile/run with the same GPU? Does its core have reusable clean crate boundaries, and what is its license of each subcomponent?
3. Do Keychain calls require signature, ACL prompts or entitlements for both CLI and packaged GUI?
4. What precise RFC security mechanisms are supported by the VNC servers the user actually intends to control?
5. Compare `Raw`, `ZRLE`, `Tight` and `CopyRect` on the local Mac before choosing the default codec order.
