# Kit structural validation — 2026-11-18

- Validated JSON (`opencode.json`), all Cargo manifests (TOML), macOS Info.plist, shell script syntax (`bash -n`), Python mock syntax.
- Confirmed **8 OpenCode skills**, **4 OpenCode commands** and all workspace files present.
- Ran a live Python socket handshake test against the local test server with a 2×2 frame; it passed (separate from this static packaging check).
- **Rust code was NOT compiled or executed.** No Rust toolchain, macOS Ventura or Metal Intel GPU is present in the kit authoring environment. Tests and binaries are not guaranteed to work until P0 physical Mac validation.
- No signing/notarization performed; no VNC TLS, Apple ARD, Tight/ZRLE or native IME claimed implemented.
- Structural input digest (pre-validation report): `abab0913c0f0eb722d03a3955fce089c138742eba891c135ccaf0aa07657432b`.

Run `bash scripts/bootstrap-macos.sh` and `bash scripts/check.sh` on the actual Intel Mac, then record the result in `docs/VERIFICATION_LOG.md`.
