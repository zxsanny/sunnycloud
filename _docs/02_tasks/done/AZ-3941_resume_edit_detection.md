# IndexedDB resume records, edit detection and tab-close recovery

**Task**: AZ-3941_resume_edit_detection
**Name**: Upload resume and edit detection
**Description**: Keep a resume record per large upload in IndexedDB, resume after a closed tab when the user re-selects the same file with at most 10 MiB re-sent, restart on any change, and detect a file edited before commit
**Complexity**: 3 points
**Dependencies**: AZ-3939_transfer_service_small_path, AZ-3940_worker_large_upload, AZ-3950_playwright_web_harness, AZ-3736_transfer_resume_abort, AZ-3829_tree_transfer_feed_handlers
**Component**: 16 web_app (Svelte SPA)
**Tracker**: AZ-3941
**Epic**: AZ-3632 (E-26)
**Ref**: E-26/c11

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/16_web_app/transfer_service.md` — `resumeUpload`, resume record, produced by E-26/c09.
- `_docs/02_document/contracts/16_web_app/api_client.md` — client wrapper and error mapping, produced by E-26/c06.
- `_docs/02_document/contracts/03_transfer/upload_service.md` — session read and received-block bitmap, produced by E-14.

## Problem

A closed tab, a locked phone or a suspended Safari tab ends a browser upload (16 §7), and `File` handles cannot survive a reload. AC-3 and AC-50 still require that re-selecting the file re-sends at most 10 MiB, and that a file edited mid-upload is detected and restarted rather than committed with mixed content (FT-N-03).

## Outcome

- Each large upload has a resume record (`transfer_service.md`) for the life of its session.
- Re-selecting the same file resumes and sends only missing blocks; any change restarts with a clear reason.
- An edit before commit aborts and restarts; no mixed revision is ever published.

## Scope

### Included
- Writing the record when the session is created; removing it on commit, cancel and when the server reports the session gone.
- `resumeUpload(file, job)`: match on name, size and `lastModified`; read the session's received blocks; send only missing ones; `Resumed{resent_bytes}` or `Restarted{ChangedSinceLastTime}`; `UploadSessionGone`; `FileUnreadable`.
- Re-reading `size` and `lastModified` before commit; any change aborts and restarts.
- Resume banner and per-job "Select file" (mockup `ui_mockups/web-files.html`: "2 uploads stopped when the tab closed…", "Stopped when the tab closed. Select the same file to continue.").
- A hidden or suspended page: the job shows as stopped and keeps its record.
- IndexedDB unavailable: uploads still complete, no resume is offered, one notice says so.

### Excluded
- The first upload pass (E-26/c10); 50 GiB resume runs (E-26/c19b).

## Acceptance Criteria

**AC-1: Resume re-sends at most 10 MiB (AC-3, AC-50, IT-16-5, NFT-RES-29)**
Given a 5 GiB upload with the tab closed at about 50 %
When the tab is reopened and the same file is selected
Then the outcome is `Resumed` with `resent_bytes` at most 10 MiB, the file is not re-hashed from zero (the saved blocklist is used), the client key is unchanged, and the finished file's hash equals the local hash

**AC-2: A changed file restarts**
Given a stopped upload
When the user selects a file with the same name and a changed `lastModified`, and in a second run the same name with another size
Then each gives `Restarted{ChangedSinceLastTime}` with the "changed since last time" message and uploads from the start

**AC-3: A tampered record cannot corrupt**
Given a stopped upload whose stored blocklist was edited
When it resumes
Then the server rejects the wrong block hash, the job restarts, and the committed content equals the file

**AC-4: Expired session**
Given a stopped upload whose server session has expired (test clock)
When the user selects the file
Then the job shows `UploadSessionGone` with a clear message, the record is removed, and a new upload can start

**AC-5: Edit before commit restarts (IT-16-6, FT-N-03)**
Given a 1 GiB upload whose content is replaced at 60 %, and a second run where only `lastModified` changes
When the Worker reaches the pre-commit check
Then it aborts and restarts, the final server content equals the new file, and 0 revisions with mixed content exist

**AC-6: Stopped jobs are listed after reopening**
Given two large uploads stopped by closing the tab
When the app is opened again by the same account
Then the banner lists both with "Select file" actions, and another account in the same browser sees none of them

**AC-7: No secrets and blocked storage (ST-16-2 resume half)**
Given a resume record, and a run with IndexedDB blocked
When storage is dumped, and the second run uploads a 300 MiB file
Then the record holds no token, password or cookie value; the blocked run completes the upload and offers no resume

## Non-Functional Requirements

**Reliability**
- Resume re-send at most 10 MiB (AC-3); records expire with the 24-hour rolling session (16 §4).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1, AC-2 | Record matching on name, size and `lastModified` | Match resumes; any difference restarts |
| AC-1 | Missing-block computation from the received bitmap via the WASM resume diff | Only missing blocks scheduled |
| AC-6 | Records keyed per account | Other account sees none |

## Blackbox Tests

Playwright against the real stack (harness E-26/c19a).

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1..AC-4 | 5 GiB upload, tab closed at 50 %; changed file; tampered record; expired session | IT-16-5; NFT-RES-29; AT-16-1 | Resume <= 10 MiB; restart; no corruption; session gone | AC-3, AC-50 |
| AC-5 | 1 GiB upload; content replaced; `lastModified` touched | IT-16-6; FT-N-03 | Abort and restart; 0 mixed revisions | AC-50 |
| AC-6 | Two stopped uploads; second account | Banner and isolation | Listed for owner only | |
| AC-7 | Record dump; IndexedDB blocked | ST-16-2 (resume half) | No secrets; upload still completes | AC-58 |

## Constraints

- The server verifies every block hash; the saved blocklist is an optimisation, never trusted (16 §5).

## Risks & Mitigation

**Risk 1: Same name, size and time but different content**
- *Risk*: a file replaced with identical metadata matches the record.
- *Mitigation*: the server rejects wrong block hashes on PUT and the roll-up at commit (AC-3); the job restarts.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| IT-16-5 says the record is "kept per spec" on `UploadSessionGone`, while 16 §4 gives it a lifetime "until commit or session expiry"; AC-4 follows 16 §4 and removes it | E-26 owner | open | Low |

## Contract

This task implements `resumeUpload` and the resume record of the contract at `_docs/02_document/contracts/16_web_app/transfer_service.md`.
Consumers MUST read that file — not this task spec — to discover the interface.
