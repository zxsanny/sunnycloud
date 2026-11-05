# Share link pipeline (F11)

**Task**: AZ-3662_pipeline_share_links
**Name**: Share link pipeline
**Description**: Owners create and revoke links, and visitors open them with optional passwords and get content served by the real tree, transfer and blobstore, end to end against the real components
**Complexity**: 3 points
**Dependencies**: AZ-3742_test_harness_skeleton, AZ-3784_sc_audit, AZ-3785_sc_ratelimit, AZ-3801_crate_skeleton_accept_loop, AZ-3808_csrf_origin_check, AZ-3812_content_header_mapper, AZ-3830_share_media_handlers, AZ-3789_composition_root, AZ-3791_serve_start_stop, AZ-3697_share_create_list_revoke, AZ-3698_share_lookup_open_uniform, AZ-3699_share_password_access_grant, AZ-3700_share_authorize_content, AZ-3701_share_abuse_limits, AZ-3707_tree_reader, AZ-3708_tree_folder_mutations, AZ-3755_transfer_download, AZ-3728_blob_reader, AZ-3943_share_dialog_public_page, AZ-3655_pipeline_large_upload, AZ-3659_pipeline_tree_conflicts, AZ-3652_initial_structure
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3662
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/p10

## Problem

A share link joins 06 (tokens, passwords, grants, cache), 02 (`TreeReader`), 03 (`DownloadService`), 04 (`BlobReader`), the shared abuse limiter and the audit log, behind both the owner routes and the public routes of 11. 06 reaches 02 only through the port 12 binds. No planned task proves that a real link follows its file through renames and deletes, or that the public route serves content with the F6 rules.

## Outcome

- An owner creates a link through the real `serve` process; a visitor without an account opens it and downloads the file; revocation and deletion close it within 5 s.
- Every component operation of flow F11 has a production caller.
- At least one integration test runs F11 through the real binary over HTTP.

## Scope

### Included
- Flow order and seams (`system-flows.md` F11 steps 1-6):
  1. Owner route (session only, CSRF) → `ShareService::create` (256-bit token, SHA-256 stored, expiry, optional Argon2id password) → URL once; `TreeReader::get_node` checks a file target.
  2. Public route → `PublicShareService::open` (constant-work lookup) → `password_required` or content grant; the shared `AbuseLimiter` counts invalid tokens and wrong passwords.
  3. `PublicShareService::verify_password` → access grant signed with the share grant key.
  4. Public content route → `PublicShareService::authorize_content` (1 s cache, current revision via `TreeReader::resolve_live_file`) → `DownloadService` → `BlobReader` → serving rules (11).
  5. `ShareService::revoke` → next open answers the uniform 404.
  6. Create, revoke and access audit events with ids only.
- The end-to-end integration test of the steps above. The web dialog and public page are E-26/c13 (its Playwright tests run against the real server).

### Excluded
- Folder shares (not in v1.0), the vault-file share refusal (E-30/p01), timing measurement on REF-SRV (E-16/c10).

## Acceptance Criteria

AC-1..AC-5 are local to this task. AC-77 keeps its number from `acceptance_criteria.md`.

### Wiring and flow criteria

**AC-1: Create and open without an account**
Given an owner with a browser session and a committed file
When the owner creates a link and a visitor with no credentials opens it and downloads the file
Then the URL is shown once, only the token hash is stored, the visitor gets the file bytes with the F6 serving rules, and the audit log has create and access events with ids only

**AC-2: Password-protected link**
Given a link with a password
When a visitor opens it, sends a wrong password 10 times within 10 minutes, then the right password
Then the open answers `password_required`, the 11th wrong attempt gets 429, and once the limit window passes the right password yields a grant that downloads the file

**AC-3: Revocation and uniform not-found**
Given a valid link, an expired link (test clock), a revoked link and a random token
When each is opened
Then the revoked link fails within 5 s of revocation, and all three failing cases return identical status, body and headers

**AC-4: Scope and target rules**
Given a device token, a folder and a cookie request without `Origin`
When a share is created with the device token, for the folder, and without `Origin`
Then the requests get 403, a typed refusal and 403

**AC-5: Every flow operation has a production caller**
Given the source tree
When the wiring check searches non-test code
Then it finds production callers of `ShareService::{create, list, revoke}`, `PublicShareService::{open, verify_password, authorize_content}`, `DownloadService` from the public content route, and the single shared `AbuseLimiter`

### Interaction criteria (Step 1.8)

**AC-77: A link follows its file and dies with it** (R10)
Given a share link for file x
When x is renamed, moved to another folder, edited in place (new revision on the same node), and finally deleted
Then after each rename, move and edit the link serves the current content, and after the delete it answers the uniform 404 within 5 s

## Non-Functional Requirements

**Performance**
- A link opens within 2 s (AC-12); median timing of missing, expired and revoked within 1 ms (measured on REF-SRV in E-16/c10).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-5 | Static caller check over non-test sources | Each named operation has a production caller |

## Blackbox Tests

Integration through the real binaries; the system scenarios stay in E-31.

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Owner session, committed file | Create, open as visitor, download | URL once, hash stored, content with F6 rules, audit ids | AC-12, AC-47 |
| AC-2 | Password link | Wrong x10, right | `password_required`, 429, then grant | AC-20 |
| AC-3 | Valid, expired, revoked, random tokens | Open | Revoked fails within 5 s; identical 404s | |
| AC-4 | Device token, folder, no Origin | Create | 403, typed refusal, 403 | AC-57, AC-58 |
| AC-77 | Link to x | Rename, move, edit, delete | Current content each time; 404 within 5 s after delete | FT-N-44 |

## Constraints

- Owner: component 12 (`src/server/`): bindings for this flow and its end-to-end tests among 12's integration tests. No SQL or business rule in 12.
- Tests run the real `sunnycloud` binary against PostgreSQL 18 and a real blob volume over HTTP; no sunnycloud component is faked.
- The component-level cache rule stays in E-16/c06; AC-77 checks the outcome across 02 and 06 through the routes.
- Downstream: E-31/c28 (FT-P-36..38), E-31/c31 (FT-N-09..11), E-31/c33 (FT-N-44) and E-31/c35 should depend on this task.

## Risks & Mitigation

**Risk 1: Stale cache serves deleted content**
- *Risk*: the 1 s authorization cache is keyed so that a delete is not seen for longer than 5 s.
- *Mitigation*: AC-77 bounds the delete case at 5 s.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| G-06-2 (right password while the link counter is limited) and G-06-3 (expiry boundary) are still a spike (E-16/c01); AC-2's last step follows its answer | E-16 | open | Low |
