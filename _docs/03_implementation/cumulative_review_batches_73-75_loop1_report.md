# Cumulative Review — batches 73 to 75 (loop 1)

**Date**: 2026-11-15
**Range**: `b73cc65..HEAD` (batch 73 `722597b`, batch 74 `4dca7c8`, batch 75 the commit that holds this report)
**Verdict**: PASS_WITH_WARNINGS

Each batch had its own review. This pass looks across the three: `serve` start and stop, the log subscriber for spawned tasks, and the first operator commands behind the CLI skeleton (`admin media-*`, `scrub`).

## Cross-batch checks

| Check | Result |
|---|---|
| Crate graph (ADR-015) | Holds (`crate_graph` green in every gate). Only the server crate names components together; it gained `uuid` as a normal dependency and `async-trait` as a dev dependency |
| One implementation per port | `dyn MediaAdmin` and `dyn Scrubber` are registered in the root over the instances 08 and 04 already hold (`composition_root.rs` asserts both); `serve` and the commands share them |
| Shared contracts changed | None; `Db::close` (helper 07), `PgUploadService::flush_all` (03) and `SyncBarrier::sync_open_groups` (04) are additive inherent methods, documented in helper 07 and in 12's description |
| Config table | New keys `public_addr`, `session_cookie`, `binding_cookie` (owner decision 2026-11-15); `cli_progress_interval` now read by the CLI |
| Migrations | None |
| Lockfile | One internal edge (`uuid` in the server crate); no new external package |
| Production code quality | No `unwrap()` added in production sources; two best-effort failures are logged, not dropped (`scrub run` progress write and the closing call after a failed step) |
| Process-global state | Exactly one: `serve` installs the log subscriber as the process default (AZ-4318); every other command keeps the per-future subscriber, asserted by `run::tests` |
| File length | No file over 500; close: `composition_root.rs` (486) |
| Test stability | No failure in the three full-suite runs (3742, 3744, 3757 passed). `range_memory` (1 GiB, debug build) takes about 10 minutes of a 25-minute run |

## Findings

| # | Finding | Next step | Severity |
|---|---|---|---|
| 1 | AZ-3797 (`key-check`, `keycheck-init`, `rewrap`) cannot be built correctly: `rewrap_batch` takes key ids, the command takes a file | Owner / E-10 answer | Medium |
| 2 | AZ-3793 AC-6 (failing, killed, irreversible migrations) needs `test-migrations` work in helper 07 and `test_migrations_dir` in helper 01, which no task covers | Tasks in E-06 and E-01 (carried from batch 46) | Medium |
| 3 | The SPA bundle is not wired to `HttpServer::with_spa`; `update_url` has no key | Later task / owner | Low |
| 4 | Server `test-clock` feature does not enable 11's (carried) | `-test` image task | Medium |
| 5 | No shared timestamp formatter for operator output | Small helper before `admin audit` / `invite list` | Low |

## Open findings carried forward

Findings 1 to 4 of the batches 70 to 72 review that are not listed above are unchanged (vault edge cases, `sync-core` vault codes, contracts part 1 at 2.0.0, OpenAPI cookie name).
