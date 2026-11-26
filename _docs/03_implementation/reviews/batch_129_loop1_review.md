# Code Review Report

**Batch**: 129, AZ-4383_h2_stack_profile_ctl, AZ-4382_fake_decoder_service | **Date**: 2026-11-26 | **Verdict**: PASS_WITH_WARNINGS

Scope: `tests/compose.h2.yml`, `tests/compose.test.yml`, `tests/run.sh`, `tests/tools/e2e-ctl/*`, `tests/consumer/{stackkit,fdkit}.py`,
`tests/consumer/Dockerfile`, the new self-tests and suite, `deploy/docker/worker.Dockerfile`, `src/worker/fake-decoder/**`,
`src/worker/tests/**`, `src/worker/Cargo.toml`, `src/Cargo.toml`, `src/Cargo.lock`, `tests/README.md`.

## Findings (final diff)

| # | Severity | Category | File:Line | Title |
|---|----------|----------|-----------|-------|
| F1 | Low | Maintainability | src/worker/fake-decoder/src/serve.rs (`Job::work`) | about 85 lines, over the 50-line guideline |

## Fixed during implementation, before this review (not open)

| Severity it would have had | Category | Where | What |
|---|---|---|---|
| High | Architecture | `src/worker/fake-decoder/src/decoder.rs` | FD linked `decoder-runner` to reuse `socket::bind`; 10_decoder has no public API (`module-layout.md`) and 09 reaches it only over the socket (ADR-009). FD now has its own `src/socket.rs` with the same rules (stale socket replaced, other file refused, group then mode 0660) and two unit tests; the dependency is gone. |
| High | Bug | `tests/tools/e2e-ctl/ctl_actions.py` `inspect` | `docker compose ps -q` lists running containers only, so `inspect {"service"}` of a killed decoder answered 409; the first `core` run showed it (the self-test stopped before restarting the decoder, and later suites ran without it). The state report now uses `ps -a -q`; listeners still need a running container. Proving test: the stack self-test of the decoder kill. |
| Medium | Bug | `selftests/stack/test_worker_harness.py` | the kill and token tests restored the stack only on success; now in `finally`. |
| Low | Maintainability | the two stack test files | the readiness wait was written twice; now `Ctl.wait_worker_readyz`. |

### Finding Details

**F1: `Job::work` length** (Low / Maintainability)
- Description: one exhaustive match over the answer kinds; each arm is short, and a split would need new enums to keep
  the match exhaustive without `unreachable!`. Left as is.

## Phases

- Spec compliance: AZ-4383 AC-1 (timings per reading binary; observation changed from the start-up line, which neither
  binary has, to the container configuration of a healthy service: spec note and batch report row 1), AC-2 (ctl unit
  tests of every new action with valid and refused arguments, refusals without side effects; stack self-tests);
  constraint "core unchanged" (the `core` run asserts the core timings). AZ-4382 AC-1 and AC-2 (crate tests `replay.rs`,
  `record.rs`, the record after SIGKILL of the binary), AC-3 (`suites/worker/test_fake_decoder_stack.py`, h2), AC-4 (the
  worker tests use the crate; `slot_support/decoder.rs` and `decoder_support/hostile.rs` are deleted).
- Security quick-scan: no token is returned by any ctl action (unit-tested); `inspect settings` reads a fixed allow-list of
  timing variables only; the secret files are rewritten in place inside the run's secrets directory; FD runs with the
  decoders' isolation (no network, read-only, no capability, uid 10003). gitleaks on the new files: clean.
- Performance: nothing on a hot path (test tooling).
- Cross-task consistency: `stackkit` and `fdkit` follow the existing `Ctl.call` pattern; compose overrides follow the
  existing profile files.
- Architecture: the decoder-runner import (fixed before review, above). The FD image stage sits before the worker's release stage, which stays last (precedent:
  the decoder's `test-stubs` stage); `build-images.sh` builds without `--target`.
