# `admin media-requeue`, `admin media-stats`

**Task**: AZ-3805_media_commands
**Name**: Media operator commands
**Description**: Requeue dead media jobs by account and kind after a policy fix, and print the media queue depth per lane and status
**Complexity**: 2 points
**Dependencies**: AZ-3788_cli_skeleton, AZ-3789_composition_root, AZ-3846_media_admin_health_doctor
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3805
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/c13

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part3_blob_share_media_vault.md` — `MediaAdmin` (`requeue_dead`, `stats`).
- `_docs/02_document/contracts/12_operator_cli/operator_command_surface.md` — command rows (E-22/c02).

## Problem

Dead jobs for files that a policy change can now handle stay dead for 14 days (08 risk table). The operator needs to requeue them, narrowed to one account or one kind, and to see whether the queue is moving.

## Outcome

- `admin media-requeue [--account] [--kind]`: one `MediaAdmin::requeue_dead(filter)` call with the given filters (none means all); prints the count; exit 0.
- `admin media-stats`: one `MediaAdmin::stats` call; prints queue depth per lane and status; exit 0.
- A kind that 08 does not define, or an account id that is not an id, exits 3 with no call.

## Scope

### Included
- Both commands: parsing, the business call, output in text and `--json`, exit mapping.

### Excluded
- Requeue rules and queue counts (08, E-18/c05, E-18/c11).

## Acceptance Criteria

**AC-1: Requeue with filters (IT-12-16)**
Given a recording `MediaAdmin`
When `admin media-requeue --account A`, `admin media-requeue --kind transcode` and `admin media-requeue` run
Then each makes one `requeue_dead` call with exactly that filter, prints the returned count and exits 0

**AC-2: Invalid filters**
Given `--kind banana` and `--account not-an-id`
When `admin media-requeue` runs
Then each exits 3 with no call

**AC-3: Stats (IT-12-16)**
Given a recording `stats` answer with depths for both lanes and several statuses
When `admin media-stats` runs, in text and in `--json`
Then every lane and status depth is printed with the same facts in both modes, and the exit code is 0

## Non-Functional Requirements

**Performance**
- `admin media-stats` is part of the PT-12-1 serial run (E-22/c17).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Filter mapping | One call; exact filter |
| AC-2 | Argument validation | Exit 3, no call |
| AC-3 | Stats printing | Same facts in text and JSON |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | Harness, recording `MediaAdmin` | `media-requeue` variants (IT-12-16) | One call per run; exit 0 / 3 | |
| AC-3 | Harness | `media-stats` (IT-12-16) | Depths printed; exit 0 | |

The owner-effect parts of IT-12-16 (requeued rows become `queued`, counts equal a harness query) are proven by E-18/c05 and E-18/c11.

## Constraints

- One business call per command; no SQL in 12.

## Risks & Mitigation

**Risk 1: Requeue of hostile files loops**
- *Risk*: requeued dead jobs fail again and again.
- *Mitigation*: 08's attempt limit applies to requeued jobs; the command only triggers the owner's operation.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The accepted kind values are 08's job kinds; the CLI takes them from 08's type and lists none of its own | E-18 owner | accepted-risk | Low |
