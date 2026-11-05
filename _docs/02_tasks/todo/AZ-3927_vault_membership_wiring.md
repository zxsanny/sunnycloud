# Vault membership on the commit path (`vault` flag, 100 MiB limit) and the share refusal

**Task**: AZ-3927_vault_membership_wiring
**Name**: Vault membership wiring
**Description**: Apply the membership mechanism chosen by E-30/c01: entries under a vault root are published with `vault = true` (so no media job is planned), uploads into a vault are refused above `max_vault_file_bytes`, shares of vault files are refused, and moves keep vault files decryptable
**Complexity**: 3 points
**Dependencies**: AZ-3913_vault_decisions_spike, AZ-3922_vaults_migration_config, AZ-3708_tree_folder_mutations, AZ-3709_tree_publish_core, AZ-3734_transfer_start_upload, AZ-3737_transfer_stage_small, AZ-3738_transfer_commit_upload, AZ-3754_transfer_commit_batch, AZ-3697_share_create_list_revoke, AZ-3841_job_planner_enqueue_effect; notes: and the follow-up tasks named by AC-4
**Component**: 20 vault_service [v1.2] (changes land on the 02, 03 and 06 paths with their owners)
**Tracker**: AZ-3927
**Epic**: AZ-3648 (E-30)
**Ref**: E-30/c07

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part2_tree_transfer_feed.md` — `PublishEntry.vault`, `PublishedEntry.vault` (E-00/c04).
- `_docs/02_document/contracts/02_tree/tree_commit.md`, `_docs/02_document/contracts/03_transfer/upload_service.md`, `commit_service.md` — publish and upload paths (E-13/c05, E-14/c04, E-14/c08).
- `_docs/02_document/contracts/contracts/part3_blob_share_media_vault.md` — `ShareService` (E-00/c05).

## Problem

v1.0 always publishes with `vault = false` (E-14/c08). In v1.2 the server cannot tell that an uploaded JPEG is ciphertext, so only membership can decide: a file under a vault root must get no thumbnail or transcode (08's planner reads the flag, E-18/c06), must be refused above 100 MiB (AC-14), and must not be link-shared (owner decision, `canonical-names.md`). Membership must be answered without calling 20 (ADR-015). AC-60 also requires that moving a vault file to another folder of the same vault leaves it decryptable, so a move must not touch its revisions.

## Outcome

- Every entry whose target lies under a vault root, at any depth and through the session and batch paths, is published with `vault = true`; entries outside are not.
- Vault uploads above `max_vault_file_bytes` are refused at upload start with a message naming the limit; uploads outside a vault are not subject to it.
- Shares of a vault file or a vault root are refused with 422 `target_not_shareable`.
- Moves follow the rules recorded by E-30/c01; a move inside one vault leaves revisions, `client_meta` and blobs untouched.

## Scope

### Included
- Setting the `vault` flag on publish for new and existing targets under a vault root, on the session path and the batch path.
- The `max_vault_file_bytes` check at upload start (declared size) and at commit for vault targets.
- 06's refusal of share creation for vault files and vault root folders.
- Move behaviour inside a vault, into and out of a vault, and for a plaintext file with an active share moved into a vault, as recorded by E-30/c01.
- Tests at the component boundary of 02, 03 and 06 with real PostgreSQL.

### Excluded
- The mechanism decision (E-30/c01) and any table change it needs (follow-ups in E-13, E-14, E-16).
- 08's planner rule itself (E-18/c06, already skips `vault = true`).
- Client encryption and the wrapped key in `client_meta` (E-29, clients).

## Acceptance Criteria

**AC-1: Vault flag on publish (AC-13, AC-14)**
Given a vault root with a child folder and a grandchild folder
When ciphertext declared `image/jpeg` and `video/mp4` is committed directly under the root and two levels deep, through the session path and the batch path, and the same bytes are committed outside the vault
Then every vault entry reaches the effects with `vault = true` and 0 jobs exist for them, while the entries outside have `vault = false` and get their jobs

**AC-2: Size limit at upload start (AC-14)**
Given `max_vault_file_bytes` = 104,857,600
When a vault upload declares 104,857,600 bytes, another declares 104,857,601 bytes, the limit is raised to 200 MiB and 101 MiB is uploaded, and 101 MiB is uploaded outside the vault
Then the first is accepted, the second is refused at upload start with an error that names the limit and at most 2 MiB is sent, the third is accepted, and the fourth is not subject to the vault limit

**AC-3: Share refusal**
Given a vault file, the vault root folder and a normal file beside the vault
When the owner creates a share for each
Then the vault file and the vault root get 422 `target_not_shareable`, the normal file gets 201, and 0 `share_links` rows exist for the vault file

**AC-4: Move inside a vault keeps the file decryptable (AC-60)**
Given a vault file with two revisions
When it is moved to another folder of the same vault
Then its node id, revision ids, `client_meta` and blob references are byte-identical to before the move, and a later revision is still published with `vault = true`

**AC-5: Boundary moves and shared files**
Given the rules recorded by E-30/c01 for moving into and out of a vault and for a plaintext file with an active share moved into a vault
When each case runs
Then the outcome matches the recorded rule, with one test per rule

**AC-6: No call to 20 on these paths (ADR-015)**
Given the server crates
When the crate-graph test runs
Then 02, 03 and 06 have no dependency on 20, and the commit and share paths make no `VaultService` call

## Non-Functional Requirements

**Performance**
- The membership answer adds no per-entry query beyond what the chosen mechanism records (E-30/c01); the publish path keeps its existing bounds (E-13).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-2 | Limit check at 104,857,600 and 104,857,601 bytes | First passes, second refused naming the limit |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Vault with nested folders; real PostgreSQL 18 | Commits inside (two depths, two paths) and outside | Vault entries flagged, 0 jobs; outside entries get jobs | IT-20-5, FT-P-44 step 4 |
| AC-2 | DS-18 files 100 MiB and 101 MiB | Upload at limit, above, raised limit, outside | Accept, refuse at start (<= 2 MiB sent), accept, not subject | IT-20-5, FT-N-14, NFT-SEC-43 |
| AC-3 | Vault file, vault root, normal file | Create shares | 422, 422, 201; 0 rows for vault file | IT-20-4 |
| AC-4 | Vault file with two revisions | Move inside the vault | Revisions and `client_meta` unchanged; still flagged | FT-N-15 step 1b, NFT-SEC-40 step 1b |
| AC-5 | c01 rules | Move in, move out, shared file moved in | Per recorded rule | IT-20-4 gaps |
| AC-6 | Workspace | Crate-graph test | No edge to 20 | ADR-015 |

## Constraints

- The code changes sit in directories owned by 02, 03 and 06 (module-layout); each change is reviewed by that component's owner.
- Nothing of this task is active in v1.0 or v1.1 builds (ADR-016).

## Risks & Mitigation

**Risk 1: A vault file slips through one path**
- *Risk*: the batch path or a staged small upload misses the flag, so a job or a share is created.
- *Mitigation*: AC-1 and AC-3 cover both commit paths and both share targets.

## Runtime Completeness

- Named capability: vault membership on the commit, upload and share paths.
- Production code that must exist: the real membership answer of E-30/c01 read on each path.
- Allowed external stubs: none in the blackbox tests.
- Unacceptable substitutes: a client-supplied "is vault" flag trusted without the server's membership answer, or a test-only flag set by hand.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| The work changes code in 02, 03 and 06 directories although the task sits in E-30; owners may prefer that E-30/c01 AC-4 creates the changes as E-13, E-14, E-16 v1.2 tasks instead | E-13 / E-14 / E-16 / E-30 owners | open | Medium |
| The error code and status of "vault file over the limit" at upload start are not in the `sc_error` table | E-14 / E-02 owners | open | Medium |
| No server rule stops a camera-backup commit under a vault root; AC-14's "camera backup does not write into the vault" relies on the clients (FT-P-45 step 3, NFT-SEC-43 step 4) | E-27 / E-30 owners | open | Low |
