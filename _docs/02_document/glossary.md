# Glossary

**Status**: confirmed-by-user
**Date**: 2026-11-04

Project-specific terms. Source abbreviations: P = problem.md, R = restrictions.md, A = acceptance_criteria.md, S = solution.md.

- **AccountLifecycleHook** — the `contracts` port (`initialize(tx, account)`, `erase_batch(account, limit) -> EraseProgress`) through which each component creates and erases its own rows for an account; implemented by 02, 03, 05, 06, 07a, 08 and 20, driven by 07's `DeletionPurger`, triggered only by 01 (source: canonical-names.md).
- **Account** — one user identity on an installation, keyed by (provider, provider user id), never by email (source: R, Authentication).
- **Admin** — an account that can manage users, invites, quotas and health; the first admin is created with the setup token (source: R, A AC-23).
- **Allowlist** — optional operator configuration listing verified email addresses that may redeem an invite (source: R).
- **Base revision** — the file revision a client's change was based on; a mismatch with the current revision produces a conflict copy (source: R, A AC-8).
- **Batch commit** — one request that publishes many small files, idempotent by a client-supplied commit ID (source: R, A AC-40).
- **Blob** — an immutable, server-encrypted file content object stored under `blobs/<2hex>/<2hex>/<id>` (source: R, S).
- **Blob GC queue** — the table written in the same transaction as a delete or overwrite that schedules blob removal after the retention period (source: R, A AC-30).
- **Block** — a fixed 2 MiB slice of a file, the unit of upload, resume and verification (source: R, S).
- **Blocklist** — the ordered list of block hashes for a file, sent at commit so the server can name missing blocks (source: S).
- **Caddy** — the default reverse proxy in the Compose file; terminates TLS, buffering off (source: R).
- **Change feed** — the per-account ordered list of file changes that devices read through the long-poll endpoint (source: S, A AC-5).
- **Commit** — the step that atomically publishes a file revision after its blocks are stored (source: S).
- **Conflict copy** — a file named "name (conflict copy …)" holding content that a stale change would otherwise have overwritten (source: R, A AC-8).
- **`contracts` crate** — the shared crate of port traits through which components call each other, so no component depends on another's crate and the crate graph has no cycles (source: component-decisions.md).
- **Cursor (`epoch.seq`)** — a device's position in the change feed; an unknown epoch or a sequence below the purge horizon returns `resync_required` (source: R, A AC-41).
- **Decoder** — the sandboxed container process that runs libvips or ffmpeg on untrusted media, with `network_mode: none`, no credentials and no blob mount, behind a hardened runner that restarts after a hostile-class outcome; it probes and converts, and the supervisor decides the transcode policy (source: R, A AC-25).
- **Device** — a client installation (desktop daemon, phone, browser session) holding its own token (source: P).
- **Device grant** — the server's own RFC 8628 flow by which a desktop daemon gets a sync-only token after a browser approval (source: R, A AC-57).
- **Derived blob** — a thumbnail or transcode produced from an original; encrypted like every blob (source: R).
- **Epoch** — a value bumped when the database is restored, so devices detect a rewound feed; `serve` refuses to start while the database epoch is below the `.epoch` marker (source: R, canonical-names.md).
- **Echo suppression** — the desktop rule that a file the daemon just wrote is not uploaded again (source: A AC-18).
- **Generic OIDC** — any standards-based identity provider the operator configures; Google works through it (source: R).
- **Hostile name** — a file name a client must not trust (traversal, reserved, bidi, symlink tricks) (source: A AC-37).
- **Injectable clock** — the server time source; in the test image one offset is shared by the API and the worker supervisor. It has two endpoints: `POST /test/clock` on `:9091` sets it (harness to API) and `GET /internal/v1/test/clock-offset` on `:8081` reads it (supervisor). The release image returns 404 for both (source: A AC-30, canonical-names.md).
- **Invite token** — a single-use token that lets one person create an account (source: R, A AC-23).
- **Job** — a row in the Postgres `jobs` table describing a thumbnail or transcode task (source: R).
- **Lane** — one of up to four parallel block uploads per transfer (source: R).
- **Login code** — a one-time, short-lived code (stored hashed) that a mobile app receives after browser sign-in and exchanges, with its PKCE verifier, for a session (source: architecture.md section 9, answer 6).
- **Master key** — the operator-supplied key that wraps per-blob keys; never stored with the data; loss makes data unrecoverable (source: R, A AC-43).
- **Mass-delete guard** — the desktop hold that stops deletes when the sync root is missing or empty or 200 or more deletions arrive at once (source: A AC-29).
- **`media-shared` crate** — the shared crate of media types, job DTOs, the transcode policy and the decoder protocol, used by the media jobs component, the supervisor and the decoder; it has no sqlx and no queue SQL (the queue SQL lives in 08, inside the API) (source: canonical-names.md).
- **Migrator role (`sunnycloud_migrator`)** — the database role that owns the schema and runs `sunnycloud migrate` as a one-shot step before the API starts; it bypasses row-level security as owner. The API runs as `sunnycloud` (DML only, no `BYPASSRLS`) (source: canonical-names.md, S4-2).
- **Operator** — the person who installs and runs a sunnycloud server (source: P).
- **Pair (sync pair)** — one link between a server folder and a local folder, with its include and exclude rules (source: P, A AC-66).
- **Placeholder (evicted file)** — a cloud-only file stub from iCloud or OneDrive that the daemon must not hydrate or delete (source: A AC-32).
- **Reference worker (REF-CPU4)** — the 4-physical-core CPU transcode machine used for acceptance (source: A AC-11).
- **REF-SRV** — the reference server: 8 physical cores, 32 GB RAM, NVMe, 1 GbE (source: A AC-2).
- **Remux** — a lossless video rewrite that copies the streams and moves the index to the front, used instead of a re-encode when the codecs are already compliant (source: component-decisions.md, decision 2).
- **Resync** — a device rebuilding its view from the server after `resync_required` (source: A AC-41).
- **Retention period** — how long replaced or deleted blobs are kept (default 7 days, 0 to 90) for operator restore only (source: R, A AC-30).
- **Scrub** — the throttled background job that re-verifies stored blobs (source: A AC-24).
- **Server-info** — the public route `GET /api/v1/server-info` (type `ServerInfo`, defined in 11) through which clients read versions, minimum client versions, limits, providers and the setup flag; clients send the header `X-Sunnycloud-Client` (source: canonical-names.md).
- **Setup token** — a one-time secret printed on first start that allows creation of the first admin (source: R, A AC-23).
- **Share link** — a revocable URL for a file (folders are not shareable in v1.0) with an unguessable token, expiry and optional password (source: P, A AC-12).
- **Slint settings window** — the small native desktop settings UI, run as a separate process (source: R).
- **Supervisor** — the trusted worker process that feeds decoders; it has no database login and claims, heartbeats, reads input, writes output and fails jobs over the API worker listener `:8081` (source: canonical-names.md, S4-1).
- **Sync root** — the local folder of a pair on a desktop device (source: P).
- **Tombstone** — a record of a deletion kept 90 days so offline devices can apply it (source: R, A AC-41).
- **Transcode policy** — the rule, applied by the supervisor, that a video is transcoded only when it is not already compliant; a compliant clip with its index at the end is remuxed instead (source: R, A AC-11).
- **Vault** — an opt-in end-to-end encrypted folder tree, unlocked by a separate passphrase and recovery key, released in v1.2 (source: P, R).
- **Vault key** — the key that wraps per-file vault keys, itself wrapped by the passphrase-derived key and the recovery key (source: R).
- **Worker** — a Rust process running a supervisor and decoder for media jobs (source: P, R).
