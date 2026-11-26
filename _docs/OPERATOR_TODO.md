# Operator to-do

What waits for the operator. The agent writes the rows, re-checks the needs itself and closes them. No secret values in this file.

## Needs

| id | need | why / blocks | how to provide | verify (the agent runs it) | since | status |
|----|------|--------------|----------------|----------------------------|-------|--------|

## Decisions

| id | question | options | default | blocks | since | status |
|----|----------|---------|---------|--------|-------|--------|
| D1 | How does the worker suite kill the supervisor in the middle of an input download and in the middle of an output upload? | A: one-shot hold on the API test image's `:9091` test listener / B: fault proxy service between worker and API in the test stack / C: hold inside the supervisor's test build | A, because it lands both kills by construction and leaves the release images and the supervisor unchanged. Needs attention: card `_docs/02_task_plans/az-3878-worker-suite/decisions.md` D1; the second opinion differs (it proposed A; the session had leaned to B) | AZ-3878 AC-6 (IT-09-14 kill at fetch and post), AZ-3661's two NOT-RUN legs | 2026-11-26 | decided 2026-11-26: A (operator, "Hold in API test image") |
| D2 | IT-09-7 (c), "a body that grows past its Content-Length": the real API can never send it (the contract fixes an exact `Content-Length`, and hyper trims a longer body), so the stack cannot run that leg as written. Waive it on the stack, or build a test-only API hook? | A: waive the stack leg; the case is NOT-RUN with a pointer to the worker's own test of the abort (`src/worker/tests/api_input.rs:175`), and 09 tests.md (c) is reworded to "a body with no declared length that grows past the input limit (client side)" / B: add a test-image API hook that streams the input without a declared length | A, because it adds no test-only behaviour to the API image and the client's abort is already tested. Needs attention (it changes the coverage of an acceptance check): card `_docs/02_task_plans/az-4385-worker-faults/decisions.md` D2b; the second opinion agrees | AZ-4387 AC-1 leg (c) only | 2026-11-26 | open |

## Closed
