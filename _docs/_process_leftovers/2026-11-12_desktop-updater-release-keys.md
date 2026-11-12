# Leftover: desktop updater release keys (owner, before the first desktop release)

kind: owner
blocked action: pin the two Ed25519 updater public keys (active and recovery) in the daemon's trust file (`src/desktop/daemon/src/updater/trust.rs`) so a release build accepts signed manifests and `update-sign` can sign one
reason: no real key pair exists yet; release builds pin none, so every manifest is refused and the offline signing tool refuses to sign (AZ-3916, batch 48). Owner decision 2026-11-12: create them before the first desktop release.

Owner steps (ADR-017): generate the active and the recovery key pair on an offline machine; keep two offline copies of each private key; hand over only the two PUBLIC keys. Then a small change pins them in `trust.rs` and the release build's `update-sign` works with the active key file.

Replay: when the public keys are provided, pin them, run the daemon updater tests and a release build of `update-sign` against a test manifest, and delete this file.
