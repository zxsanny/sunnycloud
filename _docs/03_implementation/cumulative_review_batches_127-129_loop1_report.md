# Cumulative review, batches 127-129, loop 1

**Scope**: `git diff 4bef128..565e559` without `_docs` (f28f488, 99c6a40, 409daa1, 5312a41, 565e559; AZ-3847, AZ-4381,
AZ-4383, AZ-4382) | **Date**: 2026-11-26 | **Mode**: cumulative (all seven phases, cross-batch first)
**Verdict**: PASS_WITH_WARNINGS after one auto-fix round (no Critical or High; the Medium is fixed)

Evidence was gathered by a read-only agent and checked here against the code before each verdict below.

## Findings

| # | Severity | Category | Where | Finding | Outcome |
|---|----------|----------|-------|---------|---------|
| 1 | Medium | Bug | `src/components/11_http_api/src/test_hold.rs` (`Metered`) | A stream that took the hold and is then dropped (the supervisor killed mid-transfer, the handler refusing before it reads the body, a body shorter than the hold's bytes) never let the hold go: the status kept showing it, and every later arm answered `already_armed`. AZ-4381 AC-4 ("a forgotten hold lets go") failed in exactly the case decision D1 made the hold for. The time-limit reset also checked and reset under two separate locks. | Fixed: `impl Drop for Metered` lets the hold go; `let_go` checks and resets under one lock and is used by the time limit too. Proving tests `a_stream_dropped_while_it_is_held_lets_the_hold_go` and `a_stream_dropped_before_it_was_read_lets_the_hold_go` failed before, pass after; `dropping_a_finished_stream_leaves_a_new_arm_in_place` guards the fix. |
| 2 | Low | Bug | `test_hold.rs` `hold_request` | `{"route":"input","kind":"thumb"}` was accepted, but the input stream has no kind, so the hold could never be taken. | Fixed: a kind only with `output`; test `an_input_hold_with_a_kind_is_refused`. |
| 3 | Low | Security / Spec-Gap | `scripts/ci/check-release-symbols.sh` | The `WorkerHold` marker is not exercised by the script's real-binary test (it builds an `OffsetClock` sample only). | Accepted: `test_hold` compiles only with `test-clock`, which always brings `OffsetClock`, so the existing check already refuses a release binary that holds it. |
| 4 | Low | Maintainability | `src/components/11_http_api/Cargo.toml` | `futures-core` was an unconditional dependency, used only by the `test-clock` module. | Fixed: `optional`, enabled by `test-clock`; a dev-dependency for the crate's tests. |
| 5 | Low | Maintainability | `src/server/tests/pipeline_support/media.rs`, `pipeline_worker_hold.rs` | The output PUT, the claim and the input request head are written two or three times across the server's pipeline test helpers. | Accepted for now (test code, three files of other batches); noted for the next cleanup of those helpers. |
| 6 | Low | Maintainability | `tests/consumer/fdkit.py`, `selftests/stack/test_worker_harness.py` | The worker's kill-then-start was written three times; `Ctl._post` was called from outside stackkit. | Fixed: `Ctl.restart_worker()` and `Ctl.try_call()`. The allow-lists kept in stackkit and in ctl stay apart on purpose (different processes and images). |
| 7 | Low | Architecture (doc) | `_docs/02_document/module-layout.md` | 09 did not name the `fake-decoder` crate or FD's use of the decoder side of the protocol; 11 did not name `test_hold`. | Fixed: both entries. |
| 8 | Low | CI gap | `scripts/ci/rust-lint.sh` | `clippy --features test-clock` covered the server only, so the http api's own `test-clock` tests were never built or linted. | Fixed: step 5/9 `cargo clippy -p sunnycloud-http-api --all-targets --locked --features test-clock -- -D warnings` (clean locally). |
| 9 | Low | Maintainability | `src/worker/fake-decoder/src/serve.rs`, `decoder.rs` | FD repeats the frame length check instead of the shared `FrameReader`; `FakeDecoder::start` panics without saying so. | Fixed by documentation: the shared reader is the client's synchronous side, FD reads requests on an async stream; `start` documents its panic. |
| 10 | Low | Bug | `tests/run.sh` (FD lane folders) | `getfacl … \| grep -q` under `pipefail` could read as "no default ACL" if `getfacl` got SIGPIPE. | Fixed: `grep -q … < <(getfacl …)`. The `0777` lane folders stay (test-only, git-ignored, needed by two uids). |

## Checked and found fine

- Release images: the hold routes exist only on `:9091` under `test-clock`; route and kind are allow-listed and the body
  is capped; the worker Dockerfile's release stage stays last and `build-images.sh` builds without `--target`.
- e2e-ctl: every new action is allow-listed; no action returns a token.
- Cross-task: one decoder fake (the in-process one is gone), one transfer hold; the two worker readiness signals (the
  status row and `/readyz`) mean different things.
- No vacuous test found; all files at or under 500 lines; no `unwrap` in product code.

## Tests after the fixes

`cargo test -p sunnycloud-http-api`: 505 passed (default), 515 passed (`test-clock`); `cargo test -p sunnycloud-server
--features test-clock --test pipeline_worker_hold`: 3 passed; clippy clean for http-api (default and `test-clock`),
worker and fake-decoder; e2e-ctl host tests OK; consumer unit tests 38 passed.

Next cumulative review: after batch 132.
