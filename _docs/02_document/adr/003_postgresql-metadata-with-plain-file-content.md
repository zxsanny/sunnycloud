# ADR-003: PostgreSQL for metadata, plain ID-named files for content

- **Status**: Accepted
- **Date**: 2026-11-04
- **Deciders**: the project owner
- **Supersedes**: —
- **Superseded by**: —

## Context

The server must keep a per-user tree with atomic moves, a gapless change feed, a media job queue and quota counters, and it must store files up to 50 GiB with encryption at rest, resumable uploads and byte-range reads. Metadata has several writers (API requests and janitor loops) and must be backed up with the blobs as one consistent set.

- Acceptance criteria addressed: AC-4 (50 GiB, constant-time finish), AC-2 (90% of 1 Gbps including decrypt), AC-6 (listing of 10,000 entries in 300 ms p95), AC-38 (no zeroed blocks after `kill -9`), AC-42 (case-insensitive unique names), AC-43 (per-blob keys, rewrap without rewriting), AC-69 (feed shows every change once, in order), AC-68 (no revision references a missing blob), AC-24 (scrub), AC-30 (7-day retention), AC-41 (restore and resync)
- Restrictions addressed: RESTRICT-07 (immutable blobs named by ID on XFS or ext4 as `blobs/<2hex>/<2hex>/<id>`, tree and metadata in PostgreSQL 18, SQLite excluded), RESTRICT-08 (envelope encryption, 64 KiB STREAM chunks), RESTRICT-09 (block counted only after batched `fdatasync`, `epoch.seq`), RESTRICT-10 (retention through a GC queue written in the same transaction), RESTRICT-13 (`jobs` table with SKIP LOCKED, no Redis)
- Risks addressed: R02 (snapshot restore rolls the epoch back), R07 (per-account serialisation), R14 (Postgres churn), R33 (backup ordering versus GC), R26 (no cross-node dedupe)
- Research source: `_docs/00_research/raw/storage_db.md` § Options comparison and § Recommendation; `_docs/01_solution/solution.md` § Component: Storage, metadata and encryption at rest

Dropbox-class sync needs stable file IDs and cheap moves; research found that a path-keyed file tree turns a folder move into N renames plus N database updates and creates a second source of truth (`04_reasoning_chain.md` Dimension 1). The choice of database also fixes who may write: the API and its janitors write concurrently, and the operator CLI runs as a second process.

## Decision

We will store all metadata, the change feed, the job queue and quota in PostgreSQL 18 accessed through sqlx 0.9 with plain SQL migrations, and store file content as immutable, ID-named encrypted files `blobs/<2hex>/<2hex>/<id>` on an XFS volume (ext4 supported), not content-addressed.

Each blob has a `blobs` row (`id` uuid v4, `account_id`, `state`, `wrapped_key`, `master_key_id`). The file is created at its final path in state `uploading`, preallocated, written by block at deterministic offsets, and made durable by a group-commit `fdatasync` before a block is acknowledged. The commit transaction (owned by 02 tree) flips the state to `committed`; there is no rename. A crash leaves at worst an orphan file, swept after 48 h. Replaced and deleted blobs get a `blob_gc` row in the same transaction as the change. Content hashes live in `revisions.content_hash` and block hashes in `upload_sessions.block_hashes`; they are not storage keys, so the same bytes uploaded by two users are stored twice (cross-user deduplication is out of scope).

Queues and feeds use no extra service: `jobs` is claimed with `FOR UPDATE SKIP LOCKED` inside the API (workers never touch the database), janitors claim rows the same way, and the feed reads `nodes.seq` per account. Users never see storage names; the filesystem sees only IDs, so case rules, Unicode normalisation and path length limits of the volume cannot affect user names.

## Alternatives Considered

| Alternative | Rejected because |
|-------------|------------------|
| SQLite (WAL) with Litestream | One writer, shared memory on one host and no network filesystem (`storage_db.md` F32); RESTRICT-07 excludes it. API requests, janitor loops and the separate CLI process all write concurrently. Fallback only for a one-container install, which is not a requirement. |
| Blobs inside PostgreSQL (`bytea` or large objects) | A `bytea` value is capped at 1 GB, below the 50 GiB of AC-4; large objects or chunk rows add a reassembly layer on every Range read over 64 KiB encrypted chunks and push every byte through WAL and `pg_dump`, which worsens R33 and R14. SQLite's size advantage applies only to blobs near 10 KB (`storage_db.md` F41). |
| S3-compatible object store (Garage, SeaweedFS, MinIO) | 3-5 services instead of 2 against AC-22; Garage's own documentation calls single-node replication "test only", MinIO community edition is in maintenance mode, and multipart is limited to 10,000 parts (`storage_db.md` F16-F18, option D). |
| Content-addressed chunked store (Seafile or restic style) | Needs refcounts, GC of shared chunks, reassembly per read and a chunk index; dedupe gain on already compressed photos and video was not shown (`storage_db.md` F29, "limited source"); cross-user dedupe is out of scope and conflicts with per-blob keys (AC-43). |
| Path-mirroring file tree on disk | Two sources of truth and an FS-to-DB reconciler (Nextcloud `oc_filecache` drift, oCIS posixfs experimental, `storage_db.md` F8, F11); a folder move is N renames instead of one row update. |

## Consequences

### Positive

- Move and rename are one row update; tricky user names never reach the filesystem; the database is the single source of truth for the tree.
- Constant-time commit for a 50 GiB upload (AC-4): content is already at its final path, commit is a transaction.
- One operational stack: two stateful units (Postgres, blob volume), no Redis, no object store (AC-22).
- Blobs are write-once, so file-level backups are safe at any moment as long as retention minus the GC interval exceeds the copy time (R33).

### Negative

- Operators cannot browse or restore by name; restore needs `sunnycloud admin restore` and the database.
- No filesystem data checksums on ext4/XFS; integrity relies on the scrub (AC-24) and operator backups.
- No deduplication: a lost desktop index or reinstall re-uploads unless `revisions:lookup` matches (R26).
- The database and the volume must be restored together and followed by `admin epoch-bump` (R02); a missing bump is detected only by the `.epoch` marker and the client `S > seq` check.
- Hot per-account rows (`account_feeds`, `quota`) serialise commits (R07); `block_hashes` rewrites and `jobs` churn need autovacuum tuning (R14).

### Neutral / Open

- The `BlobWriter`/`BlobReader` traits in 04 are the seam for another backend; none is planned in v1.0.
- ZFS or btrfs mirrors are allowed under the same layout; only the filesystem changes.
- Encryption and the nonce construction are decided in the blobstore component and reviewed under R08, not here.

## Evidence

- `_docs/00_problem/restrictions.md` § Technology
- `_docs/02_document/architecture.md` § 2. Technology Stack and § 4. Data Model Overview
- `_docs/02_document/data_model.md` § 6.1 `blobs`, § 6.3 `upload_sessions`, § 6.4 Blob lifecycle and sweeps, § 7.2 Rules for `epoch.seq`, § 9.1 `jobs`
- `_docs/02_document/components/04_blobstore/description.md` § 4. File Format and Encryption and § 5. Durability Policy (fdatasync)
- `_docs/02_document/deployment/deployment_procedures.md` § Backup and restore (AC-24, AC-30, AC-41, AC-43, AC-65)
- `_docs/02_document/canonical-names.md` § Technical defaults applied at Step 4 (reversible; listed in `risk_mitigations.md`) (cross-node dedupe)
- `_docs/00_research/raw/storage_db.md` § Options comparison and § Recommendation
- `_docs/00_research/04_reasoning_chain.md` § Dimension 1: Where files and the tree live and § Dimension 2: Metadata database
- `_docs/00_research/03_comparison_framework.md` § Initial Population (decisions, lead vs main alternatives)

## Notes

The `bytea` 1 GB cap is a PostgreSQL limit stated here from general knowledge; the research did not evaluate blobs-in-database. Re-evaluate content addressing only if users routinely edit multi-GiB files (the research's trigger for chunked CAS with delta) or if measured duplication across users is large.
