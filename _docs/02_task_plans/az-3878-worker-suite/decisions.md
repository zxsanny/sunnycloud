# Design decisions: AZ-3878 worker integration suite (H2)

## Asked

### D1: How a supervisor kill lands inside the input fetch and inside the output post

**Needs attention because**: the recommended option adds test-only code to the API crate. If that code ever reached a release build, anyone who can reach port 9091 could stall worker transfers (a media-pipeline outage), so "if it is wrong" is not "nothing".
**Came up in**: implement, batch 128 pick (AZ-3878 AC-6, IT-09-14). It is also the NOT-RUN fetch and post legs of AZ-3661 (`_docs/_process_leftovers/2026-11-24_az-3661-not-run-legs.md:16-20`).
**Decision**: AC-6 kills the supervisor during the fetch, during the transcode and during the post, and expects attempt 2 to end `succeeded` (`_docs/02_tasks/todo/AZ-3878_worker_integration_suite.md:67-70`). On the stack, a fetch or post of a test-sized file takes milliseconds. Nothing holds the supervisor inside those steps. The task's own plan, "wait for the API access-log line", cannot work: the API logs no line for a 2xx answer (`src/components/11_http_api/src/middleware/observe.rs:34-35,64`). A marker seen after the post would also be too late, because the last output closes the job at attempt 1 (`src/components/08_media_jobs/src/derived_store.rs:342-345`).

**Evidence**:
- The worker's HTTP client sets no request timeout, so a held call keeps the supervisor inside the step (`src/worker/src/api/mod.rs:5-7`).
- `:9091` exists only in test images (`src/components/11_http_api/src/test_clock.rs:75-81`).
- ctl may call the API on 9091 (`tests/tools/e2e-ctl/ctl_actions.py:27`).
- The release-symbol check exists (`.github/workflows/ci.yml:377-381`, `scripts/ci/check-release-symbols.sh`).
- The decoder's `test-freeze` test-build feature (`src/decoder/Cargo.toml:27`) and the worker's `test-clock` feature (`src/worker/Cargo.toml:48-50`) are precedents for test-only builds.
- IT-09-14 (`_docs/02_document/components/09_worker_supervisor/tests.md:129-135`) and NFT-RES-36 (`_docs/02_document/tests/resilience-tests.md:562-579`) name the case.

**Not verified**:
- the CI step that proves the release API image refuses `:9091`;
- the decoder's 2 s cancel on socket loss.

| | A: hold in the API test image | B: fault proxy between worker and API | C: hold in the supervisor test build |
|---|---|---|---|
| What it is | A one-shot `POST /test/worker-hold {route, kind, after_bytes}` on `:9091` parks the next matching worker input or output stream after N bytes. The suite waits until it is held, kills the supervisor, then releases it | A small test-only service in `tests/compose.test.yml` forwards worker traffic to the real API. A ctl action makes it stall the next matching request mid-body | A `test-hold` feature in the `-test` worker image pauses inside fetch and post while a marker file exists |
| Reversible | yes: test image only, no schema, wire or supervisor change | yes: stack files only | yes: test feature only |
| If it is wrong | leaks into release → outage (stalled worker streams; no data read or changed). Guarded by `:9091` being refused on release and a new release-symbol marker | nothing (not in any image) | leaks into release → stalled jobs. Guarded the same way as A |
| Standard | extends the `:9091` test listener and the test-feature pattern; the hold itself is ours (failpoint style) | named practice (Toxiproxy-style fault proxy); a new stack service | extends the test-feature pattern; puts test code in the component under test, at the point under test |
| Critical resource | none named (`_docs/02_document/decision-criteria.md` does not exist) | none named | none named |
| Automation | by itself (the suite arms and releases it) | by itself | by itself |
| Lands inside the step | yes, by construction, for both fetch and post | fetch yes; post depends on timing (body buffered or not) | fetch yes; a hold "before the post" is not "during" it |

**Simplest alternative**: kill on a marker, or use a large input. It lost: there is no marker for a 2xx answer, and a large input still races on a docker network.

**What goes wrong → what happens**

| who or what is wrong | A | B | C |
|---|---|---|---|
| the test clock is moved before the kill | the lease ends, the held call is dropped, the kill lands **late** and proves IT-09-10 instead. Move the clock after the kill | same | same |
| the hold is not one-shot | attempt 2 also parks; the run fails **late** at 180 s | same | same |
| the suite runs against the release API image | the hold route answers 404: the legs must report NOT-RUN | not affected | not affected |
| a build leaks the feature | release-symbol check fails CI | n/a | release-symbol check fails CI |

**Second opinion**: differs. It proposed A, which the session had not drafted. The session had leaned to B. The difference: A lands the post kill by construction and changes no image, network path or supervisor in the release. B adds a moving service and leaves the post leg timing-dependent. The session now recommends A, deciding on priority 3 (the simpler option that extends an existing mechanism), with automation equal across the options.

**Answer**: open (OPERATOR_TODO D1).
