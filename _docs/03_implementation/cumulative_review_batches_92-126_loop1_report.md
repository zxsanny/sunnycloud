# Cumulative Review — batches 92 to 126 (loop 1), mechanical pass

**Date**: 2026-11-26
**Range**: `05038aa..900178e` (799 files under `src/`, `scripts/`, `deploy/`, `tests/`, `.github/`; about 88,600 lines added)
**Verdict**: PASS_WITH_WARNINGS (after fixes)

The cumulative review is due every three batches (implement Step 14.5). The last one covered batches 89 to 91. None ran for batches 92 to 126, and no batch report said so (friction note 2026-11-26).

This pass is **mechanical only**. It covers the checks a script can run over the whole range. It is not the cross-batch semantic reading the earlier reports did, which follows each changed contract across batches. That reading is still owed for this range (see Carried forward).

## Checks run

| Check | Scope | Result |
|---|---|---|
| File length, 500-line soft cap | every added or changed non-exempt file in the range | 4 over the cap (findings 1 to 4). The three UniFFI Kotlin bindings (`src/mobile/android/.../uniffi/*.kt`, 3,469 to 5,319 lines) are generated, so they are exempt. `ci.yml` (539 lines) is CI configuration, not app code |
| Errors swallowed in production Rust | added lines matching `let _ =`, `.ok();`, `.unwrap()`, `unwrap_or_default()`, `.is_ok();` outside tests | 7 hits, none a finding. Two are documented best-effort sends: `thumb_notifier.rs:32` (no subscriber is not an error) and `decoder/src/self_check/network.rs:20` (the receiver gone after the deadline). The other five are `Option` defaults, each validated right after or harmless: `build_facts.rs:83` refuses an empty value, `decoder/src/main.rs` turns an empty `PATH` into a typed tool error, plus the `realtime/session.rs` media prefix, `discovery.rs` cursor and the `decoder/stub` binary |
| Errors swallowed in the web app | added `catch {}`, `.catch(() => …)` and lint disables in `src/web/src`, tests excluded | `renewal.ts:15` handles untrusted JSON with the documented outcome. `renewal.ts:41` keeps the original 401 when no answer arrives, also documented. The lint disables are `prefer-svelte-reactivity` on intentionally non-reactive maps. One finding (5) |
| `2>/dev/null` in scripts | added lines in `scripts/`, `tests/`, `deploy/` | none outside fallback chains |
| Crate layering and cycles | workspace | enforced in CI by `server/tests/crate_graph.rs` (AZ-3835 AC-4) and `worker/tests/crate_graph.rs`. Not repeated here |

## Findings

| # | Sev | Category | Location | Finding | Outcome |
|---|---|---|---|---|---|
| 1 | Medium | Maintainability | `src/server/tests/cli_suite.rs` | 741 lines | **Fixed**: `tests/cli_suite/` with `main.rs` and one module per acceptance check: `exit_rules`, `one_operation`, `coverage_map`, `secret_channel`, `lazy`, plus `common`. Binary name unchanged, 14 tests before and after. `it_cases.txt` and `it12_coverage_map.md` follow the new module paths |
| 2 | Medium | Maintainability | `src/server/tests/pipeline_realtime.rs` | 515 lines | **Fixed**: `WEBP` and `post_output` moved to `pipeline_support/media.rs` with the other worker-side helpers. 489 lines, 14 tests |
| 3 | Medium | Maintainability | `src/web/src/lib/realtime/client.test.ts` | 782 lines | **Fixed**: `client.test.ts` (first connection, frames), `client.reconnect.test.ts`, `client.liveness.test.ts`, `cursor.test.ts`, and the shared `client.test-support.ts`. 32 `it` and 4 `it.each` before and after |
| 4 | Medium | Maintainability | `src/web/src/lib/transfer/service.resume.test.ts` | 508 lines | **Fixed**: `service.resume.test.ts` and `service.stopped.test.ts`, plus `service.resume.test-support.ts`. 14 `it` and 1 `it.each` before and after |
| 5 | Low | Maintainability | `src/web/src/lib/transfer/queue.svelte.ts:238` | `.catch(() => undefined)` relied, without a word, on the handler five lines up | **Fixed**: a comment says the start failure is logged and handled there, and this chain only stops the same rejection from being reported twice |

## Verification

- `scripts/ci/check-it-suite.sh` with the `cli-suite` job's settings: 19 of 19 IT-12 cases inside their budgets.
- `cargo test -p sunnycloud-server --test pipeline_realtime`: 14 passed.
- `cargo clippy -p sunnycloud-server --all-targets --locked -- -D warnings`, and the same with `--features test-clock`: clean. `rustfmt --check` clean on every changed Rust file.
- `src/web`: `vitest run src/lib/realtime src/lib/transfer` 216 passed in 16 files. `bun run typecheck` 0 errors, 0 warnings. `bun run lint` clean.

## Carried forward

- The cross-batch semantic reading of batches 92 to 126 is owed. It should look at contract changes that span several batches: the realtime frames, the share-link `content_hash` (AZ-4350), the account-deletion purge, the media pipeline on the stack, and the CLI lazy config. Run it in smaller ranges, three batches at a time, starting from the newest.
- Earlier carry-overs still open: AZ-4332 enqueue adoption (the owner should confirm), the unregistered `ratelimit_collector` and `audit_collector` (an E-09 follow-up).
