# ADR-001: Rust for the API, workers, desktop agents and shared client core

- **Status**: Accepted
- **Date**: 2026-11-04
- **Deciders**: the project owner
- **Supersedes**: —
- **Superseded by**: —

## Context

The product spec fixes the implementation language of the server side before research starts: "Server components (API, workers, desktop agents) are written in Rust for maximum performance" (`problem.md`). This ADR records that owner decision and the project forces that back it, so later readers do not reopen it.

- Acceptance criteria addressed: AC-2 (single transfer at 90% of 1 Gbps with blob encryption), AC-1 (500 users, 100 concurrent sessions), AC-5 (5 s p95 sync under 100 background uploads), AC-15 (daemon at most 100 MB and 1% idle CPU with 100,000 watched files), AC-43 (encryption at rest meets AC-2), AC-52 (one upload implementation for every mobile entry point, at most 150 MB), AC-62 (vault key never in a browser heap scan)
- Restrictions addressed: RESTRICT-05 (Rust for API, workers and desktop daemons; Svelte 5 web; Flutter for mobile with Kotlin/Swift where needed), RESTRICT-06 (native Rust daemon, Slint settings), RESTRICT-11 (axum/tokio with a hand-built hyper accept loop), RESTRICT-15 (one Rust uploader for mobile through UniFFI), RESTRICT-23 (vault crypto as one Rust core: native, WASM, UniFFI)
- Risks addressed: R15 (21 components, about 50 port traits; one language keeps the shared crates simple), R27 (100 MB daemon budget), R31 (UniFFI/NDK problems on Android)
- Research source: `_docs/00_research/02_fact_cards/mve_round2.md` (blob envelope encryption measurement), `_docs/00_research/06_component_fit_matrix.md`

The same protocol logic (blocklist planning, BLAKE3 hashing, resume checkpoints, vault crypto) must run in four places: the server, the desktop daemon, the Android/iOS uploader and the browser. A byte-identical second implementation per platform is a correctness risk on a sync protocol, and the daemon and uploader have hard memory ceilings. The language choice therefore decides how many protocol implementations exist.

## Decision

We will write the API, the media supervisor and decoder runner, the operator CLI, the desktop daemon and settings window, and the shared client crates (`sync-core`, `mobile_backup`, `vault_core`) in Rust, in one Cargo workspace.

The Rust core reaches the other client technologies through generated bindings, not rewrites: `sync-core` builds to WASM for the Svelte 5 web app, and `mobile_backup` is exposed through UniFFI to Kotlin (Swift in v1.1) under the Flutter app, with no flutter_rust_bridge (owner answer S4-4). Svelte 5 (web) and Flutter (mobile) are separate restrictions (RESTRICT-05); this ADR does not decide or revisit them.

Server and workers use stable Rust with the MSRV set by notify 8.2 (1.88): axum 0.8.9 on hyper 1.12 and tokio 1.54.1 or later, sqlx 0.9, blake3 1.8.7, chacha20poly1305 =0.11.0. The server is one binary, `sunnycloud`, with subcommands (`serve`, `migrate`, `doctor`, `admin ...`); the worker supervisor and `decoder-runner` are separate Rust binaries so the decoder image carries no database or storage code.

## Alternatives Considered

| Alternative | Rejected because |
|-------------|------------------|
| .NET (ASP.NET Core) for server and workers | Owner constraint (RESTRICT-05). The desktop daemon is Rust either way, and UniFFI generates the Kotlin/Swift bindings from Rust only, so the protocol, hash and vault code would exist twice, against AC-52 ("one upload-protocol implementation") and RESTRICT-23 ("one shared Rust core"). |
| Go for server and workers | Owner constraint. Same duplication: no UniFFI generator for a Go core, so the mobile uploader and the browser need their own implementation of the blocklist protocol and vault crypto. |
| Python for workers and tooling | Owner constraint. AC-2 headroom is measured only for Rust (single-core XChaCha20 decrypt 1.16-1.25 GB/s against 0.112 GB/s needed, `mve_round2.md` MVE 2), and a Python worker would not share the job DTOs and transcode policy in `media-shared` with the Rust API. |
| Rust server with a different language per client family (Kotlin, Swift, TypeScript implement the protocol natively) | Three more protocol implementations to keep byte-compatible; AC-52 and RESTRICT-15 forbid a second mobile upload path; the vault construction (AC-60, AC-61) would need independent reviews. |

## Consequences

### Positive

- One protocol implementation: the same `sync-core` serves the daemon, the Android uploader and the browser, and the server shares the hashing and block-size constants through the workspace.
- Measured headroom for the hot path: decrypt at 1.16-1.25 GB/s per core against the 0.112 GB/s the 1 Gbps target needs, so AC-2 does not depend on tuning the cipher.
- One toolchain for CI (`cargo audit`, `cargo deny`, RESTRICT-12) and one binary for all operator commands.
- Memory-safe language for the parts that parse untrusted network input; the untrusted media parsers (ffmpeg, libvips) stay in C but are isolated in the decoder container (R01).

### Negative

- Smaller contributor pool and slower builds than a managed-runtime stack; MSRV is pinned by a dependency (notify 8.2 sets 1.88), so a crate bump can force a toolchain bump.
- Cross-compiling the daemon for Windows and later macOS, WASM, and Android NDK/16 KB page-size builds is extra CI work (R31).
- Several security-relevant crates have no stated audit (`aead-stream`, R08, R23), and the hand-built random-access STREAM nonces need known-answer tests and an external review.
- The component graph depends on about 50 port traits to avoid crate cycles (R15); Rust makes that explicit but costly.

### Neutral / Open

- The web app stays Svelte 5 and the mobile UI stays Flutter; their build and test stacks are unchanged by this ADR.
- Black-box tests are Python 3.13 and pytest (`tests/environment.md`); the language rule covers shipped product code, not the external test consumer.
- Performance numbers for AC-2 through Caddy are still a runtime gate (spike S1, R04), not settled by the language choice.

## Evidence

- `_docs/00_problem/problem.md` (line "Server components (API, workers, desktop agents) are written in Rust")
- `_docs/00_problem/restrictions.md` § Technology
- `_docs/02_document/architecture.md` § 2. Technology Stack
- `_docs/02_document/architecture.md` § Component list (dependency rule, shared crates `contracts` and `media-shared`)
- `_docs/02_document/canonical-names.md` § Owner decisions at Step 4 (2026-11-04) (S4-4, UniFFI only)
- `_docs/02_document/components/13_sync_core/description.md` § 1. High-Level Overview
- `_docs/02_document/components/17_android_app/description.md`
- `_docs/00_research/02_fact_cards/mve_round2.md` § MVE — RustCrypto chacha20poly1305 0.11.0 (decrypt measurement)
- `_docs/02_document/risk_mitigations.md` § Risk Register (R15, R27, R31)

## Notes

The research folder holds no benchmark of .NET, Go or Python alternatives; they were never evaluated against the AC set. The rejection rests on the owner constraint and on the shared-core requirement (RESTRICT-23, AC-52), not on measured slowness. Re-open only if the owner drops the shared-core model.
