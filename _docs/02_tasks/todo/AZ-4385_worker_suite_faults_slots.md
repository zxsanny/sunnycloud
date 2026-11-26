# Worker suite on H2: faults and slots with the fake decoder

**Task**: AZ-4385_worker_suite_faults_slots
**Name**: Worker suite, faults and slots (H2)
**Description**: AZ-3878's AC-4 and AC-5 against the real API worker listener and PostgreSQL on the stack, with FD fault scripts and two FD sockets
**Complexity**: 5 points
**Dependencies**: AZ-4382_fake_decoder_service, AZ-4383_h2_stack_profile_ctl
**Component**: 09_worker_supervisor
**Tracker**: AZ-4385
**Epic**: AZ-3639 (E-20)
**Ref**: split from AZ-3878 (E-20/c11)

## Acceptance Criteria

These are AZ-3878's AC-4 and AC-5, moved here unchanged. The expected results are in `_docs/02_document/components/09_worker_supervisor/tests.md`.

**AC-4: Faults (IT-09-6, IT-09-7, IT-09-10, IT-09-11, IT-09-12, IT-09-13)**
Given:
- the FD fault scripts;
- the edited job hash;
- the limits;
- the expired lease;
- the replaced token (ctl token swap);
- the silent and the over-time decoders;
- the hostile output files and frames of 09 tests.md.

When each case runs
Then each meets its expected result, including no secret byte in any posted body and the supervisor process staying up. The G-09-2 and G-09-3 defaults (`input_too_large`, `output_rejected`) are asserted.

**AC-5: Slots (IT-09-9)**
Given 40 queued jobs and two FD sockets
When the supervisor runs
Then the lane sequences and the per-slot concurrency meet IT-09-9

## Constraints

- The real API, queue and PostgreSQL in every case. FD only for fault and protocol behaviour.
- Each case keeps its "Max execution time" from 09 tests.md.
