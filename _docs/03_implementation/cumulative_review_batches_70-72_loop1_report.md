# Cumulative Review — batches 70 to 72 (loop 1)

**Date**: 2026-11-15
**Range**: `b173fd0..b73cc65` (batch 70 `5dea90a`, batch 71 `de43c32`, batch 72 `b73cc65`)
**Verdict**: PASS_WITH_WARNINGS

Each batch had its own review; findings were fixed or recorded. This pass looks across the three batches. 117 files changed, about 3,250 lines added: the composition root (lazy builders, late lists, `ApiServices`, janitor scheduling), 07's `AdminService` implementation, 06's `PublicShareService` implementation with `authorize_derived`, and the per-commit account for 08's media effect.

## Cross-batch checks

| Check | Result |
|---|---|
| Crate graph (ADR-015) | Holds (`crate_graph` green in every gate). The server crate is the only place that names component crates together; the vault crate is linked only with feature `vault` (default build checked) |
| Shared contracts changed | Part 1 2.0.0 (`AdminError::Forbidden`; `AdminService::health` returns `Result`: breaking, owner to confirm); part 2 1.4.0 (`PublishedEntry.account_id`, additive) |
| One implementation per port | Every port of `ApiServices` is the instance the builders made for every other consumer (`api_services.rs`); 07 and 06 have one path each (inherent duplicates removed) |
| Config table | `share_password_max_concurrency` earlier; now `identity_janitor_interval` 5m, `audit_retention_interval` 1h (owner) |
| Migrations | None |
| Lockfile | One internal edge (`sc_ratelimit` in the server crate); no new external package |
| System-scope allowlist | No change |
| Production code quality | No `unwrap()` and no `#[allow]` added in production sources; no SQL `now()` (gate) |
| File length | No file over 500; close: `02 src/publish.rs` (489), `11 tests/it/identity/operations.rs` (470) |
| Test stability | 03 `reaper_race` failed once in batch 71 without a captured message (35/35 after; the gate now keeps full output). Batch 72's 4 failures were the rebuilt database's 100-connection default, not code |

## Findings

| # | Finding | Next step | Severity |
|---|---|---|---|
| 1 | `serve` is not yet runnable: start/stop order, listeners and the two shutdown steps | AZ-3791 (batch 73, owner decisions recorded) | Medium |
| 2 | Server `test-clock` feature does not enable 11's; `test_clock_addr` has no key; `--all-features` does not compile the server | `-test` image task | Medium |
| 3 | `update_url` has no key, so a `min_client_version` cannot be configured | `server_info.md` open point 2 (owner) | Low |
| 4 | 11's OpenAPI document hardcodes the session cookie name that is now a config key | 11 (E-21) | Low |
| 5 | Contracts part 1 at 2.0.0 although CC-3 called the change minor | Owner | Low |
| 6 | 07 `purge_support` and 03 `range_memory` tests pass empty commit effects | Test-only, keep | Low |

## Open findings carried forward

| # | Finding | Status |
|---|---|---|
| 1 | Vault edge cases (revived file inside a vault, restore into `Restored`), `vault_boundary` 409 without decision | Owner |
| 2 | `sync-core` maps the vault error codes to "unexpected" | 13 owner |
| 3 | 11 HTTP gaps from batch 66 (`ClientKeyMismatch` 500, `overloaded` without `Retry-After`, route error lists) | E-21 |
| 4 | OpenAPI parameters without a pattern; `MasterKey` without zeroize; Kotlin/Swift bindings print token fields | Open |
| 5 | IT-12-11 needs `sc_db` test migrations | Open (AZ-3793 partly) |
| 6 | Timing-flaky under load: 07a lock wait, sync-core `hash_guards`, server `config_binary`; 03 `reaper_race` once | Open, Low |
| 7 | `rust-lint.sh` steps 3-4 need `psql` here | Environment |
| 8 | Safari S12 run; REF-SRV timing rows; Android legs | Leftovers |
| 9 | Owner questions: AZ-3755 suffix ranges, AZ-3700 AC-6, AZ-3698 pending-cascade timing, AZ-3738 foreign base, AZ-3701 unknown token on the password route, AZ-4024 flags | Owner |
