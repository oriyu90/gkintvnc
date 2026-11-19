# GlintVNC

A native VNC viewer for **Intel x86_64 Macs (macOS 13 Ventura or later)**, written in Rust with a complete Japanese and English interface.

- **Author:** Yuki_Orita (折田悠希 / おりたゆうき)
- **License:** MIT — see [LICENSE](LICENSE) for the full text and third-party notices
- **Repository:** https://github.com/oriyu90/gkintvnc
- **Release:** https://github.com/oriyu90/gkintvnc/releases/tag/v1.0.0
- **Website:** https://studio-rizi.pages.dev/projects/gkintvnc/

## What it does

GlintVNC connects to an RFB/VNC desktop and shows the remote screen in its own window (or fullscreen). It supports RFB 3.3 / 3.7 / 3.8 with VNC authentication and the Raw, CopyRect and ZRLE encodings. Screen updates are streamed as small dirty rectangles and uploaded to the GPU partially, so dragging windows stays responsive.

The interface is fully localized in **Japanese and English**: connection list, profile editor, settings, and the in-session control bar. Display scale (Fit / 1:1 / Stretch) and keyboard mapping are configured per connection. The session bar offers **Refresh (再描画 / ⌘R)** to clear stale pixels, **F8** to show/hide the bar, and **Esc** to leave fullscreen.

Passwords are stored only in the macOS Keychain — never in the settings JSON, logs, or command-line arguments. Remote framebuffer data is treated as untrusted: decoding uses strict bounds, a 128 MiB per-update RGBA budget, a 16 Mpx framebuffer cap, and network read timeouts.

## Download and first launch

Download `GlintVNC-1.0.0-macOS-x86_64.dmg` from the [v1.0.0 release](https://github.com/oriyu90/gkintvnc/releases/tag/v1.0.0), mount it and drag GlintVNC to Applications.

The DMG is **ad-hoc signed and not Apple-notarized**. On first launch, right-click the app and choose **Open** (or allow it in **System Settings → Privacy & Security**). Because the ad-hoc signature changes with every rebuild, macOS Keychain may ask for approval again after installing a new build.

## Build from source

```sh
bash scripts/check.sh          # format, check, 55 tests, clippy
bash scripts/package-macos.sh  # build the ad-hoc-signed .app and DMG
```

Requires Rust 1.100+ with the `x86_64-apple-darwin` target and macOS 13 SDK.

## Command line

The same binary provides `profiles`, `settings`, `secret`, `probe` and `connect` subcommands. Run `glintvnc --help` for details.

## Support

Bug reports and announcements: [Discord](https://discord.gg/x7KXhNTD8M) · [GitHub Issues](https://github.com/oriyu90/gkintvnc/issues) · [X](https://x.com/InovateofRIZI) · [Studio Rizi](https://studio-rizi.pages.dev/)
