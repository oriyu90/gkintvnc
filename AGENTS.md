# Persistent OpenCode workspace instructions

Product: GlintVNC. Target: **Intel x86_64 MacBook Pro 2017 / macOS Ventura 13.7.8**. This environment is a hard requirement, not an optional CI target.

## Read order
1. `OPEN_CODE_START_PROMPT.md`
2. `docs/PRODUCT_SPEC.md`, `docs/ARCHITECTURE.md`, `docs/ROADMAP.md`
3. `docs/SECURITY.md`, `docs/COMPATIBILITY_MATRIX.md`
4. `docs/VERIFICATION_LOG.md`, `docs/research/SOURCES.md`

## Non-negotiable rules
- Prefer Rust safe APIs; `#![forbid(unsafe_code)]` in protocol and core. Any AppKit/Metal FFI must live in isolated adapter crate with documented safety invariants and separate audit.
- Use ONE profile/settings schema shared by GUI and CLI. Never store passwords in that schema.
- Network decoding is untrusted. Bound lengths/pixels/rectangles/decompressed output. Unsupported codecs/auth => explicit error.
- Fail closed on insecure transport by default; no silent security downgrade. VNC DES auth is not encrypted transport.
- Support atomic saves, reasonable backups/migrations, Keychain credential lifecycle, no secrets in logs or process args.
- Never block UI thread on the network, synchronous decoding, filesystem, DNS or interactive Keychain prompts; move to workers where practical.
- Connection states must be explicit; release held key and mouse state on disconnect, focus loss and reconnect.
- Keep CPU and RAM usage measurable on the **physical Intel test Mac**, not assumed from benchmarks on Apple Silicon.
- Every substantive change: `cargo fmt`, `cargo clippy`, `cargo test`, manual compatibility test if behavior affected; record evidence.
- No production-ready claim until QA gate and security test matrix pass. This ZIP starts at P0, not P5.

## Skills
Read `.opencode/skills/<name>/SKILL.md` as relevant. Do not fetch/rely on unverifiable third-party skills blindly. Use `/glint-plan`, `/glint-implement`, `/glint-review`, `/glint-qa` commands for repeatable execution.

## First response from agent
Summarize discovered environment and current compile status, propose smallest patch set for P0, then implement and verify. Avoid unnecessary questions when file evidence is present.
