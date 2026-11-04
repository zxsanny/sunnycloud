# ADR-004: Blocklist sync over HTTP/2 with a server-ordered change feed

- **Status**: Accepted
- **Date**: 2026-11-04
- **Deciders**: the project owner
- **Supersedes**: —
- **Superseded by**: —

## Context

Clients on web, Android and desktop must upload files from a few bytes to 50 GiB, resume after interruption, move 10,000 small files quickly, and learn about changes made on other devices within seconds. Client clocks cannot be trusted, and the same wire protocol must be implementable from a browser, a phone and a daemon.

- Acceptance criteria addressed: AC-2 (90% of link speed up to 1 Gbps), AC-3 (resume re-sends at most 10 MiB), AC-4 (50 GiB, constant-time finish), AC-5 (5 s p95 for files up to 10 MB), AC-8 (later commit by server order wins, conflict copy for stale base), AC-38 (block acknowledged only after flush), AC-40 (10,000 files up to 1 MiB in 60 s at 20 ms RTT), AC-41 (`resync_required` after restore), AC-69 (50 concurrent commits, feed shows each change once in order), AC-1 (70% idle long-poll sessions), AC-21 (revocation ends held streams within 5 s)
- Restrictions addressed: RESTRICT-11 (fixed 2 MiB blocks, at most 4 lanes, BLAKE3, server-assigned commit order, single request up to 2 MiB, batch commit, long-poll, no HTTP/3, no content-defined chunking), RESTRICT-09 (`fdatasync` before ack, `epoch.seq` cursor, `resync_required`), RESTRICT-32 (batch commits idempotent by client commit ID), RESTRICT-24 (last commit wins, stale edit becomes a conflict copy)
- Risks addressed: R04 (HTTP/2 windows through Caddy), R07 (per-account serialisation), R14 (block-hash rewrites), R29 (staged-file cap throttles bulk imports), R27 (daemon in-flight buffers), R02 (restore and epoch)
- Research source: `_docs/00_research/raw/sync_protocol.md` § Options comparison tables (T1, T2, T3, T5)

Block size and lane count follow from AC-3: worst-case re-send is block size times lanes, so 2 MiB times 4 lanes gives 8 MiB, while 4 MiB times 4 gives 16 MiB and violates it (T2). Ordering follows from AC-8 and AC-69: a Postgres sequence is not commit-ordered, so a poller reading "id greater than cursor" can skip a late-committing lower id (`sync_protocol.md` F19).

## Decision

We will sync with a Dropbox-style blocklist protocol over HTTPS and HTTP/2: fixed 2 MiB blocks hashed with BLAKE3, up to 4 parallel lanes, a single-request path for files up to 2 MiB plus an idempotent batch commit, server-assigned commit order, and a per-account `epoch.seq` cursor served by a held long-poll.

**Upload.** `POST /api/v1/uploads` reserves quota and returns `need_blocks` and `lane_limit`; each block is sent with `PUT /uploads/{id}/blocks/{index}` carrying `X-Blake3`, hashed and rejected on mismatch before any write, and answered with 204 only after the group-commit `fdatasync`. `POST /api/v1/commits` publishes the file by comparing the client's blocklist with the stored per-block hashes, so finishing is O(1) in file size. Files up to 2 MiB use `PUT /staged-uploads/{key}` and are published in groups by `POST /api/v1/commits:batch` (at most 500 entries) with a client `commit_id`; the server stores a receipt keyed by `(account, commit_id)` with a request hash, replays it for the same hash and answers 422 `commit_id_reused` for a different one. Each change carries `base_revision`; a stale base or a name collision produces a conflict copy, and an edit beats a delete.

**Order and feed.** The commit transaction bumps `account_feeds.seq` as its last statement and stamps it on every touched `nodes` row, so `seq` is gapless and equals commit order. `GET /api/v1/changes?cursor=E.S&wait=30` returns the latest state of nodes with `seq > S`; with nothing new it parks up to 30 s, woken by the in-process notifier after COMMIT. It answers HTTP 410 `resync_required` when the epoch differs, `S` is below the purge horizon (tombstones kept 90 days) or `S` is above the head.

## Alternatives Considered

| Alternative | Rejected because |
|-------------|------------------|
| Rolling-hash or content-defined chunking with rsync-style delta (FastCDC, Seafile) | Creates a permanent cut-point contract and FastCDC has several incompatible implementations; poor fit for already compressed photos and video (`sync_protocol.md` F33, T1 rank 5). RESTRICT-11 excludes it. Cost: a large edited file is re-sent whole. |
| WebDAV chunk folder plus MOVE (Nextcloud), or rsync | WebDAV chunking costs MKCOL/MOVE round trips and Nextcloud users see about 10 files/s (F7) against AC-40's 10,000 files in 60 s (at least 167 files/s). rsync's rolling checksum was not verified in research (F11) and has no browser client. |
| tus or S3-style multipart | tus streams over one TCP window per file and has no per-block verification; S3 parts start at 5 MiB, which makes AC-3 tight with several lanes (T1 ranks 2-3). |
| WebSocket or SSE push in place of long-poll | Push still needs the cursor pull for catch-up, so it adds a second mechanism. SSE is buffered by proxies, `EventSource` cannot set an auth header and HTTP/1.1 allows 6 connections per origin; WebSocket's bidirectionality goes unused; Nextcloud `notify_push` still advises a poll fallback (T5, F35-F36). A held long-poll meets AC-5 with one endpoint. |
| Client-ordered logical clocks, version vectors or client mtime | AC-8 requires server order, not client clocks; wall-clock last-write-wins loses writes under skew (F18). Version vectors (Syncthing) add per-device state and a merge protocol the "last commit wins" model does not need. |
| Global database sequence as the cursor | Not commit-ordered: late commits are skipped (F19); the per-account counter taken under the account lock avoids it. |

## Consequences

### Positive

- Resume is bounded by construction: at most 2 MiB times 4 lanes (8 MiB) is lost, inside AC-3, and the received-bitmap is checkpointed every about 8 MiB or 1 s.
- Small files cost one request plus a shared batch commit; replaying a lost batch response is harmless.
- Conflict handling needs no client clocks; the feed is a plain cursor read, so catch-up and wake-up are the same endpoint and idempotent apply is easy for clients.
- The protocol is plain HTTPS: it works in a browser (Web Worker, WASM `sync-core`), on a phone and in the daemon with one shared client implementation.

### Negative

- No delta transfer: editing a large file re-uploads it whole; fixed blocks also shift on insertions.
- Hot per-account counter row and advisory lock serialise commits (R07); `block_hashes` is rewritten at each checkpoint (R14).
- The feed carries latest state only; a node changed ten times appears once, so clients cannot replay intermediate states.
- Every idle device holds a long-poll (AC-1: 70% of sessions); connection limits, proxy timeouts above 30 s and per-principal hold caps become capacity settings.
- Correctness after a restore depends on epoch discipline (`admin epoch-bump`, `.epoch` marker, R02). The staged-file cap (default 1,000 per account) throttles bulk imports (R29).
- The Caddy-to-client HTTP/2 window must sustain 1 Gbps; this is gate S1, unproven today (R04).

### Neutral / Open

- HTTP/3 is not used (up to 45% slower on fast links, `sync_protocol.md` T3); it can be revisited for lossy mobile links if measured.
- Hash and block size are fixed in v1.0; changing them is a protocol version change under `/api/v1` and `min_client_version`.
- Content is never matched across accounts: the only hash lookup is `POST /api/v1/revisions:lookup` for reinstall rebuild.

## Evidence

- `_docs/00_problem/restrictions.md` § Technology (Sync protocol bullet)
- `_docs/02_document/system-flows.md` § Flow F4: Large-file upload, § Flow F5: Small-file upload and batch commit, § Flow F7: Change-feed long-poll sync, § Flow F8: Conflict handling
- `_docs/02_document/components/03_transfer/description.md` § 3. Internal Interfaces and § 4. External API Specification (owned resources; handlers live in 11)
- `_docs/02_document/components/05_sync_feed/description.md` § 3. External API Specification (resources owned; routed by 11_http_api)
- `_docs/02_document/data_model.md` § 7.2 Rules for `epoch.seq`, § 7.3 `commit_receipts` (proposal P2, owned by 03 transfer), § 6.3 `upload_sessions`
- `_docs/02_document/components/13_sync_core/description.md` § 3. Behaviour that matters
- `_docs/02_document/components/11_http_api/description.md` § 4. Listeners and Accept Loop (HTTP/2 windows)
- `_docs/00_research/raw/sync_protocol.md` § Options comparison tables (T1, T2, T3, T5)
- `_docs/00_research/04_reasoning_chain.md` § Dimension 3: Transfer protocol and transport and § Dimension 4: Notification and ordering
- `_docs/02_document/risk_mitigations.md` § Risk Register (R02, R04, R07, R14, R29)

## Notes

Re-evaluate chunking only if users routinely edit multi-GiB files (the research trigger for delta) and a measured re-upload cost exceeds an agreed budget. Re-evaluate push (SSE) if long-poll request churn shows up in NFT-PERF-01. Reasoning on the 8 MiB bound: `sync_protocol.md` T2.
