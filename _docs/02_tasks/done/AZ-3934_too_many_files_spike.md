# Spike: decide the `TooManyFiles` selection limit

**Task**: AZ-3934_too_many_files_spike
**Name**: TooManyFiles limit spike
**Description**: Measure browser heap and start time for very large file selections and record the selection size at which `enqueueUpload` refuses with `TooManyFiles`
**Complexity**: 1 point
**Dependencies**: AZ-3652_initial_structure
**Component**: 16 web_app (Svelte SPA)
**Tracker**: AZ-3934
**Epic**: AZ-3632 (E-26)
**Ref**: E-26/c05

## Problem

`enqueueUpload` names the error `TooManyFiles`, but no limit raises it, so IT-16-7 is BLOCKED (`tests/spec-gaps.md` row IT-16-7). A folder pick of N files creates N `File` objects before the queue cap (`web.max_inflight_files`, 200) applies; PT-16-4 requires 100,000 tiny files to work with heap at most 150 MB (failure above 300 MB) and the first upload within 5 s.

## Outcome

- Heap after selection and time to the first upload request measured for 10,000, 100,000, 250,000 and 500,000 tiny files in Chromium and Firefox (Safari when the Mac is available).
- A proposed limit and message, confirmed by the owner, recorded as a build constant in `components/16_web_app/description.md` §10 and in IT-16-7 of the 16 test spec; the spec-gaps row resolved.
- E-26/c09 implements the refusal from the record.

## Scope

### Included
- A probe page with a folder picker that counts `File` objects and samples heap.
- Folder trees from the many-small generator (DS-01 expanded).

### Excluded
- The upload itself (E-26/c09); drag-and-drop of folders (same `File` count, measured once as a cross-check).

## Acceptance Criteria

**AC-1: Selections measured**
Given folder trees of 10,000, 100,000, 250,000 and 500,000 tiny files
When each is selected by folder pick in Chromium and Firefox
Then the record holds the heap after selection and the time to the first upload request per size and engine

**AC-2: Limit proposed**
Given the measurements
When the proposal is written
Then the limit is at least 100,000 (PT-16-4 must pass), is the largest measured size whose heap stays at or below 150 MB, and comes with a message that points large trees to the desktop client (16 §5)

**AC-3: Recorded and unblocked**
Given the owner's confirmation
When the spike closes
Then 16 §10 lists the limit as a build constant, IT-16-7 has its BLOCKED clause replaced by the limit and the expected refusal, and the spec-gaps row IT-16-7 is marked resolved with the date

## Non-Functional Requirements

**Performance**
- PT-16-4 targets: heap at most 150 MB (failure 300 MB), first upload at most 5 s (failure 15 s) for 100,000 files.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Probe counter on a 1,000-file tree | Count equals 1,000 |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Generated folder trees; `browsers` container | Folder pick, heap and start time | Values recorded per size and engine | PT-16-4 |

## Constraints

- The limit is a selection limit only; the in-flight cap of 200 stays as specified.

## Risks & Mitigation

**Risk 1: Engines differ widely**
- *Risk*: Firefox may hold more per `File` than Chromium.
- *Mitigation*: the proposal uses the lower engine's value.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The limit is a product choice; the spike proposes, the owner confirms | Owner | open | Low |
