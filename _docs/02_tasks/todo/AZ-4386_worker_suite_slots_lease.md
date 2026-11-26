# Worker suite on H2: slots, lanes and the lease

**Task**: AZ-4386_worker_suite_slots_lease
**Name**: Worker suite, slots, lanes and the lease (H2)
**Description**: IT-09-9 and IT-09-10 against the real API worker listener and PostgreSQL on the stack
**Complexity**: 3 points
**Dependencies**: AZ-4382_fake_decoder_service, AZ-4383_h2_stack_profile_ctl, AZ-4384_worker_suite_gate_decisions
**Component**: 09_worker_supervisor
**Tracker**: AZ-4386
**Epic**: AZ-3639 (E-20)
**Ref**: split from AZ-4385 in batch 131 (its former AC-5 and the IT-09-10 part of its AC-4)

## Acceptance Criteria

The expected results are in `_docs/02_document/components/09_worker_supervisor/tests.md`.

**AC-1: Slots and lanes (IT-09-9)**
Given 20 queued poster or transcode jobs and 20 stills jobs, and FD on both sockets answering after 300 ms
When the supervisor runs
Then the interactive slot claims only `lane=image`, the general slot alternates `video, image` while both lanes have work, both lanes progress, each slot holds at most one job at a time, and a priority-0 stills job queued behind the bulk goes to the next free image-capable slot

**AC-2: Lease heartbeat, 409 and 403 (IT-09-10)**
Given an FD job lasting 8 s
When the lease expires mid-job (test clock advance), and in a second run when the API's worker token is replaced mid-run (`Ctl.rotate_worker_token("drop-old")`)
Then the heartbeat cadence, the cancel after the 409 with nothing posted and scratch removed, and the readiness and claim stop after the 403 meet their expected results; no log line holds the token

## Notes

- FD's record names each job by id; mapping an id to its lane or kind needs a fixed read-only probe in ctl (no free-form SQL).
- The heartbeat cadence and "0 claims after the 403" need the API's view of the worker calls: the api log (`inspect logs`) or a fixed probe of lease timestamps.

## Constraints

- The real API, queue and PostgreSQL in every case. FD only for fault and protocol behaviour.
