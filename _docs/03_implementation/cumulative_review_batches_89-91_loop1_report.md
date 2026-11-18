# Cumulative Review — batches 89 to 91 (loop 1)

**Date**: 2026-11-18
**Range**: `78ed0ca..eaf8886` (batch 89 `5ad0bd3`, 90 `f1ed0c5`, 91 `eaf8886`)
**Verdict**: PASS_WITH_WARNINGS (after fixes)

Each batch had its own review. This pass looks across them: the media-job remediation (AZ-4332), the `test-clock` feature wiring (AZ-4334), the restore pipeline and the doctor epoch check (AZ-3666), the GC/retention pipeline (AZ-3663), and the shared server test harness and CI scripts the three batches changed.

## Cross-batch checks

| Check | Result |
|---|---|
| 08 NULL-revision semantics | `revision_id` is read only in `job_repo.rs` (`HELD`, now optional); `check_revision` maps NULL to `RevisionObsolete` for input and output; claim, reap, purge and requeue work on status and account only |
| Enqueue adoption | Account-scoped (`jobs_key` includes `account_id`, the foreign key is `(revision_id, account_id)`, the tenant policy applies) and applies only `WHERE jobs.revision_id IS NULL`; concurrent adopters serialise on the row lock; the purge CTE uses `FOR UPDATE SKIP LOCKED`; a deleting account refuses commits and erase deletes by account, so no erase race |
| Down migration | `DELETE ... WHERE revision_id IS NULL` is exactly what the old cascade removed: no extra loss |
| test-clock isolation | The feature turns on only `sunnycloud-http-api`, `sc_clock` and `sc_config` test-clock; every `#[cfg]` row and wiring hangs on it; in a feature build the server has one `OffsetClock` and no `SystemClock::new()` remains in server production wiring (log timestamps use the context clock); the release-symbol check and the Dockerfile mismatch guard stay |
| Doctor epoch check | Error handling is log plus a typed Fail, no swallowed errors; tag and placement match the sibling checks; no new crate dependency; no business logic in 12 |
| Test `Ports` | The no-reuse list is not held across an await; refused listeners stay open for the whole call; `all()` includes the fourth port only in a feature build; no leak |
| CI scripts | `run-one-test.sh` and `rust-lint.sh` correct; `install-tools.sh pg-client-18` fetches the key once and compares its fingerprint (whole-output equality, so extra keys fail closed) before dearmoring, with `signed-by` and a release-suffix guard |

## Findings

| # | Sev | Category | Location | Finding | Outcome |
|---|---|---|---|---|---|
| 1 | Medium | Maintainability | `server/tests/pipeline_tree_conflicts.rs` | 501 lines, over the 500 cap | **Fixed**: the quota-settled assertions moved to `Pipeline::assert_quota_settled` in `pipeline_support/tree.rs` (497 lines) |
| 2 | Low | Production | `server/src/epoch_check.rs` | The inventory was read on every call, though `epoch_findings` uses it only for an absent marker: an inventory error turned a matching epoch into a Fail; `!page.missing.is_empty()` was redundant | **Fixed**: read only when the marker is absent; `holds_blobs` is `page.next.is_some()` |
| 3 | Low | Semantics | `epoch_check.rs` | "Holds blobs" comes from committed rows, not from files on the volume | **Fixed**: documented in the module doc (restored database on an empty volume is the missing-blob report's finding) |
| 4 | Low | Test | `migrate_interrupted.rs` | The fingerprint covered columns and constraints only; the lock blocks only migrations that need more than ACCESS SHARE | **Fixed**: indexes, row-level policies and triggers added; the lock assumption documented (a migration that never waits fails at the "never started waiting" deadline, not silently) |
| 5 | Low | Duplication | `pipeline_support/{restore,wiring,feed}.rs` (source readers), `test_clock_serve.rs` vs `gc.rs` (janitor-cycle scrape) | Three copies of "read the source up to `#[cfg(test)]`", two of the cycle-counter scrape | **Fixed** in batch 92: one `production_text` in `wiring.rs`, one `janitor_cycles` in `serve_support/http.rs` |
| 6 | Low | Test | `test_clock_serve.rs` AC-3 | The held-port bind result was never checked, so the premise could be unproven | **Fixed**: only `AddrInUse` is accepted as "already taken"; any other failure fails the test |
| 7 | Low | Migration | `20261118100000_keep_jobs_on_revision_collect.up.sql` | `ADD CONSTRAINT ... FOREIGN KEY` validates all of `jobs` under SHARE ROW EXCLUSIVE; minor while `jobs` is small | Kept: a committed, unreleased migration is not rewritten for a small table; use `NOT VALID` + `VALIDATE` if `jobs` grows |
| 8 | Low | Test gap | 08 `obsolete.rs` | No proof that adoption cannot cross accounts | **Fixed**: `an_obsolete_job_of_one_account_is_never_adopted_by_another_accounts_enqueue` |

Lower-confidence notes and decisions:
- The PG client pin `18.7-1.pgdg24.04+2` was checked against PGDG's package index: it lists 18.7, 18.6 and 18.4 today, so the pin works and is not at risk from the next minor release. PGDG moves older versions to an archive eventually; the bump step is now written in `install-tools.sh` and the CI README.
- CI job timeouts (`rust-test`, `rust-lint` 25 min) have no timing data after the added steps; the first CI run shows whether they hold.
- On a real runner image `apt-get install libpq5=18.7...` may replace a preinstalled `libpq-dev`; the batch 90 installer trial used a bare `ubuntu:24.04` container. The first CI run shows it.
- `run_tool` in `restore.rs` writes the whole dump to `pg_restore` stdin before draining its output; fine at current sizes with `--exit-on-error`.
- A job is keyed by content hash but tied to one revision: when two live revisions share content and the job's revision is collected, the other revision is not reprocessed until another enqueue adopts the job (the old cascade had the same gap).

## Carried forward

AZ-4332 enqueue adoption (owner to confirm), AZ-3835 AC-2/AC-3 and AZ-3664, AZ-3654, AZ-3971 (E-31 test stack), AZ-3795 AC-4 (mock-idp), unregistered `ratelimit_collector` and `audit_collector` (E-09 follow-up), `pipeline_support` upload-driver merge (cumulative 86-88 item 6).
