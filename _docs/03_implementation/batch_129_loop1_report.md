# Batch 129, loop 1: AZ-4383 H2 stack profile and ctl actions; AZ-4382 fake decoder service

**Batch**: 129
**Tasks**: AZ-4383_h2_stack_profile_ctl, AZ-4382_fake_decoder_service
**Date**: 2026-11-26

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-4383_h2_stack_profile_ctl | Done | 15 files | ctl host 52/52, consumer unit 38/38, stack: core 113 passed / 7 skipped, h2 53 / 1 | 1 (AC-1 observation, below) |
| AZ-4382_fake_decoder_service | Done | 40 files | fake-decoder 44/44, worker 211/211, stack: h2 FD case passed | None |

AZ-4382 was built in two parts: the Rust crate by one worker agent (owned `src/worker/**`, `src/Cargo.toml`, `src/Cargo.lock`),
the image target, Compose services, ctl swap and stack case by the parent. The write sets did not overlap.

## What was built

AZ-4383:
- `tests/compose.h2.yml`, merged over `tests/compose.test.yml` by `tests/run.sh --profile h2`: the H2 timings of 09 tests.md
  on the binary that reads each key (api: job lease 6 s, status 1 s; worker: claim poll 100 ms, status 1 s, silence 1 s,
  cancel grace 500 ms, photo timeout 5 s, shutdown grace 3 s). The profile is kept in `e2e-results/.stack-state`, so
  `reset`, `rerun`, `down` and the ctl stub (`--compose-file` repeatable) use the same files. `core` is unchanged.
- ctl actions (`tests/tools/e2e-ctl/`; `ctl_worker.py` holds the worker-suite ones, `ctl_probes.py` the db-probe table and
  `ctl_errors.py` the error type, so `ctl_actions.py` stays under 500 lines):
  - `internal-http` reaches the worker's ops port `:9090` (`service: "worker"`); the worker bearer stays on the api's listeners;
  - `inspect` lists a decoder's processes with `docker top` on the host (nothing runs inside the decoder, whose runner
    watches its own process table), and reads the allow-listed timing settings of the api's or the worker's container;
  - `process-kill` kills and starts a decoder (its runner is the container's main process), and restarts the api, waiting
    for it to be healthy, optionally rotating the worker token first: `next` (a fresh token in `worker_api_token_next`,
    the api accepts both) or `drop-old` (the next or a fresh token becomes the only one, so a worker still holding the old
    one gets 403). The secret files are rewritten in place (each is bind-mounted into the api), and no token is returned.
    `worker_api_token_next` is a new Compose secret of the api: an empty file, which the config reads as absent.
- stackkit helpers with argument checks: `worker_ops`, `worker_metrics`, `timing_settings`, `decoder_processes`,
  `kill_decoder`, `start_decoder`, `rotate_worker_token`; `Ctl.internal` takes a `service`.
- `selftests/stack/test_worker_harness.py`: under either profile, the profile's timings on the api and the worker, the
  worker ops port, an idle decoder's processes, a decoder kill and restart, and a token rotation (the worker keeps working
  with both tokens, goes unready once the old one is dropped, and is ready again after its restart).

AZ-4382:
- `src/worker/fake-decoder/` (package `sunnycloud-fake-decoder`, lib + bin `fake-decoder`): the in-process `FakeDecoder` and
  the hostile peers moved out of the worker tests into one crate (AC-4: the worker tests use it; the old files are
  deleted). Script (JSON: hello, self_check, per request kind a queue and a default, idle outage; answers probe, image,
  transcode, error, hang, silent, alive_only, garbage, over_time, outage; outputs file, bytes, absent, symlink, hard_link,
  fifo, sparse, directory) and record (JSON lines, one unbuffered write per received message before the answer) are
  documented in the crate's module doc. The socket is bound by the real decoder's `decoder_runner::socket::bind`.
- Image target `fake-decoder` in `deploy/docker/worker.Dockerfile` (before the release stage, which stays last; the
  precedent is the decoder's `test-stubs` stage).
- Services `fd-interactive` and `fd-general` in `tests/compose.h2.yml` (compose profile `fd`, the real decoders'
  isolation, uid 10003); script and record in `e2e-results/fd/<lane>/`.
- ctl `process-kill` ops `fake` (stop the lane's decoder, start a new FD container on its socket and scratch) and `real`
  (stop FD, start the decoder, wait for healthy); 409 `no_fake_decoder` on a stack without FD.
- `tests/consumer/fdkit.py` (`FakeDecoderLane`: start with a script, read and wait on the record, stop) and
  `suites/worker/test_fake_decoder_stack.py` (AC-3, skipped under `core`).

## Code Review Verdict: PASS_WITH_WARNINGS

`_docs/03_implementation/reviews/batch_129_loop1_review.md`: one Low (Maintainability) open; an Architecture and a Bug
found and fixed before the review (FD's import of the decoder's socket setup; `inspect` of a stopped container).

## Test Suite

- `cargo test -p sunnycloud-fake-decoder`: 44 passed; `cargo test -p sunnycloud-worker`: 211 passed
- e2e-ctl host unit tests: 52 passed; consumer unit tests (`selftests/unit`): 38 passed
- `tests/run.sh --profile core` (full): 113 passed, 0 failed, 7 skipped (the NOT-RUN sentinel, the three AZ-3661 legs,
  PT-12-1/2, and the FD case, which runs under h2 only); after the last edits to two self-tests, `core` with
  `selftests suites/worker`: 52 passed, 2 skipped
- `tests/run.sh --profile h2` with `selftests suites/worker`: 53 passed, 0 failed, 1 skipped (the sentinel)
- CI-parity: PASS: `cargo fmt --all --check`, `scripts/ci/rust-lint.sh` steps 1-6 (clippy workspace, server `vault` and
  `test-clock`, decoder `test-freeze`, mobile `uniffi`), `scripts/ci/tests/run-script-tests.sh`, `scripts/ci/audit.sh`,
  `scripts/ci/check-pinning.sh`, gitleaks on the new files. Not run locally: rust-lint steps 7-8 (the sqlx offline data
  check needs `psql` on the host; this batch changes no query), the six workspace rust-test shards (the two changed
  crates ran in full).

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | AC-1 reads the settings "from the start-up line", but neither binary logs its settings and the log schema is closed (`sc_observability` field set). | AZ-4383 AC-1 | AC-1 now reads each service's container configuration (`inspect settings`) and requires a healthy start: a binary refuses an unknown `SUNNYCLOUD_*` name or a value it cannot bind, and ignores a table key it does not read, so the check is per reading binary. Spec updated with a note. | clear (the AC's title, "the profile carries the H2 values", is what is proven; no product guarantee changes) |
| 2 | The spec's token swap says "restarts the worker with `worker_api_token_next` set"; only the api reads `worker_api_token_next` (config table, readers A). | AZ-4383 Outcome | The api is restarted; the worker keeps the token it read at its start, which is what IT-09-10 case B needs. | clear |
| 3 | IT-09-12(b), the hard link: with `fs.protected_hardlinks=1`, FD (uid 10003) may only link an `in/` file it owns or can read and write. | AZ-4385 AC-4 | AZ-4385 checks the mode the supervisor gives `in/` files; if FD cannot link, the case shows `fd_error` in the record and must fail, not pass. | clear |
| 4 | FD's default output bytes are refused by the API's output sniffer, so no FD output can become a derived file; IT-09-12(e), the control case, needs postable bytes. | AZ-4385 AC-4 | The `bytes` output kind carries the exact bytes a case needs. | clear |
| 5 | An FD answer later than the 1 s silence timeout needs `alive` frames meanwhile. | IT-09-10, IT-09-17 | `over_time` takes `alive_every_ms`; cases with a long answer set it. | clear |
| 6 | The supervisor does not notice that an idle decoder connection ended (SIGKILL or stop of the decoder, also after the decoder's restart): `/readyz` stays 200, no reconnect, claims go on; the first job on the dead connection would fail before a reconnect. Measured on the kept H2 stack. | IT-09-15 (AZ-4384 AC-1) | A product change in 09 (the slot watches its idle connection) as part of AZ-4384, noted in its spec. Meanwhile `fdkit` restarts the supervisor on each FD swap, and the decoder-kill self-test restarts it in its cleanup so later suites do not inherit a dead connection. | clear (IT-09-15's expected result states the behaviour) |
| 7 | The checkout carries a default ACL `other::---`: files the consumer writes in a bind-mounted folder were unreadable for FD (uid 10003), and FD's record would be unreadable for the consumer. | AZ-4382 AC-3 | `run.sh` strips the default ACL from `e2e-results/fd/<lane>`; gotcha `_docs/gotchas/checkout-default-acl-hides-files-from-other-uids.md`. | clear |
| 8 | AC-3 says "when the supervisor starts" with FD on the socket; the first version of the case swapped FD in under a running supervisor (which is IT-09-15's reconnect). | AZ-4382 AC-3 | The case restarts the supervisor after the swap (fdkit default). | clear |

## Commit

`[AZ-4383] [AZ-4382] H2 stack profile, ctl worker actions, fake decoder`

## Next Batch: AZ-4384, AZ-4385 (both depend on these two); then AZ-3878
