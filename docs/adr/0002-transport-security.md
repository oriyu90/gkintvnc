# ADR-0002: Insecure VNC must fail closed by default
Status: ACCEPTED (2026-11-18)

Context: RFC 6143 None and DES challenge/response do not provide encrypted screen/control transport. Most user's network topology is unknown. Complicated TLS/Apple ARD compatibility cannot be assumed.

Decision: P0 LocalhostOnly default; explicit per-profile AllowLegacyUnencrypted for non-loopback, with warning. P2 native verified TLS/VeNCrypt and optional external-tunnel workflow, no silent downgrade after TLS failure.

Consequences: Some older VNC servers initially fail to connect without explicit opt-in; that is intentional. Later default switches to authenticated encrypted transport when available. Never claim the current `localhost-only` mode is end-to-end encryption.
