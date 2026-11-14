# PublicShareService::authorize_derived and the PublicShareService impl

**Task**: AZ-4310_share_authorize_derived
**Name**: Share derived authorization
**Description**: Add `authorize_derived` to 06 and implement `contracts::PublicShareService` for `PgShareService`, so the composition root can hand it to 11
**Complexity**: 3 points
**Dependencies**: AZ-3700_share_authorize_content, AZ-3699_share_password_access_grant, AZ-3701_share_abuse_limits
**Component**: 06 sharing
**Tracker**: AZ-4310
**Epic**: AZ-3619 (E-16)

## Problem

AZ-3789 found that `ApiServices` cannot be assembled: `PgShareService` has inherent `open`, `authorize_content` and `verify_password`, but `authorize_derived` (share thumbnail and preview, named by 11 and 08; contracts part 3 flag: its inputs mirror `authorize_content`) is missing, so there is no `impl PublicShareService`.

## Outcome

- `authorize_derived(token, grant?, ip)` returns `ShareDerived{account_id, file}` (part 3 1.1.0) with the same lookup, liveness, grant, abuse-limit and constant-work rules as `authorize_content`; no `share_accessed` audit (a thumbnail is not a download start).
- `impl PublicShareService for PgShareService` for all four methods; the inherent methods become the trait's (one path).

## Acceptance Criteria

**AC-1**: Given a live link on a JPEG, When `authorize_derived` is called, Then it returns the owning account and the live file; a revoked, expired, unknown or malformed token, a deleted file and a folder answer one `NotFound`.
**AC-2**: Given a password link, When `authorize_derived` is called without a grant and with a valid grant, Then it answers `PasswordRequired`, then the file.
**AC-3**: Given the statement log, When the NotFound cases of `authorize_derived` are compared, Then they run the same statements (IT-06-9 shape), and an unknown token counts on the per-IP invalid-token counter.
**AC-4**: Given the four methods, When called through `dyn PublicShareService`, Then each answers as its inherent form did (the existing suites run through the trait).
