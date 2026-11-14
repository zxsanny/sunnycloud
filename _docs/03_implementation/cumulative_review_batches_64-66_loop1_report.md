# Cumulative Review — batches 64 to 66 (loop 1)

**Date**: 2026-11-14
**Range**: `c3b3184..0a4a3c5` (batch 64 `2b983b4`, batch 65 `723ff73`, batch 66 `0a4a3c5`)
**Verdict**: PASS_WITH_WARNINGS

Each batch had its own review; findings were fixed or recorded. This pass looks across the three batches. 150 files changed, about 10,800 lines added: 02 real-implementation pass; 03 janitors, reaper, security suite, metrics and perf harness, erase hook, client protocol; 04 read-ahead and floor serialisation; 06 password grants and abuse limits; 07 last-admin guard and dev seed; shared config and workspace dependency tidy-ups.

## Cross-batch checks

| Check | Result |
|---|---|
| Crate graph (ADR-015) | Holds (`crate_graph` green in every gate). New normal edges are helpers only (06 on `sc_ratelimit`, `hmac`; 07 on `sc_ids`, `bytes`, `futures-util`); component edges are dev-only |
| Shared contracts changed | None in signatures (one doc comment on `AccountAdmin::lock_active_admins`); 06 constructor gained the limiter (crate-internal) |
| Config table | `share_password_max_concurrency` default 4 moved into `sc_config` (single source) |
| Migrations | None |
| Lockfile | No new external package; `hmac` now declared once in the workspace table |
| System-scope allowlist | Two reviewed lines: 03 `janitor_repo.rs` (reaper lists accounts with expired rows), 02 `tests/tree/real_account_repo.rs` (test account creation as 01 does it) |
| Production code quality | No `unwrap()` and no `#[allow]` added in production sources |
| Statement shape | `DELETE … IN (SELECT … LIMIT … FOR UPDATE)` could delete more than the limit; 03's session and receipt deletes now materialise the picked rows; only 20's vault delete still uses `IN (…LIMIT)`, safe (unique order, at most one row per account) |
| File length | No file over 500 lines; `03 tests/transfer/start_support.rs` is at 489 (new helpers must go into new support files) |
| Owner decisions applied | 04 read-ahead growth and floor lock; AZ-3699, AZ-3818, AZ-3822 (partly), AZ-3927 recorded |

## Findings

| # | Finding | Next step | Severity |
|---|---|---|---|
| 1 | 11 answers `ClientKeyMismatch` as 500 (no wire code) and sends 503 `overloaded` without `Retry-After`; two route error lists miss codes | E-21 (11) follow-up; contracts change if `Overloaded` should carry a delay | Medium |
| 2 | Composition root work is piling up: `PgTransferLifecycle`, `PgShareLifecycle`, 03 janitors, 08 janitors, `MediaHealth`, the seed's per-account media effect and `ProviderLogin` | AZ-3789 / E-22 | Medium (expected order, growing) |
| 3 | `set_state` on a `deleting` account is `Unexpected` (a disable racing a self-deletion gets a 500) | Owner E-15/E-17 | Low |
| 4 | Floor lock is per writer instance; the CLI builds its own writer | Documented; owner if CLI writes during serve | Low |
| 5 | PT-03 harness and PT-02 rows never executed | REF-SRV | Low |
| 6 | `sc_ratelimit` has no refund (per-link password limit approximated) | Shared helper owner | Low |

## Open findings carried forward

| # | Finding | Status |
|---|---|---|
| 1 | OpenAPI parameters without a pattern | Open |
| 2 | `contracts::MasterKey` does not zeroize on drop | Open |
| 3 | IT-12-11 needs `sc_db` test migrations | Open (AZ-3793 partly) |
| 4 | `max_vault_header_bytes` default outside `sc_config` | Open, Low |
| 5 | Timing-flaky under load: 07a lock wait, sync-core `hash_guards`, server `config_binary` | Open, Low |
| 6 | Generated Kotlin and Swift bindings print token fields | Open |
| 7 | Android JNI and UniFFI legs never run | Leftover |
| 8 | `rust-lint.sh` steps 3-4 need `psql` here | Environment |
| 9 | Test log capture copied (01, 20, sc_audit, 06 `share_abuse`) | Shared helper when next needed |
| 10 | Safari S12 run on the owner Mac | Leftover |
| 11 | Owner questions: AZ-3755 suffix ranges, AZ-3700 AC-6, AZ-3698 pending-cascade timing, AZ-3738 foreign base, AZ-3701 unknown token on the password route, AZ-4024 flags | Owner |
