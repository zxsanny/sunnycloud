# Cumulative Review — batches 86 to 88 (loop 1)

**Date**: 2026-11-18
**Range**: `dcffdee..78ed0ca` (batch 86 `931ac5c`, 87 `40406f4`, 88 `78ed0ca`)
**Verdict**: PASS_WITH_WARNINGS (after fixes)

Each batch had its own review. This pass looks across them: the vault routes and their wiring (AZ-3926), the admin performance suite (AZ-3824), the IT-11 runner and gates (AZ-3835), and six upload/download/feed/tree/media pipelines through the real binary (AZ-3655..3660), with their shared test harness.

## Cross-batch checks

| Check | Result |
|---|---|
| Production code | Vault handlers bind, make one port call, map (ADR-015); 428 before any port call; statuses match `vault_http_api.md` and `sc_error.md`; `create_vault` conflict check in the same transaction; `DbMetrics` registered once per context. No `unwrap`/`expect` in production code |
| Crate graph | New rules (11 takes no `sc_db`/`sqlx`/component; no component takes 11) with positive and negative tests; new dependencies follow workspace patterns (`base64`, `hyper` from the workspace; `hyper-util`, `http-body-util` as 11 declares them) |
| Public API | No production import of a non-public item of another component |
| CI | New jobs `http-api-suite` and `pipelines-large` pin action SHAs and the Postgres digest, use the watchdog and have timeouts; `it_suite.py` fails on missing/unknown cases, empty prefixes, 0 passed and over-budget cases |
| File length | No changed file over 500 lines (`pipeline_large_upload.rs` 493, `pipeline_tree_conflicts.rs` 492, `crate_graph.rs` 491) |
| Sleeps | No fixed sleep of a production duration; admin_perf's 60 s baseline only in the REF-SRV-only ignored test |

## Findings

| # | Sev | Category | Location | Finding | Outcome |
|---|---|---|---|---|---|
| 1 | High | CI / test gap | `.github/workflows/ci.yml` | No CI job builds or tests feature `vault`: the AZ-3926 binary tests (412/428, CSRF, cross-account 404, ST-20-2) and the vault branch of `dispatch.rs` ran only locally; in CI `vault_report` runs 0 tests | **Fixed** (owner approval 2026-11-18): vault clippy as step 3/5 of `rust-lint.sh`; `rust-test` job step "Server tests with the vault feature" (vault_http, vault_report, local_commands, api_services, composition_root) |
| 2 | Medium | CI | `ci.yml` `pipelines-large` | `--ignored --exact <name>` passes silently when a renamed test matches nothing | **Fixed**: `scripts/ci/run-one-test.sh` fails unless exactly one test passed (script test covers 1, 0, 2 passed, no result line, exit 101) |
| 3 | Medium | Test flakiness | `pipeline_large_upload.rs` AC-4 | Reaches the commit guard only within the 3 s login cache; a loaded runner can see 401 instead | **Fixed**: the test asserts the commit leaves within the 3 s window after the cache fill (clear message), and on a 401 retries the arrange once (account back to active, one authenticated call refills the cache, deleting again); the guard assertion is unchanged |
| 4 | Medium | Duplication | `vault_support/mod.rs` vs `serve_support/http.rs` | `send`/`parse`/`dechunk` copied to keep response headers | **Fixed**: `serve_support::http::Response` carries headers; the copy is gone |
| 5 | Medium | Duplication | `serve_support/account.rs` vs `pipeline_support/tree.rs` | The device-token insert twice | **Fixed**: one helper with a device name |
| 6 | Medium | Duplication | `pipeline_support/download.rs` vs `large.rs` | Two upload drivers over the same F4 routes | Deferred: merge into one start/lane-PUT/commit path with a block source when the next pipeline touches uploads |
| 7 | Low | Duplication | four `uuid_of`-style helpers | Same helper four times | **Fixed**: one `pipeline_support::uuid_of` |
| 8 | Low | Duplication | `wiring.rs` vs `feed.rs` source walkers; small helpers defined twice (`delete_*`, `names`, `wait_past`, long-poll spawn, `folder`) | Kept; merge with #6 |
| 9 | Low | Weak assertion | `pipeline_large_upload.rs` AC-1 | `!poll.is_finished()` cannot tell a parked poll from one not yet sent | **Fixed**: the parked-poll proof of `pipeline_change_feed` (one hold per principal, a same-principal probe answered 429) |
| 10 | Low | Duplicate logic | `11 server.rs` `vault_document` vs `registry::served_routes` | The vault route list is assembled twice; no parity test with the slot filled | Kept; noted for the next 11 change |
| 11 | Low | CI consistency | `scripts/ci/it_suite.py` | `cargo test` without `--locked` | **Fixed** |
| 12 | Low | Contract wording | `vault_http_api.md` POST row | "or the root already holds one" contradicts open point 3 and the code (another account's vault root is 404) | **Fixed** |

Lower-confidence notes: `pipeline_change_feed` AC-2 compares the API pool's gauge, which is meaningful only if long-polls hold that pool; change-feed AC-5 asserts answers within 3 s, equal to `shutdown_grace`, so the margin is tight on a slow runner.

## Carried forward

AZ-4332 (remediation, AZ-3660 AC-3), AZ-3835 AC-2/AC-3 and the empty server `test-clock` feature (Mode R leftover), unregistered `ratelimit_collector` and `audit_collector` (E-09 follow-up), AZ-3795 AC-4 (mock-idp).
