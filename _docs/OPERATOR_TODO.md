# Operator to-do

What waits for the operator. The agent writes the rows, re-checks the needs itself and closes them. No secret values in this file.

## Needs

| id | need | why / blocks | how to provide | verify (the agent runs it) | since | status |
|----|------|--------------|----------------|----------------------------|-------|--------|

## Decisions

| id | question | options | default | blocks | since | status |
|----|----------|---------|---------|--------|-------|--------|
| D1 | How does the worker suite kill the supervisor in the middle of an input download and in the middle of an output upload? | A: one-shot hold on the API test image's `:9091` test listener / B: fault proxy service between worker and API in the test stack / C: hold inside the supervisor's test build | A, because it lands both kills by construction and leaves the release images and the supervisor unchanged. Needs attention: card `_docs/02_task_plans/az-3878-worker-suite/decisions.md` D1; the second opinion differs (it proposed A; the session had leaned to B) | AZ-3878 AC-6 (IT-09-14 kill at fetch and post), AZ-3661's two NOT-RUN legs | 2026-11-26 | open |

## Closed
