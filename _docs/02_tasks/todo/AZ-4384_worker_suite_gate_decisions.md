# Worker suite on H2: start-up, gate and decisions with the fake decoder

**Task**: AZ-4384_worker_suite_gate_decisions
**Name**: Worker suite, gate and decisions (H2)
**Description**: AZ-3878's AC-1 and AC-3 against the real API worker listener and PostgreSQL on the stack, with FD for the gate and scripted decoder answers
**Complexity**: 3 points
**Dependencies**: AZ-4382_fake_decoder_service, AZ-4383_h2_stack_profile_ctl
**Component**: 09_worker_supervisor
**Tracker**: AZ-4384
**Epic**: AZ-3639 (E-20)
**Ref**: split from AZ-3878 (E-20/c11)

## Acceptance Criteria

These are AZ-3878's AC-1 and AC-3, moved here unchanged. The expected results are in `_docs/02_document/components/09_worker_supervisor/tests.md`.

**AC-1: Start-up and reconnect (IT-09-1, IT-09-15, AC-25)**
Given:
- pre-filled scratch roots;
- FD with all self-check items ok, and then with one failed;
- the real decoder (RD), whose runner is killed while idle.

When the supervisor starts and the runner restarts
Then IT-09-1 and IT-09-15 meet their expected results:
- the scratch is swept before the first claim;
- no claim is made while the gate is failed;
- after a reconnect: `hello`, then `self_check`, then a claim.

**AC-3: Decisions, compliance and obsolete (IT-09-3, IT-09-5, IT-09-8)**
Given FD scripted probes and outputs, and a revision deleted before fetch
When the jobs run
Then the poster offsets and decisions, the `OutputInvalid` single attempt and the `obsolete` close meet their expected results

## Constraints

- The real API, queue and PostgreSQL in every case. FD only where the case scripts decoder faults or protocol behaviour.
- Each case keeps its "Max execution time" from 09 tests.md.

## Found in batch 129 (before this task started)

On the H2 stack the supervisor does not notice that an idle decoder connection ended: after `docker compose kill -s KILL`
or `stop` of `decoder-interactive`, and after its restart, `/readyz` stays 200 with `decoder_interactive: ok`, no
reconnect is logged, and the restarted runner never logs a new connection. Claims go on for that slot. IT-09-15 expects
no claim during the outage and a reconnect with `hello`, then `self_check`, then the first claim. The first job on
the dead connection would fail on the write or the read and only then trigger the reconnect.

So AC-1's IT-09-15 leg needs a product change in 09, not only the test: the slot has to watch its idle connection
(for example by reading it between jobs, so an EOF marks the slot as connecting) and then follow the existing reconnect
backoff (1 s to 30 s, `src/worker/src/compose.rs`). Until then `fdkit` restarts the supervisor on every FD swap
(`restart_worker=True`); the IT-09-15 case passes `restart_worker=False`. Re-estimate the points when this task starts.
