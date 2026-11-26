# Worker suite on H2: decoder faults with the fake decoder

**Task**: AZ-4385_worker_suite_faults_slots
**Name**: Worker suite, decoder faults (H2)
**Description**: The decoder fault cases of AZ-3878's AC-4 against the real API worker listener and PostgreSQL on the stack, with FD scripting the faults
**Complexity**: 5 points
**Dependencies**: AZ-4382_fake_decoder_service, AZ-4383_h2_stack_profile_ctl, AZ-4384_worker_suite_gate_decisions (the suite's fixtures, `fdkit`, `jobkit`)
**Component**: 09_worker_supervisor
**Tracker**: AZ-4385
**Epic**: AZ-3639 (E-20)
**Ref**: split from AZ-3878 (E-20/c11); split again in batch 131: IT-09-9 and IT-09-10 moved to AZ-4386, IT-09-7 and IT-09-6 (i) to AZ-4387

## Acceptance Criteria

The expected results are in `_docs/02_document/components/09_worker_supervisor/tests.md`.

**AC-4: Decoder faults (IT-09-6 (a) to (h), IT-09-11, IT-09-12, IT-09-13)**
Given:
- the FD fault scripts of IT-09-6 (stalled once and twice, NVENC encoder failure, a failed remux, unsupported, corrupt, limit exceeded, a socket closed mid-job);
- the silent and the over-time decoders of IT-09-11;
- the hostile output files of IT-09-12 (symlink to the worker secret, hard link, FIFO, sparse file over the cap, directory) and the control file;
- the hostile frames of IT-09-13.

When each case runs
Then each meets its expected result, including no secret byte in any posted body and the supervisor process staying up. The G-09-3 default (`output_rejected`) is asserted.

## Notes

- IT-09-12's control case needs postable WebP bytes of known dimensions (FD's `bytes` output); `tests/tools/gen_media.py` can make the fixture.
- With `fs.protected_hardlinks=1`, FD (uid 10003) may only hard-link an `in/` file it can read and write (batch 129, row 3): check the mode the supervisor gives `in/` files; an FD failure to plant shows as `fd_error` in its record and must fail the case.
- An FD answer later than the 1 s silence timeout needs `alive_every_ms` (over-time cases).

## Constraints

- The real API, queue and PostgreSQL in every case. FD only for fault and protocol behaviour.
- Each case keeps its "Max execution time" from 09 tests.md as far as the harness allows (batch 130, row 3: times include supervisor restarts).
