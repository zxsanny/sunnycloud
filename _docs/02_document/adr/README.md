# Architecture Decision Records

This index lists every ADR for this project, in number order. ADRs are immutable once `Accepted` —
new decisions that overturn a prior ADR are recorded as new ADRs whose `Supersedes` field points
back, and the original ADR's `Superseded by` field is updated.

| # | Title | Status | Date | Supersedes |
|---|-------|--------|------|------------|
| 001 | [Rust for the API, workers, desktop agents and shared client core](001_rust-for-server-workers-and-desktop.md) | Accepted | 2026-11-04 | — |
| 002 | [Single-host Docker Compose with one API process](002_single-host-compose-with-one-api-process.md) | Accepted | 2026-11-04 | — |
| 003 | [PostgreSQL for metadata, plain ID-named files for content](003_postgresql-metadata-with-plain-file-content.md) | Accepted | 2026-11-04 | — |
| 004 | [Blocklist sync over HTTP/2 with a server-ordered change feed](004_blocklist-sync-over-http2-with-server-ordered-feed.md) | Accepted | 2026-11-04 | — |
| 005 | [Last write wins, stale edits become conflict copies, no version history or user trash](005_last-write-wins-with-conflict-copies.md) | Accepted | 2026-11-04 | — |
| 006 | [Application-level envelope encryption of every blob](006_application-level-envelope-encryption-of-blobs.md) | Accepted | 2026-11-04 | — |
| 007 | [Opt-in client-side encrypted vault (v1.2)](007_opt-in-client-side-vault.md) | Accepted | 2026-11-04 | — |
| 008 | [Built-in OAuth/OIDC sign-in, server-owned sessions and device grant](008_built-in-oidc-sign-in-and-device-grant.md) | Accepted | 2026-11-04 | — |
| 009 | [Sandboxed media pipeline with API-only workers](009_sandboxed-media-pipeline-with-api-only-workers.md) | Accepted | 2026-11-04 | — |
| 010 | [CPU-first transcoding with opt-in GPU](010_cpu-first-transcoding-with-opt-in-gpu.md) | Accepted | 2026-11-04 | — |
| 011 | [AGPL-3.0 licence with an App Store exception](011_agpl-licence-with-app-store-exception.md) | Accepted | 2026-11-04 | — |
| 012 | [Native desktop daemon with a separate Slint settings process](012_native-desktop-daemon-with-separate-slint-settings.md) | Accepted | 2026-11-04 | — |
| 013 | [Shared Rust uploader behind UniFFI for mobile](013_shared-rust-uploader-behind-uniffi-for-mobile.md) | Accepted | 2026-11-04 | — |
| 014 | [Two database roles with row-level security as defence in depth](014_two-database-roles-with-row-level-security.md) | Accepted | 2026-11-04 | — |
| 015 | [Ports-only dependencies between components](015_ports-only-component-dependencies.md) | Accepted | 2026-11-04 | — |
| 016 | [Staged releases v1.0, v1.1 and v1.2](016_staged-releases-v1-0-v1-1-v1-2.md) | Accepted | 2026-11-04 | — |
| 017 | [Release signing and offline updater keys](017_release-signing-and-offline-updater-keys.md) | Accepted | 2026-11-04 | — |
