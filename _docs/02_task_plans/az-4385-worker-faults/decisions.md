# Design decisions: az-4385-worker-faults (AZ-4385 split; AZ-4387 input fetch)

## Applied without asking

| id | decided | why it is safe | undo | date |
|----|---------|----------------|------|------|
| D2a | IT-09-6 (i), a fetch that fails on the server, is produced on the real API by the listed fault injector "Bit flip at rest" (`_docs/02_document/tests/environment.md:191`, action `bit-flip` in `tests/tools/e2e-ctl/actions.json`, not implemented yet): one byte of the input blob's first chunk is flipped while the supervisor is stopped, the blob reader refuses the first chunk before any body byte (`src/contracts/src/blobstore/reader.rs:12`), and the API answers 5xx, which the worker maps to a transient failure (`src/worker/src/api/mod.rs:240`) | reversible: yes (test stack only: ctl action and suite, no image or contract change); if wrong: nothing in a release (ctl and the suite never ship); standard: an existing fault-injector row of the stack's design (`environment.md:180`); critical resource: none named (`_docs/02_document/decision-criteria.md` absent) | drop the `bit-flip` handler and the case | 2026-11-26 |

Notes for D2a from the second opinion, checked here: assert the API's 5xx for the input route (a flip in a later chunk would give 200 and an abort mid-body, a pass for the wrong reason); do not move the test clock in that case; run it in its own module (the corrupt read marks the blob unavailable and turns blob health degraded until the module's golden restore).

## Asked

### D2b: IT-09-7 (c), "a body that grows past its Content-Length", on the stack

**Needs attention because**: it changes the coverage of an acceptance check (AZ-4387 AC-1 (c) on the real API), and the project's rule asks for 100% scenario coverage or an explicit waiver per uncovered item.
**Came up in**: implement, splitting AZ-4385 (batch 131); AZ-4387 spec "Open: decision D2".
**Decision**: the worker suite on H2 is meant to run every IT-09-7 leg against the real API. Leg (c) needs a server that sends more bytes than the `Content-Length` it declares. The real API never does: the contract fixes an exact `Content-Length` (`_docs/02_document/contracts/08_media_jobs/worker_job_protocol.md:54`), and hyper cuts a sized body to the declared length on the server (hyper 1.12.0 `src/proto/h1/encode.rs:143-150`). The worker refuses a declared length over its limit before reading (`src/worker/src/api/input.rs:49-54`) and aborts a body while streaming only when no length was declared (`:57-60`); that branch is covered by `src/worker/tests/api_input.rs:175` (a chunked body with no length).

**Evidence**: `_docs/02_document/components/09_worker_supervisor/tests.md:76-77` (IT-09-7 (c)), `:16` (AC-72 traceability); `_docs/02_tasks/todo/AZ-4387_worker_suite_input_fetch.md` AC-1; `src/worker/tests/api_input.rs:151-193`; the AC coverage rule (`quality-thresholds.md`, "AC/restriction scenario coverage").
**Not verified**: whether a test-image hook that streams without `Content-Length` could live in the existing hold module without changing the release-symbol check.

| | A: waive the stack leg | B: a test-image API hook |
|---|---|---|
| Reversible | yes: docs and a NOT-RUN case | yes, but test-only code in the API image stays to maintain |
| If it is wrong | nothing in a release; the stack does not prove the abort against the real listener | nothing in a release; a hook that leaked into a release would let a request change how input is served (guarded by the release-symbol check) |
| Standard | existing precedent: NOT-RUN with a crate pointer (`test_it_09_06_retry.py`, leg (c)) | own invention (a second test hook beside the hold) |
| Critical resource | none named | none named |
| Automation | one manual step: the operator's waiver; tests.md (c) reworded to "a body with no declared length that grows past the input limit (client side)" | by itself once built |

**Simplest alternative**: it is option A.

**What goes wrong → what happens**

| who or what is wrong | A | B |
|---|---|---|
| a future API change streams input without a declared length | the leg becomes reachable and the waiver goes stale; tie the waiver to `worker_job_protocol.md:54` so a contract change reopens it | the hook keeps testing a path the real API now has |
| a worker regression in the streaming abort | the crate test `api_input.rs:175` fails | the stack case and the crate test fail |

**Recommendation**: A, decided by the simpler option (no new test-only behaviour in the API image; the behaviour is the client's and its test exists).
**Second opinion**: waive (c) on H2 with a pointer to `api_input.rs:175` and reword tests.md (c); for (i) use the bit-flip fault: agrees (it also showed that a test hold could not produce (c) either, because hyper truncates what it adds).
**Outcome**: asked 2026-11-26, answer: A (operator, "Waive the stack leg"). The waiver is tied to `worker_job_protocol.md:54`: a contract change that lets input stream without a declared length reopens it.
**Flagged**: no
