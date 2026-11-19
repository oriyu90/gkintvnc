# Rust library and AI-skill investigation

Primary sources were checked on **2026-11-18**; exact Cargo resolution and macOS 13 runtime support must be proven by OpenCode on the machine.

## Core implementation choices

| Library / capability | Role | Current plan |
|---|---|---|
| [egui](https://github.com/emilk/egui), [eframe](https://docs.rs/eframe/0.37.0/eframe/) | Rust GUI and native window | v0.37 prototype; beware `App::ui` API; Intel GPU verification required |
| [wgpu](https://wgpu.rs/) | Metal accelerated rendering | included through eframe; direct region upload phase P3 |
| [tokio](https://tokio.rs/) | async sockets, cancellation, timeouts | core event loop |
| [serde](https://serde.rs/), [serde_json](https://docs.rs/serde_json/) | profiles and settings | versioned schema, atomic changes |
| [keyring](https://docs.rs/keyring/3.6.3/keyring/) | macOS Keychain | 3.6.3 apple-native; if bump to 4.x, read breaking changes |
| [security-framework](https://docs.rs/security-framework/) | richer native Keychain ACL/Trust APIs | investigate for signed builds and certificate pinning |
| [thiserror](https://docs.rs/thiserror/) | precise error taxonomy | used in core |
| [clap](https://docs.rs/clap/) | CLI subcommands and help | used in app |
| [des](https://docs.rs/des/0.8.1/des/) | legacy DES challenge-response in RFC 6143 | compatibility only; NEVER mistaken for transport encryption |
| [rustls](https://github.com/rustls/rustls) | authenticated TLS | phase P2; no unchecked cert flags |
| [tracing](https://github.com/tokio-rs/tracing) | diagnostic events | P1; redact host/secret fields according to policy |
| [proptest](https://github.com/proptest-rs/proptest), [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) | malformed RFB, state machine testing | P1–P4 |
| [cargo-nextest](https://nexte.st/) | fast parallel test execution | optional CI acceleration |
| [cargo-deny](https://embarkstudios.github.io/cargo-deny/) | license and dependency risk | P4 before release |
| [cargo-audit](https://github.com/rustsec/rustsec) | RustSec vulnerability checks | P4 before release |
| [cargo-flamegraph](https://github.com/flamegraph-rs/flamegraph) | CPU flamegraph, profiling | optional; verify perf tooling on Ventura first |
| [criterion](https://github.com/bheisler/criterion.rs) | decoder microbench | optional P3 |

**Do not install all of these immediately.** Keep binaries few, dependency graph controlled and target-specific. `rustfmt`, `clippy`, `cargo test` are the initial required tools. Build optional fuzz, audit, benchmark tools only when necessary, after examining dependencies and their maintenance.

## OpenCode AI skill standard

Official OpenCode docs: [Skills](https://opencode.ai/docs/skills), [Rules](https://opencode.ai/docs/instructions/), [Commands](https://opencode.ai/docs/commands). A skill is `.opencode/skills/<id>/SKILL.md`, frontmatter with `name` and `description` and task instructions. `AGENTS.md` contains universal invariants. No untrusted automatic skill installer in this kit.

Bundled project skills:

- `rust-mac-build`: diagnose Intel Ventura toolchain / wgpu and CI.
- `rfb-protocol`: RFC6143 parsing, auth negotiation and malformed byte test cases.
- `keychain-security`: OS secrets, secure transport, threat model and licenses.
- `input-mapping`: macOS Command, Fn, IME, modifier and X11 keysym semantics.
- `rendering-performance`: partial GPU updates, high DPI and latency.
- `reconnect-reliability`: state machine, structured faults, cancellation.
- `ui-polish`: dark/light GUI typography, native full-screen, accessibility.
- `regression-qa`: test evidence, error injection, release gating.

## Third-party skill options (study, don't blindly install)

- [Agent Skills specification](https://agentskills.io/specification): format and organization; inspect code before use.
- [OpenCode official docs](https://opencode.ai/docs/skills): authoritative install/discovery behavior.
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/): recommended idioms, useful as a *reference* to custom Rust skill.
- [Rust Secure Code WG](https://rustsec.org/): advisory ecosystem, important for network parsers.
- [Apple AppKit docs](https://developer.apple.com/documentation/appkit/): input/fullscreen and accessibility.
- [RFC 6143](https://www.rfc-editor.org/rfc/rfc6143): protocol contract, not a sample implementation.

The project-local skills have higher relevance than generic "beautiful frontend" skills because remote framebuffer, native input, and secure VNC protocol work require domain-specific constraints. Add third-party skills only after inspecting licenses, instructions and any commands they can run.
