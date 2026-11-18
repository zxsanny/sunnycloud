# Cumulative Review — batches 79 to 85 (loop 1)

**Date**: 2026-11-18
**Range**: `e6004b5..b700de6` (batches 79 `b027a33`, 80 `6cfcf82`, 81 `e47fc4f`, 82 `c2fb816`, 83 `4e917d5` + `2771103`, 84 `038f6cf`, 85 `b700de6`)
**Verdict**: PASS_WITH_WARNINGS (after fixes)

Overdue: the K=3 passes after batches 81 and 84 were not run, so this one covers all seven batches. Each batch had its own review. This pass looks across them: the key commands, `doctor --pre-flight`, the identity operator commands, restore dry run, operator account deletion, the doctor checks (database, config, missing blobs, quota drift, identity) and the migration fault tests.

## Cross-batch checks

| Check | Result |
|---|---|
| Crate graph (ADR-015) | Holds. Only new edge: `01_identity → sc_observability` (helper crate). No component-to-component edge |
| Public API | The server reaches components only through their `lib.rs` re-exports |
| Raw SQL | None in `src/server/src`; `DatabaseCheck` goes through `sc_db::Db::inspect`; new SQL stays in the owners' repo modules (01 `invite_repo`, 02 `revision_repo`) |
| Production panics | No `unwrap`/`expect`/`panic!`/`let _ =` in changed non-test code |
| Connections and transactions | `Db::inspect` closes its pool on every path; `preview_restore` runs inside a transaction that always rolls back; missing-blob walk paged, no N+1 |
| Exit codes | doctor Ok 0 / Warn 1 / Fail 2; key refusals 2, bad `--from-key-id` 3; rewrap cancelled 130; `restore --dry-run` `changed:false` |
| File length | `server/tests/composition_root.rs` had grown to 572 lines (fixed below); every other changed file ≤ 500 |

## Findings

| # | Sev | Category | Location | Finding | Outcome |
|---|---|---|---|---|---|
| 1 | Medium | Bug | `server/src/user_commands.rs` `deletion_failure` | Any deletion failure except `LastAdmin` became an opaque `internal` and the cause was never logged; an unknown account id surfaced as `internal` | **Fixed** (owner approval 2026-11-18): the cause is logged with `log_error!`; new `DeletionError::NotFound` (owner decision) from 01's new `AccountAdminError::NotFound`, so `admin user delete <unknown> --yes` answers `not_found` and writes nothing |
| 2 | Medium | Spec-Gap | `server/src/root/lists.rs` `providers_check` | An `oidc_issuer` that is no URL passed `sc_config`, failed in 01's provider build as a stack build error, and `doctor --pre-flight` aborted with no findings (AZ-3809 AC-1, AZ-3807 AC-3) | **Fixed** (owner approval 2026-11-18): `IdentityError::Config` maps to `CliError::Config`, so it is one `configuration` Fail finding |
| 3 | Medium | Maintainability | `server/tests/composition_root.rs` (572 lines) | Over the 500-line cap | **Fixed**: janitor, scheduler and readiness tests moved to `composition_scheduling.rs` (387 + new file) |
| 4 | Medium | Maintainability | `02_tree/src/revision_repo.rs` | `DESCRIBE_BLOB_REVISIONS` is a near copy of `DESCRIBE_BLOBS`; `TreeReader::describe_blobs` has no production caller left | Skipped: retiring a port method is a `contracts` change; noted for the AZ-3666 restore pipeline, whose spec still plans `describe_blobs` |
| 5 | Low | Style | `02_tree/src/revision_repo.rs` | `describe_blobs`' doc comment sat on `describe_blob_revisions` | **Fixed** |
| 6 | Low | Maintainability | `server/src/root/lists.rs` | `DoctorChecks` doc still said "04, then 08" | **Fixed** (lists the real order) |
| 7 | Low | Maintainability | seven doctor checks; five account-id parsers in the server | Private copies of the `Finding{level,message,remedy}` constructor and of account-id parsing | Skipped: a shared `Finding::ok/warn/fail` is a `contracts` addition; revisit in one pass when the next doctor check lands |

Lower-confidence notes, not counted: host clock read after `Db::inspect` (adds the migration check time to skew; harmless at 2 s); quota drift paging by offset may skip or repeat accounts that change during the run; `key_commands.rs` module comment says key bytes never leave 04 although `keycheck-init` reads `master_key_file` (allowed by the port contract; comment only); quota drift reports one aggregate Ok rather than one per account (owner reading of AZ-3809 AC-5).

## Tests

- Contracts `errors` 6; 01 `account_admin` 5, `account_finalize` 4, `account_profile` 2 (unknown `set_role`/`set_state` now assert `NotFound`); 07 `deletion_request` 5 (new: unknown account on the operator path is `NotFound`, no audit row), `error_codes` 5; 11 `it errors::` 21; server `composition_root` 12, `composition_scheduling` 5, `doctor_findings` 6 (new: issuer that is no URL is one configuration finding), `doctor_preflight` 1, `user_commands` 9 (new: `not_found`, and `internal` with the cause in the log only). All passed.

## Carried forward

AZ-3795 AC-4 (`dev-seed`, waits for mock-idp). Contract docs updated: `07_admin/account_services.md` 1.4.0, `01_identity/account_admin_and_guard.md` 1.4.0, `contracts` part 1 2.1.0.
