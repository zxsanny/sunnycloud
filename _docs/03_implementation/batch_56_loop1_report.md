# Batch Report

**Batch**: 56
**Tasks**: AZ-3711_tree_quota_gc_deltas (done), AZ-3718_tree_thumb_state_listing (done), AZ-3692_identity_security_acceptance_suite (done, two defects found), AZ-4264_blob_registry_port_account (done); new follow-ups AZ-4267 (High, security) and AZ-4268
**Date**: 2026-11-13

Three parallel workers (`02_tree` for AZ-3711 and AZ-3718; `01_identity` tests; contracts + `04_blobstore` + 03 for AZ-4264). No manifest or lockfile change. The parent pre-approved two additive contract changes (`NodeView.thumb_state`; `BlobRegistry` methods take the account) and accepted a third (`BlobWriter::create_file` takes the plaintext size), wrote every document change, reviewed, ran the gate and committed.

## Owner decisions 2026-11-13

1. Device approval: a pending grant is bound at lookup to the account that looked it up (one new column) → AZ-4267.
2. Every token kind carries a short type prefix (`sc_ses_`, `sc_ref_`, `sc_dev_`, `sc_inv_`, `sc_set_`, `sc_lgc_`, `sc_dcd_`, `sc_shr_`) → AZ-4268.

## Task Results

| Task | Status | Result |
|------|--------|--------|
| AZ-3711_tree_quota_gc_deltas | Done | One pure delta table (`publish_usage::usage`) for every publish outcome, applied with `adjust_used`, the reservation release and `GcQueue::schedule_in_tx(Replaced, AfterRetention)` in the entry's savepoint; `delete_node` of a file: −current size once, blob queued `Deleted`; cascade: one `adjust_used(−sum)` per account per transaction, each marked file's blob queued `Deleted`. AC-1..AC-5, including 1,000 concurrent publishes, deletes and cascade batches under a 64 MiB quota: no sample above the quota, final used = `live_bytes` = recompute |
| AZ-3718_tree_thumb_state_listing | Done | `NodeView.thumb_state` (contracts); `PgTreeReader::with_thumbnails(source)` fills it on `list_children` with one `states` call per page after the page's transaction, image and video hashes only; `None` elsewhere and without a source. AC-1..AC-3 |
| AZ-3692_identity_security_acceptance_suite | Done, 2 defects | Cross-account (sessions, devices, approved grants, every row unchanged by `xmin`), RLS (`ALTER TABLE` and `SET ROLE` as the API role refused), foreign credentials (worker token, refresh token, login code, device code give no principal), no token stored or logged (700 tokens of 7 kinds scanned in every table and log line), first admin through the ports. Defects found, tests marked `#[ignore = "DEFECT: …"]`, not weakened: (1) another account can decide a pending device grant (High) → AZ-4267; (2) identity tokens carry no type prefix → AZ-4268 |
| AZ-4264_blob_registry_port_account | Done | Every `BlobRegistry` port method takes the account and every 04 statement filters by it (a blob named with another account behaves like an absent row); 04 implements the port as a trait; 03 uses it through the port (adapter removed); the GC sweeper passes the locked row's account. The writer keeps no plans at all: `create_file(plan, plaintext_size, preallocate)`, so a refused or cancelled start cannot leak (100 refused starts leave nothing) |

## Contract and document changes (parent)

`blob-registry-gc.md` 2.0.0, `blob-writer.md` 2.0.0 (breaking: account on the registry, size on `create_file`); `tree_commit.md` 1.2.0 (usage deltas, delete, cascade; constructors); `tree_reader.md` 1.3.0 (`thumb_state`); `data_model.md` §5.1 (cascade quota and GC); AZ-4250 gains AC-5 (the real GC queue in 05's via-tree tests).

## Code Review Verdict: PASS_WITH_WARNINGS

CI-parity (`CARGO_INCREMENTAL=0`): fmt, workspace clippy `-D warnings`, workspace tests on PostgreSQL 18 (3265 passed, 0 failed, 23 ignored: the 21 hardware and not-run tests plus the two defect tests), wasm build and the four check scripts all exit 0.

Warnings (recorded):
- **Security (High, AZ-4267)**: device approval can be decided by another account holding the grant id.
- `PgTreeService::new` now takes `Arc<dyn GcQueue>` and `PgTreeMaintenance::new` takes `Arc<dyn QuotaLedger>`, against the brief's "keep constructors": accepted (required arguments keep a production setup from silently skipping GC and quota); the 05 and 08 test call sites were updated by the worker.
- 05's via-tree tests use a recording `GcQueue` double (no shared blobstore test config) → AZ-4250 AC-5.
- The cascade makes one `schedule_in_tx` per marked file (no batch call); PT-02-4 is hardware-only, so the cost is unmeasured.
- `thumb_state` is not on 11's listing JSON yet (`api_v1_surface.md` open item 30).
- `CommitEffect::on_entry` receives no account, so 03's commit effect must carry it (note for AZ-3738).
- `oauth_states` keeps `nonce` and `pkce_verifier` in plain text, as the schema intends (short-lived); worth a look by the at-rest rule's owner.
- One worker ran `cargo fmt --all` once mid-batch (formatting only, other workers' files).

## Discovered during implementation

| # | Item | Spec ref | Handling | clear / unclear |
|---|------|----------|----------|-----------------|
| 1 | A live file under a tombstoned ancestor not yet marked still counts in usage; its revive applies +new − old and queues the old blob `replaced` | AZ-3711 | As implemented, consistent with `live_bytes` | clear |
| 2 | Test fixtures that seed raw rows must add their bytes to `quota.used_bytes`, or delete and cascade hit the ledger's invariant check | AZ-3711 | Test support updated | clear |
| 3 | `revoke` of an id the account does not own answers `Ok` and writes nothing | AZ-3692 | Allowed by AC-1 | clear |
| 4 | `GcQueue::schedule_in_tx` still has no account | AZ-4264 | Out of scope | unclear |

## Commit

`[AZ-3711] [AZ-3718] [AZ-3692] [AZ-4264] Usage deltas, thumb state, identity suite, registry` — Loop: 1

## Next Batch

AZ-4267 (device grant binding, High) and AZ-4268 (token prefixes) first.
