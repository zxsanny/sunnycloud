# Batch Report

**Batch**: 39
**Tasks**: AZ-3705, AZ-3721, AZ-3731, AZ-3732 (spikes, documents only, no code); AZ-3722 not done (blocked, see item 1)
**Date**: 2026-11-11

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3705_tree_spike_names_effect_tests | Done: G-1 decided by the owner (one name, stored as sent); marker and suffix items confirmed client-side; `CommitEffect` contract-test plan complete | `tests/spec-gaps.md` (G-1, ST-14-5), `tests/blackbox-tests.md` (G-1), `contracts/02_tree/tree_commit.md` | none (spike) | Items 2, 3 |
| AZ-3721_blob_rewrite_rule_spike | Done: owner chose "identical bytes accepted, different bytes `BlockConflict`"; ST-04-8 has a procedure and pass/fail criteria; external review scope written | `04_blobstore/description.md` (`write_block` row, risk row, new section 15), `04_blobstore/tests.md` (ST-04-8), `contracts/04_blobstore/blob-writer.md` 1.1.0, `tests/spec-gaps.md` | none | Items 4, 5 |
| AZ-3731_transfer_spike_resend_stale_base | Done: owner chose AC-3 = 16 MiB (G-03-1) and the proposed commit answer (G-03-3); commit contract frozen | `00_problem/acceptance_criteria.md` (AC-3), `tests/traceability-matrix.md`, `data_model.md`, `03_transfer/tests.md`, `contracts/03_transfer/commit_service.md` 1.1.0, `upload_service.md`, `tests/spec-gaps.md`; concern rows of 8 task specs | none | Items 6-8 |
| AZ-3732_transfer_spike_nothing_to_send | Done: rule per case recorded (agent decision, owner may overturn); IT-13-16 (B) unblocked | `contracts/03_transfer/upload_service.md` 1.1.0, `03_transfer/tests.md` (IT-03-1), `13_sync_core/tests.md` (IT-13-16), `tests/spec-gaps.md` | none | Item 9 |
| AZ-3722_group_commit_fs_spike | **Not done, blocked**: no privileged runner (item 1) | `_docs/_process_leftovers/2026-11-11_az-3722-privileged-runner.md` | none | Item 1 |

## Code Review Verdict: PASS

Documents only. Four task specs archived to `done/`; Jira AZ-3705, AZ-3721, AZ-3731, AZ-3732 are `In Testing`.

## Test Suite

Not run: no code changed (last full run in batch 37: 2762 passed, 0 failed, 9 ignored).

## Owner decisions (this batch, 2026-11-11)

1. G-1: an NFC and an NFD spelling are one name; the stored name keeps the first writer's spelling. This is what `sc_names` already does.
2. ST-04-8: a second `write_block` of one index accepts identical bytes (decrypt the stored block and compare, nothing written) and refuses different bytes with `BlockConflict`. Reason: after a crash an un-checkpointed block is re-sent with the same bytes, so refusing every second write would stop every resume.
3. G-03-1: AC-3 is changed from 10 MiB to 16 MiB; the limits stay.
4. G-03-3: stale base: `node_id` is the file with the new content, `conflict_copy` is the older sibling; name taken: `node_id` is the new file and `conflict_copy` repeats its own id; commit never answers `EditedSince`.

## Issues and discovered scenarios

| # | Scenario or question | Spec ref | Handling | clear / unclear |
|---|----------------------|----------|----------|-----------------|
| 1 | AZ-3722 cannot run: `sudo -n true` needs a password, so no loop mount; the spec forbids tmpfs as a substitute | AZ-3722 constraint | Leftover with three ways to unblock: passwordless sudo for mount/losetup, two ready mounted directories (XFS and ext4), or the reference server | unclear |
| 2 | The `%5F` marker and the ` (2)` suffix are still open owner items, but only for client wording; the server is unaffected (checked against the E-05 epic and the IT-02-1 accept list) | AZ-3705 AC-2 | Noted in the ST-14-5 row, which stays BLOCKED on the client wording | unclear |
| 3 | The `CommitEffect` plan needed one edit: the media-job rollback row had no test ID (now IT-02-9 and IT-08-1); the other five required cases were already in the contract | AZ-3705 AC-3 | Consumers list already named E-14/c08, c09, c15 and E-18/c06 | clear |
| 4 | `BlobError::BlockConflict` is a new variant in `contracts` part 3 | AZ-3721 AC-1 | E-10/c06 adds it; 03 maps it to its own `BlockConflict`; no other change to 03 | clear |
| 5 | External review: the owner still has to name the reviewer and a date; the scope is written (description section 15) | AZ-3721 AC-4 | Needs the owner before the v1.0 release tag | unclear |
| 6 | AC-50 (web: re-select a file, at most 10 MiB re-sent) has the same 16 MiB protocol bound but was not part of the question; I changed AC-3 only | AZ-3731 | Decision needed: change AC-50 to 16 MiB too (my suggestion) | unclear |
| 7 | Derived 10 MiB thresholds elsewhere still read 10 MiB: AC-50 rows, E-23, E-24, E-26, 13, 16, FT-P-01, NFT-PERF-05, NFT-RES-01, NFT-RES-29, results_report rows 1 and 50, ADR-004, system-flows | AZ-3731 AC-2 | Listed in the G-03-1 row for the consuming tasks and test writers; not edited here | unclear |
| 8 | The `EditedSince` variants of `CommitError` and `EntryError` in `contracts` (and the 11 mapping of them) are not removed here | AZ-3731 AC-3 | E-14/c08 removes them; the contract already says commit never answers it | clear |
| 9 | `NothingToSend` needs three read calls (`find_live_node`, `resolve_live_file`, `get_revision`) where the task's NFR says one lookup and one read; one fewer if E-13 lets `NodeRef` or `LiveFile` carry the hash | AZ-3732 NFR | Written into the contract; E-13 may shorten it | unclear |
| 10 | Items from batches 29-38 stay open | earlier batches | | unclear |

## Commit

`[AZ-3705] [AZ-3721] [AZ-3731] [AZ-3732] Tree, blob and transfer spike decisions` — Loop: 1

## Next Batch

Ready now: AZ-3722 (blocked, item 1), AZ-3893 (Windows only) and 25 other decision spikes (AZ-3787, 3799, 3849-3851, 3867, 3881-3887, 3913, 3921, 3930, 3933, 3934, 3973, 3985-3988, 4004, 4005). Unblocked by this batch but waiting on other tasks: AZ-3734, AZ-3736, AZ-3738, AZ-3747, AZ-3760, AZ-3762, AZ-3962, AZ-3953 (client spike, now has the owner's G-03-1 answer). 246 tasks left.
