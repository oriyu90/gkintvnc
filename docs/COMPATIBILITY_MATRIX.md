# Compatibility and benchmark matrix (all untested until recorded)

## Physical host

| Item | Target | Test method |
|---|---|---|
| OS | macOS 13.7.8 Ventura | `sw_vers` |
| CPU | Intel x86_64 (MacBook Pro 2017) | `uname -m`; `sysctl -n machdep.cpu.brand_string` |
| SDK | Xcode/CLT version compatible with Ventura | `xcode-select -p`, `xcrun --show-sdk-version` |
| GPU | 2017 Intel graphics; Metal supported? verify actual model | System Report > Graphics/Displays; run wgpu sample |
| Retina | built-in Retina vs external HDMI | pointer scale test, 125–200% zoom |
| Full screen | native macOS Space transition | launch, Esc/toolbar, Cmd-Tab, sleep/wake |
| Binary | x86_64 Mach-O | `file target/release/glintvnc`; `otool -L` |

## VNC server test matrix

| Server | Platform | Auth/transport | Encodings | Status |
|---|---|---|---|---|
| controlled mock | localhost | None / VNCAuth | Raw, CopyRect, ZRLE | PASS (test harness in-tree) |
| x11vnc 0.9.17 (LibVNCServer) | Linux test host (`:0`, physical display of the OptiPlex, user-authorized) | VNCAuth (legacy, `-usepw`) | Raw, CopyRect, ZRLE (client implements + advertises first); **LibVNCServer LED State `0xFFFE0000` (Round 8)** | PASS (LAN live probe + render; dirty-rect streaming measured; ZRLE traffic observed) |
| TigerVNC (Xtigervnc) | (not exercised yet on this project; prior documentation misidentified the LAN server as this — corrected 2026-11-19) | — | — | UNVERIFIED |
| x11vnc | Linux | password, external tunnel | Raw/Hextile | TODO |
| TightVNC | Windows/Linux | VNCAuth | Tight | TODO |
| macOS Screen Sharing | macOS | legacy VNC password, later ARD username/password | as offered | TODO |
| QEMU VNC | local VM | selected server config | Raw, later compression | TODO |
| RealVNC | licensed compatible endpoint | RA2 etc | as available | TODO / licensing check |

Only connect to machines you own or are authorized to administer. Never open raw VNC port 5900 to the Internet for testing.

## Resolution/refresh matrix

| Remote framebuffer | Approx RGBA32 size | Expected P0 concern | Test |
|---|---:|---|---|
| 800×600 | 1.83 MiB | low | border/aspect/scroll |
| 1920×1080 | 7.91 MiB | normal | input latency, steady CPU |
| 2560×1440 | 14.06 MiB | moderate | high DPI |
| 3840×2160 | 31.64 MiB | full-frame copies costly | dirty area, peak RSS |
| 5120×2880 | 56.25 MiB | GPU updates costly | 5K framebuffer memory |
| 7680×4320 | 126.56 MiB | heavy / safeguard | limit and pressure recovery |

**Metrics:** median/p95 connection setup, first-frame time, frame age, rect decode cost, UI render cost, bytes/s, CPU %, resident memory (RSS), GPU utilization where available, reconnect success rate. Test 60s idle, 60s scrolling, 10-minute streaming, sleep/wake and forced network interruptions. `time` and `sample`, `powermetrics` only with explicit approval where it requests privilege. Avoid invented performance numbers.

**Round 8 hardening:** server-sent RGBA is capped at 128 MiB per update (full-frame CopyRect flood cannot OOM); framebuffer <=16 Mpx; all reads have a 15-second inactivity timeout; DesktopSize forces a non-incremental follow-up request; negative/signed cursor hotspots are clamped.

## Edge cases

- 16-bit and non-true-color server format; requested 32bpp response accuracy; byte order and RGB shift.
- International keyboard: US/JIS hardware layouts, Option+2/3, `@`, `\\`, Yen, emoji, dead keys and Japanese conversion.
- Multi-screen remote geometry; DesktopSize / ExtendedDesktopSize; 1:1 and fit.
- Multiple connections/session closing; Keychain locked or user denies access.
- IPv6 `[::1]`, Bonjour `.local`, DNS failure, VPN route changing, server restart.
- Wi-Fi roam, battery low-power mode, AirPlay/external monitor resolution transitions.
