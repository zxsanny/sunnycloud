# Spike: slot-loop boundaries (G-09-1), OffsetClock effect (G-09-4), scratch arithmetic

**Task**: AZ-3867_worker_spike_slots_clock_scratch
**Name**: Worker supervisor spike
**Description**: Record the slot-loop boundaries the tests rely on, fix what the test clock changes inside the supervisor, and reconcile the interactive scratch size (512 MiB in the component spec, 1g in the deployment docs) with its arithmetic
**Complexity**: 2 points
**Dependencies**: AZ-3652_initial_structure
**Component**: 09_worker_supervisor
**Tracker**: AZ-3867
**Epic**: AZ-3639 (E-20)
**Ref**: E-20/c01

## Problem

Three open points block exact tests. The slot-loop type is not named (G-09-1), so tests must know which boundaries they may rely on. The supervisor polls the API's clock offset, but nothing says what it changes inside the supervisor (G-09-4), so IT-09-18 has no pass criterion. The interactive scratch is 512 MiB in 09 §6 and `size=1g` in `containerization.md`; one of them is wrong, and the general scratch (24 GiB) is to be revisited at gate S5.

## Outcome

- A written list of the slot boundaries (one job per slot, lane mask, own heartbeat, observable through API, socket, scratch and ops port), with the loop type left to the implementer; G-09-1 closed by decision.
- A testable statement of the offset clock's effect, written into 09 §2 and IT-09-18; G-09-4 closed.
- One interactive scratch size in both docs, with the arithmetic; the general scratch note for gate S5; a runbook note for scratch sizing and for worker secret rotation through `worker_api_token_next`.

## Scope

### Included
- Read IT-09-9, IT-09-11 to IT-09-15 and PT-09-1 and list the boundaries they use; record in `tests/spec-gaps.md` and 09 §3.
- G-09-4: evaluate the candidate drawn from IT-09-17 (the status report's `image_age_days` is computed from the decoder `build_timestamp` against the supervisor's clock, so the offset moves it) and any other wall-time use (log timestamps); write the chosen effect into 09 §2 and give IT-09-18 a pass criterion.
- Scratch: interactive = largest photo input (256 MiB) + two outputs (at most 32 MiB each) + margin, inside the 2 GiB container limit; general = 2 x `worker_max_input_video` + 4 GiB; write the result into both docs or raise the owner question with both numbers.
- Runbook notes in `deployment/deployment_procedures.md`: scratch sizing and secret rotation steps.

### Excluded
- Implementing the loop (E-20/c06) or the offset poll (E-20/c09).

## Acceptance Criteria

**AC-1: Boundaries recorded (G-09-1)**
Given the tests that name slot behaviour
When the spike closes
Then `tests/spec-gaps.md` marks G-09-1 resolved with the list of boundaries and states that the loop type is the implementer's choice

**AC-2: Offset effect testable (G-09-4)**
Given the candidate effect from IT-09-17
When the spike closes
Then 09 §2 states which supervisor outputs use the offset clock, and IT-09-18 has a pass criterion that observes it (for example `image_age_days` shifts by the advanced days)

**AC-3: One interactive scratch size**
Given 09 §6 (512 MiB) and `containerization.md` (`size=1g`)
When the spike closes
Then both documents give the same number with the arithmetic, or an open owner question names both values and the trade-off

**AC-4: Runbook notes**
Given the E-20 definition of done
When the spike closes
Then `deployment_procedures.md` has a scratch-sizing note (with the S5 revisit of the 24 GiB general scratch) and a worker secret rotation note (`worker_api_token_next` on the API, then the worker's secret file, then a worker restart)

## Non-Functional Requirements

**Reliability**
- Scratch sizes never exceed the decoder container memory for the tmpfs (`decoder-interactive` 2 GiB).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1, AC-3, AC-4 | Spike record review (`tests/spec-gaps.md`, 09 §3 and §6, `containerization.md`, `deployment_procedures.md`) | Each decision recorded where the AC names it |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-2 | Test clock advanced (later, E-20/c11) | IT-09-18 with the new criterion | Effect observed | |

## Constraints

- The decision must not add state to the supervisor: no persisted data, nothing to rehydrate (09 §4).

## Risks & Mitigation

**Risk 1: The offset has no useful effect**
- *Risk*: the supervisor measures only local durations, so the test clock changes almost nothing.
- *Mitigation*: AC-2 accepts a narrow effect as long as it is observable and stated.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Interactive scratch 512 MiB (09 §6) vs `size=1g` (containerization) until AC-3 settles it | Deployment owner | open | Low |
