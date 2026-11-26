# H2 stack profile and ctl actions for the worker suite

**Task**: AZ-4383_h2_stack_profile_ctl
**Name**: H2 stack profile and ctl actions
**Description**: The H2 settings as a profile of the Compose test stack, and the ctl actions the worker cases need
**Complexity**: 3 points
**Dependencies**: AZ-3661_pipeline_media_derived_outputs (the media stack suites), AZ-3874_worker_status_shutdown_main
**Component**: 09_worker_supervisor (test stack)
**Tracker**: AZ-4383
**Epic**: AZ-3639 (E-20)
**Ref**: split from AZ-3878 (E-20/c11, harness H2)

## Problem

The stack runs the worker with its own media settings (`tests/compose.test.yml`: lease 10 s, photo timeout 10 s; claim poll, status, silence, cancel grace and shutdown grace unset). H2 needs other values, and ctl cannot do several things the cases need (`tests/tools/e2e-ctl/ctl_actions.py`):
- list the processes in a decoder container (the "no ffmpeg left" check);
- kill a decoder's runner;
- swap the worker token;
- reach the worker's `/readyz` and `/metrics`.

## Outcome

- `tests/run.sh --profile h2` (or a documented override) runs the worker and the API with the H2 values of 09 tests.md: `job_lease=6s`, `worker_claim_poll_interval=100ms`, `worker_status_interval=1s`, `decoder_silence_timeout=1s`, `decoder_cancel_grace=500ms`, `photo_job_timeout=5s`, `worker_shutdown_grace=3s`. The `core` profile is unchanged.
- New ctl actions, each typed and checked against an allow-list like the existing ones:
  - `inspect` gets a process listing of a decoder container;
  - `process-kill` can kill the runner inside a decoder container;
  - a token swap restarts the worker with `worker_api_token_next` set, and then without the old token;
  - `internal-http` can reach the worker service's ops port.
- stackkit gets helpers for these actions, each with a unit test of its argument checks.

## Acceptance Criteria

**AC-1: The H2 profile carries the H2 values**
Given the stack started with the H2 profile
When the settings are read from the worker's and the API's start-up line
Then each value equals 09 tests.md H2

**AC-2: Each new ctl action works and refuses what is not allowed**
Given the stack
When each new action runs with valid and with refused arguments
Then valid calls do what they say, and refused ones exit 2 without side effects

## Constraints

- No change to the `core` profile's behaviour; the existing suites keep passing.
