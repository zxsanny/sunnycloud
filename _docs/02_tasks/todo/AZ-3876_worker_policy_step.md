# Policy step: poster offset, stills edges, transcode decision and output compliance check

**Task**: AZ-3876_worker_policy_step
**Name**: Worker policy step
**Description**: Turn a claimed job, its probe and the slot's cached decoder capabilities into the step the slot loop executes (stills edges, poster offset, skip, remux or a CPU transcode plan) using `media-shared::policy`, and fail a non-compliant decoder output as `OutputInvalid` so a rerun cannot loop
**Complexity**: 2 points
**Dependencies**: AZ-3868_worker_crate_skeleton, AZ-3838_media_policy, AZ-3837_media_shared_params_protocol, AZ-3771_media_shared_skeleton
**Component**: 09_worker_supervisor
**Tracker**: AZ-3876
**Epic**: AZ-3639 (E-20)
**Ref**: E-20/c10a

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/media-shared/skeleton_types.md` — `Probe`, `Plan`, `Decision`, `DecoderCapabilities`, `EncoderPref`, `Params`.
- `_docs/02_document/contracts/08_media_jobs/media_policy.md` — `is_compliant`, `decide`, `poster_offset_ms`, produced by E-18/c03.
- `_docs/02_document/contracts/08_media_jobs/worker_job_protocol.md` — `Params` values per `params_version`, produced by E-18/c02.

## Problem

The decoder only executes; the supervisor decides what to produce (ADR-009). A compliant clip must never be re-encoded and an index-at-end clip must be remuxed (AC-11); the poster comes from the policy's offset (1 s, or 10% for clips under 10 s); and a decoder output that is itself non-compliant must fail once, permanently, or the same job would loop on every retry (AC-44).

## Outcome

- One pure step chooser used by the slot loop, built on the shared policy functions; no policy rule is restated in the supervisor.
- A non-compliant transcode output is `OutputInvalid`, permanent, with no CPU rerun.

## Scope

### Included
- `stills`: edges from `Params[params_version]` (1440 then 320).
- `poster`: `render_still{max_edge: 320, poster_at_ms: poster_offset_ms(duration)}`.
- `transcode`: `decide(probe, caps, encoder_pref)` -> `SkipCompliant`, `Remux` or `Transcode(Plan)`; in v1.0 `encoder_pref` is always `Cpu`.
- Output check: `is_compliant(probe_of_output)`; false -> `OutputInvalid` (permanent).

### Excluded
- The policy table itself and its tests (E-18/c03); executing the step (E-20/c06); NVENC preference (E-20/c10b, v1.1).

## Acceptance Criteria

**AC-1: Poster offset (IT-09-3, AC-10)**
Given video probes of 9 s and 60 s
When the poster step is chosen
Then it requests `max_edge 320` with `poster_at_ms` 900 and 1,000

**AC-2: Transcode decision (AC-11, IT-09-3)**
Given a DS-07 compliant probe, a portrait 1080x1920 compliant probe, a DS-35 moov-at-end probe and a 10.5 Mbit/s probe
When the transcode step is chosen
Then the first two are `SkipCompliant`, the third `Remux`, and the fourth `Transcode{encoder: Cpu, faststart: true, tone_map: false}`, and the serialised plan holds only enumeration and number fields

**AC-3: Stills edges from parameters**
Given a `stills` job with the current `params_version`
When the stills step is chosen
Then it requests edges 1440 then 320 as defined by that version's `Params`

**AC-4: Non-compliant output fails once (AC-44, IT-09-5)**
Given a transcode reply whose `probe_of_output` is 12 Mbit/s H.264
When the output is checked
Then the result is `OutputInvalid` with disposition `Permanent`, and the step asks for no CPU rerun

## Non-Functional Requirements

**Performance**
- Pure functions; no I/O.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Poster step for 9 s and 60 s | 900 and 1,000 ms |
| AC-2 | Step per probe class (compliant, portrait, moov at end, one non-compliant axis) | Skip, skip, remux, transcode |
| AC-3 | Stills edges per `params_version` | 1440 then 320 |
| AC-4 | Output check on compliant and non-compliant probes | Pass; `OutputInvalid` permanent |

## Blackbox Tests

Run in harness H2 (E-20/c11).

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | FD scripted probes | IT-09-3 | Offsets and decisions as specified | AC-11, FT-P-23, FT-P-25 |
| AC-2 | RD with DS-07, DS-35 | IT-09-4 | Skip with 0 bytes; real remux | AC-11 |
| AC-4 | FD non-compliant output | IT-09-5 | No post; `dead` after one attempt | AC-44, FT-P-26 |

## Constraints

- The supervisor never restates the compliance rule; it calls `media-shared::policy` (09 §2, ADR-015).

## Risks & Mitigation

**Risk 1: Rule drift between API and worker**
- *Risk*: a local copy of the rule diverges from the shared one.
- *Mitigation*: only the shared functions are called; unit tests check the step mapping, not the rule.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Split from E-20/c10 (2 points planned): the row mixes v1.0 work (policy step, output check) with the v1.1 NVENC preference, but the epic labels the encoder-choice task `release:v1.1` and the v1.0 pipeline AZ-3661 depends on this row; the NVENC part is E-20/c10b | Parent (Step 4 ref map) | open | Low |
