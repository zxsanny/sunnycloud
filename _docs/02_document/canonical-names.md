# Canonical Names (updated by plan Step 4 review)

Applies on top of `component-decisions.md`. Every doc uses these spellings. Where this file and `component-decisions.md` disagree, this file wins (it records the owner's Step 4 answers).

## Owner decisions at Step 4 (2026-11-04)
- **S4-1 Workers talk only to the API.** The supervisor claims, heartbeats, reads input, writes output and fails jobs over HTTP on the worker listener `:8081` (`/api/v1/worker-jobs/**`). There is no worker database login, no `sunnycloud_worker` role, no `db_worker_url`. Remote workers on other hosts are out of v1.0 (Compose single host; `:8081` is plaintext on the internal network). `media-shared` has no sqlx/queue module: job DTOs, policy and decoder protocol only; the queue SQL lives in 08, inside the API.
- **S4-2 Two database roles: `sunnycloud_migrator` and `sunnycloud`.** The migrator owns the schema and runs `sunnycloud migrate` as a one-shot Compose step before the API starts; it bypasses RLS as owner. `sunnycloud` is the API runtime role: DML only, no DDL, no `BYPASSRLS`, RLS policies enabled (FORCE not needed). `audit_log`: the API role has INSERT and SELECT only; retention purge is a `SECURITY DEFINER` function owned by the migrator. Config keys `db_api_url` and `db_migrator_url`. The API does not migrate itself at start; at start it checks the schema version and refuses to serve on mismatch. Backups run `pg_dump`/`pg_restore` as the Postgres superuser inside the db container.
- **S4-3 Updater:** the manifest is signed offline (or in a manual job), never by an automatic CI secret. The app pins two Ed25519 public keys (active and recovery). The manifest carries `version`, per-OS entries with SHA-256, a monotonic `serial` and an `expires` time. Exact layout is an ADR.
- **S4-4 Android bridge:** UniFFI only; no flutter_rust_bridge. Kotlin owns the Rust uploader handle and sends status to Flutter over a Method/EventChannel. iOS does the same with Swift.

## Technical defaults applied at Step 4 (reversible; listed in `risk_mitigations.md`)
- `serve` refuses to start when the database epoch is below the `.epoch` marker and prints `admin epoch-bump`. `doctor` reports the same as Fail.
- Restore re-points the existing revision row (and re-parents it into the original folder or `Restored`); it never creates a second revision for one blob (`revisions.blob_id` is UNIQUE). 02 requires `BlobRegistry` (reinstate) inside the restore transaction.
- `BlobOwnerCleanup::remove_owner_rows` deletes the revision row whatever its state; the FK `ON DELETE SET NULL` clears `nodes.current_revision_id`.
- New-file name collision at commit: the server assigns the conflict-copy name (one behaviour, no client retry).
- Non-current retained revisions are not downloadable by users (operator restore only); `/revisions/{id}/content` serves current revisions only for non-admin principals.
- Quota exceeded: HTTP 507, error code `quota_exceeded`.
- Zero-byte files: staged upload path with `block_count = 0` (`declared_size >= 0`).
- Reviving a tombstoned folder's ancestor clears `cascade_pending` on the revived ancestors; clients re-list a folder that turns live.
- `reset_admin` re-binds the existing admin account to a new identity through an operator invite with `target_account_id`.
- Vault files cannot be link-shared in v1.2.
- Cross-node content dedupe is out of scope (no commit-by-hash); the batched `POST /api/v1/revisions:lookup` exists only for reinstall rebuild and `server_has_hashes` is removed.
- Desktop remote deletes go to the OS trash/recycle bin. One feed loop and one cursor per account (not per pair).
- Android: minSdk 30; camera backup folder `Camera Uploads/<device label>/` with a collision suffix taken from the asset id; WASM split in 13 stays as confirmed (trim is a risk-register item).
- Decoder isolation: hardened runner (`PR_SET_DUMPABLE=0`, child subreaper, SIGKILL all descendants after every job, seccomp denying `ptrace`/`process_vm_*`) and the runner exits after any hostile-class outcome (Compose restarts it); no gVisor in v1.0. The Forgejo/Woodpecker fallback only builds and tests; releases are signed only from the GitHub release workflow.

## Operator commands (binary `sunnycloud`)
`serve`, `migrate`, `doctor`, `key-check`, `admin create`, `admin reset`, `admin promote`, `admin demote`, `admin user list`, `admin restore`, `admin epoch-bump`, `admin keycheck-init`, `admin rewrap`, `admin media-requeue`, `admin media-stats`, `scrub run [--full]`, `scrub status`, `admin setup-token`, `admin invite`, `admin gc-run`, `admin audit export`, `admin dev-seed`, `admin user set-quota`, `admin user delete`, `healthcheck`, `openapi`, `version`. No `key rewrap`, no `media requeue`, no top-level `restore`.

## Ports (all in server-only `contracts`)
- Periodic work: trait `Janitor` (`name()`, `run_cycle(now)`). Not `PeriodicTask`.
- Account erasure: `AccountLifecycleHook { initialize(tx, account), erase_batch(account, limit) -> EraseProgress{remaining} }`, implemented by 02 tree, 03 transfer (sessions, staged rows, receipts, reservations), 05 sync_feed, 06 sharing, 07a quota, 08 media_jobs, 20 vault_service. 07's `DeletionPurger` drives every hook until all report `remaining = 0`; 01 `AccountAdmin::finalize_deletion` then deletes `identities`, `sessions`, `device_tokens`, `device_grants`, `login_codes` and the account row. Replaces `UploadAdmin`, `ShareAdmin::revoke_all_for_account`, `MediaAdmin::cancel_jobs/purge_derived`, `TreeMaintenance::purge_account_batch`, `BlobAdmin::files_for_account`. Only 01 triggers hooks. Hook and cleanup lists are registered late (by 12), which breaks wiring cycles.
- `TreeReader` absorbs `NodeResolver`: `get_node`, `list_children`, `find_live_node`, `resolve_live_file(account, node) -> LiveFile{kind, revision, blob_ref}`, `root_node`, `get_revision`, `is_current_or_retained`, `describe_blobs(blob_ids) -> paths` (doctor, AC-73), `lookup_revisions(account, hashes<=1000)`. Folder versus gone are distinct results.
- `QuotaLedger::{reserve, release_reservation, adjust_used}` (07a). `QuotaAdmin::recompute(account, live_bytes, reserved_bytes)`: the caller (12 doctor, 07) takes the values from `LiveUsageSource` (02) and `ReservationSource` (03); 07a has no Requires.
- Commit: `AccountGuard::ensure_active(tx, account)` (01; `FOR SHARE` on the account row) is called first inside the commit transaction. `CommitActor{account_id, device_label}` is passed into `CommitService`. `CommitEffect` returns `Result<(), EffectError{Entry|Batch}>`; `on_batch` receives every entry outcome; entries carry `import_hint`. The advisory lock on the account is taken by `publish`; `create_folder`, `delete_node` and the erase hooks work in short batches and do not take it. There is no multi-level lock-order contract.
- Worker authentication belongs to 08 `WorkerAuthenticator` only (no `Worker` credential or scope in 01).
- Thumbnail state: `ThumbnailStateSource` (08; required by 02 and 11). Epoch: `EpochAdmin::bump_epoch` (05; also writes the `.epoch` marker through 04's `EpochMarker`). Restore: `TreeRestore::restore_revision` returns `placement: Original | Restored`. `DoctorCheck` is a `contracts` port.
- Client-side traits (`SyncClient` as trait `SyncApi` implemented by `HttpSyncClient`, `CredentialProvider`, `FileSource`, `DownloadSink`, `CheckpointStore`, `ProgressSink`) live in the client crate `sync-core`, not in `contracts`. `BackupCore` is defined in `mobile_backup`. `DaemonControl` types live in `ipc_schema`.

## Runtime
- Containers: `decoder-interactive` (tmpfs, numeric size cap) and `decoder-general` (disk scratch on a dedicated mount or quota) per supervisor, both `network_mode: none` with the decoder socket volume; the worker container has a `mem_limit`. Networks: `proxy` (caddy, api) and `backend` (api, db, worker); Caddy is not on `backend`. Test compose uses the same service names.
- Test clock (test image only): `POST /test/clock` on `:9091` sets the offset; `GET /internal/v1/test/clock-offset` on `:8081` reads it. The release image does not contain the routes; CI asserts a 404 on the pushed digest before signing.
- Config: environment variable = `SUNNYCLOUD_` + upper-cased key (helper 01 owns the single key-to-variable table; an unknown `SUNNYCLOUD_*` variable refuses start). Keys: `tombstone_horizon`, `device_code_ttl`, `share_max_ttl`, `share_grant_key`, `db_api_url`, `db_migrator_url`.
- Scheduler and rate limiter are constructed by the `sunnycloud` binary (12); 11 receives them. Janitors have no leader (`FOR UPDATE SKIP LOCKED` or leases). GC and the account-erase loop drain until nothing is due, within a time budget.
- Sync feed wake-up is the in-process notifier plus a safety recheck (10 s); CLI commands in another process rely on the recheck. No PgListener.
- `resync_required` is HTTP 410; the upgrade error is `upgrade_required`; info route `GET /api/v1/server-info`; header `X-Sunnycloud-Client`. `ServerInfo` is defined in 11.
- Mobile sign-in yields a `sessions` row (15-min access token plus rotating refresh token; the client single-flights refresh and persists the new refresh token before use). The desktop device grant yields a sync-only token.

## Open owner items (go to the Step 4 sufficiency question; do not resolve in files)
Vault share links beyond v1.2; Windows reserved-name marker (`%5F`) and ` (2)` suffix; poster-frame SSIM tolerance (decided 2026-11-11 by the owner: SSIM >= 0.95 against the harness render of an equivalent chain; for the record, measured SSIM of tone-mapped HLG poster frames is in `components/10_decoder/description.md` section 13.3: about 0.85 to 0.91 against the SDR master the generator started from, 0.995 or more against a harness render of an equivalent chain, so the owner must also say which reference counts; synthetic clip, AZ-3851); WASM trim; external review of the blob encryption before v1.0; Windows signing, Apple/Google accounts.
