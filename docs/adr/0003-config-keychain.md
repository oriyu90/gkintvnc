# ADR-0003: Shared versioned profile model, separate Keychain secrets
Status: ACCEPTED (2026-11-18)

Context: Need GUI/CLI parity, profile persistence, crash safety and macOS-friendly password storage. Storing plaintext in JSON is forbidden.

Decision: `~/Library/Application Support/GlintVNC/config.json` (or `GLINTVNC_DATA_DIR/config.json` test override); immutable profile IDs. Keychain service `dev.glintvnc.viewer.v1` / account profile ID. Atomically rename temporary JSON; later add migration/backup.

Consequences: CLI and GUI must share signing identity where feasible; changing app signature/bundle ID can prompt Keychain authorization. Profile removal tries to clear Keychain; if it fails, warn clearly. Keychain should never be silently replaced with plaintext fallback.
