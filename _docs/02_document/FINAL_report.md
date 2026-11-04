# sunnycloud — Planning Report

## Executive Summary

sunnycloud is a self-hosted service that syncs files, photos and videos across web, Android, iOS and desktop (Linux, Windows, macOS) clients. The plan is a Rust API with sandboxed media workers on a single-host Docker Compose stack, PostgreSQL for metadata and plain files for content, a block-list sync protocol with a server-ordered change feed, and thin native clients around one shared Rust sync core. It decomposes into 21 components, 8 shared helpers, 2 shared crates and 32 epics (about 1,300 planned story points); 17 ADRs, a 35-risk register and about 790 test specs back it.

## Problem Statement

An operator installs the API and workers on their own infrastructure; users keep folders and camera photos in sync across all their devices, with automatic camera backup (the server thumbnails and transcodes), share links, optional end-to-end encrypted vault folders, and desktop folder sync driven by a background daemon with a tray icon. Hundreds of users per installation. No version history (last write wins), no streaming in v1.0.

## Architecture Overview

One installation = Caddy + one Rust API process + PostgreSQL 18 + an encrypted blob volume + sandboxed workers, on Docker Compose. Workers talk only to the API. Clients upload 2 MiB blocks over HTTP/2 (BLAKE3, up to 4 lanes) and follow a per-account change feed with a long-poll. Full detail: `architecture.md`, `data_model.md`, `system-flows.md`, ADRs 001-017.

**Technology stack**: Rust (axum 0.8, sqlx 0.9, tokio), PostgreSQL 18, XFS/ext4 blob volume, Svelte 5 web (sync core as WASM), Flutter + Kotlin/Swift hosts with a UniFFI Rust uploader, Slint settings window, Docker Compose, GitHub Actions.

**Deployment**: Compose release bundle with a one-shot `migrate` step; staged builds on `stage`, production on the operator's host; GitHub Actions signs, the Forgejo/Woodpecker fallback only builds and tests.

## Component Summary

| # | Component | Purpose | Epic (Jira) |
|---|-----------|---------|-------------|
| 01 | identity | sign-in, sessions, device grant, invites | E-15 (AZ-3646) |
| 02 | tree | files, folders, revisions, names, conflicts, commit transaction | E-13 (AZ-3640) |
| 03 | transfer | upload sessions, block lists, quota reservation, download | E-14 (AZ-3643) |
| 04 | blobstore | blob layout, encryption, GC, scrub, restore | E-10 (AZ-3627) |
| 05 | sync_feed | change feed, long-poll, resync, epoch | E-12 (AZ-3635) |
| 06 | sharing | share links (files only in v1.0) | E-16 (AZ-3619) |
| 07 | admin | users, invites, account deletion | E-17 (AZ-3624) |
| 07a | quota | quota ledger | E-11 (AZ-3631) |
| 08 | media_jobs | job queue, derived files | E-18 (AZ-3629) |
| 09 | worker_supervisor | claims jobs through the API, drives decoders | E-20 (AZ-3639) |
| 10 | decoder | sandboxed libvips and ffmpeg | E-19 (AZ-3634) |
| 11 | http_api | delivery layer, routes, error mapping | E-21 (AZ-3644) |
| 12 | operator_cli | `sunnycloud` binary, composition root, scheduler | E-22 (AZ-3647) |
| 13 | sync_core | sans-IO protocol, client part, WASM build | E-23 (AZ-3650) |
| 14 | desktop_daemon | watcher, LocalIndex, tray, updater | E-24 (AZ-3625) |
| 15 | desktop_settings | Slint settings window | E-25 (AZ-3628) |
| 16 | web_app | Svelte 5 SPA | E-26 (AZ-3632) |
| 17 | android_app | Kotlin host, `mobile_backup` | E-27 (AZ-3637) |
| 18 | ios_app (v1.1) | Swift host | E-28 (AZ-3641) |
| 19 | vault_core (v1.2) | client-side crypto | E-29 (AZ-3645) |
| 20 | vault_service (v1.2) | server side of the vault | E-30 (AZ-3648) |

Shared: helpers 01 config, 02 error, 03 clock, 04 audit, 05 ids_hashing, 06 names, 07 db, 08 ratelimit (cross-cutting epics E-01 to E-08, plus E-09 observability); crates `contracts` (server ports) and `media-shared`; bootstrap E-00 (AZ-3621); Blackbox Tests E-31 (AZ-3649). Mapping and dependencies: `epics.md`.

**Implementation order**:
1. E-00 bootstrap (workspace, `contracts`, CI skeleton), then E-01 to E-09 cross-cutting.
2. Server core: blobstore, quota, sync_feed, tree, transfer, identity, sharing, admin (ports built against `contracts` with recording doubles; integration passes when counterparts land).
3. Media: media_jobs, decoder, worker_supervisor; then http_api and operator_cli (composition root).
4. Clients: sync_core, then desktop daemon, settings, web, Android; blackbox harness starts after E-00/E-21.
5. v1.1: iOS, macOS, Apple sign-in, NVENC. v1.2: vault core and service after an external audit.

## System Flows

F1 first run, F2 sign-in, F3 device login, F4 large upload, F5 small upload and batch, F6 download, F7 sync, F8 conflict, F9 desktop watcher, F10 media pipeline, F11 share link, F12 GC and retention, F13 account deletion, F14 restore and resync, F15 scrub, F16 camera backup, F17 vault. Details and diagrams: `system-flows.md`, `diagrams/`.

## Risk Summary

| Level | Count | Key Risks |
|-------|-------|-----------|
| Critical | 3 | decoder compromise (mitigated), unproven performance targets (needs hardware), complexity (staged, with a fallback) |
| High | 13 | snapshot restore, Caddy HTTP/2 windows, single API process (accepted), blob-encryption review, store policy, vault format |
| Medium | 17 | — |
| Low | 2 | — |

**Iterations completed**: 1 (owner confirmed sufficient). **All Critical/High risks have a mitigation or an owner action**; open ones are listed in `risk_mitigations.md`. Reference: `risk_mitigations.md`.

## Test Coverage

Component specs (`components/*/tests.md`): 349 integration, 58 performance, 88 security and 48 acceptance tests across the 21 components (the vault service has no performance test: no AC sets a bound). System-level: 249 scenarios in `tests/` (blackbox 121, resilience 39, security 50, performance 30, resource-limit 9).

**Overall acceptance criteria coverage**: 79 of 79 ACs and 33 of 33 restrictions have at least one test (`tests/traceability-matrix.md`). Some tests are BLOCKED on small spec details or on hardware; 57 open spec gaps are indexed in `tests/spec-gaps.md` (22 need an owner decision).

## Epic Roadmap

32 epics in Jira project AZ (AZ-3619 to AZ-3650), label `sunnycloud`; local specs in `epics/`, index with dependencies in `epics.md`.

| Group | Epics | Planned points |
|-------|-------|----------------|
| Bootstrap | E-00 | 40 |
| Cross-cutting | E-01 to E-09 | 39 |
| Server components | E-10 to E-22 | 583 |
| Clients | E-23 to E-28 | 357 |
| Vault (v1.2) | E-29, E-30 | 70 |
| Blackbox tests | E-31 | 210 |

**Total estimated effort**: about 1,300 story points (dependency order is by epic id; ports with a higher-id counterpart are soft links with a late integration pass).

## Key Decisions Made

ADRs 001-017 in `adr/README.md`, all Accepted 2026-11-04: Rust everywhere; single-host Compose with one API process; PostgreSQL plus plain-file content; block-list sync over HTTP/2; last-write-wins with conflict copies; application-level blob encryption; opt-in vault (v1.2); built-in OIDC sign-in; sandboxed API-only workers; CPU-first transcoding; AGPL with App Store exception; native desktop daemon with Slint settings; shared uploader behind UniFFI; two database roles; ports-only dependencies; staged releases; offline-signed updater keys. Owner answers at Step 4 and 5: API-only workers, migrator role, offline two-key updater, UniFFI only, AC-60 reworded (parent not bound), Android accepts user-added CAs with a warning, Jira project AZ.

## Quality Checklist

Verified at the end of planning:
- Blackbox tests: every AC and restriction in the matrix; positive and negative scenarios; compose environment defined in `tests/environment.md`; CI reporting defined. Not run: nothing has been executed (documentation only).
- Architecture, interaction risks, data model, deployment, components, risks, tests, epics: checked by three independent reviews and two consistency passes; port graph is acyclic by construction (ports-only, ADR-015). Tracing: no distributed tracing in v1.0 (single process; request-id correlation instead, `deployment/observability.md`).
- Rollback is a restore from the pre-upgrade backup (added at the end of Step 6).

## Open Items for the Owner

- Hardware for the performance gates (REF-SRV, REF-CPU4, a weak Windows machine, phones, a Mac, an iPhone); until then perf scenarios report NOT-RUN.
- `COPYING.iOS` wording for the App Store exception; DCO versus CLA before any outside contribution.
- Apple and Google developer accounts, Windows signing (SignPath proposed), registry (GHCR proposed), demo server.
- Offline signing tool and update-manifest layout confirmation (ADR-017 notes).
- External review of the blob encryption before v1.0; vault review for v1.2.
- 22 spec decisions in `tests/spec-gaps.md`; undefined spike gates S6 and S13; Windows reserved-name marker and collision suffix; poster-frame tolerance; decoder image tag versus digest.
- Jira: dependency links between epics are not created; Jira table cells with a leading ">" were repaired in four epics (see the Step 6 note in `epics.md` if a repair is pending).

## Next Steps (autodev greenfield)

UI design, test-spec architecture sync, decompose (tasks from epics), implement. Commits are made at autodev commit points; nothing is committed yet.
