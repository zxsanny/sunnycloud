# AdminService trait implementation with AdminError::Forbidden

**Task**: AZ-4309_admin_service_impl
**Name**: AdminService implementation
**Description**: Implement `contracts::AdminService` for 07's `AdminServiceImpl` with the planned `AdminError::Forbidden` (CC-3), so the composition root can hand an `AdminService` to 11
**Complexity**: 3 points
**Dependencies**: AZ-3817_admin_users_quota, AZ-3818_last_admin_guard, AZ-3789_composition_root (partly done)
**Component**: 07 admin
**Tracker**: AZ-4309
**Epic**: AZ-3624 (E-17)

## Problem

AZ-3789 found that `ApiServices` cannot be assembled: 07's `AdminServiceImpl` has only inherent methods returning `AdminCallError`, because `AdminError` has no `Forbidden`, and its `health` returns `Result<HealthReport, _>` while the port returns `HealthReport`. CC-3 (E-17 epic, Dependencies) already plans `AdminError::Forbidden` on all four `AdminService` operations (non-admin 403, G-S6).

## Outcome

- `contracts::AdminError::Forbidden` (additive; contracts part 1 version and change-log row).
- `impl AdminService for AdminServiceImpl` matching the port exactly; the inherent methods and `AdminCallError` removed or reduced to what the trait needs (one canonical path).
- 11's port-error mapping (403) and its doubles updated; 07's tests call the trait.

## Acceptance Criteria

**AC-1**: Given a user session and an admin device token, When each calls the four `AdminService` operations through the trait, Then each answers `AdminError::Forbidden`, nothing changes and no audit row is written (ST-07-1 through the trait).
**AC-2**: Given an admin session, When `health` is called through the trait, Then it returns the aggregated `HealthReport` (a failing contributor is reported inside the report, not as an error).
**AC-3**: Given 11's error table, When `AdminError::Forbidden` is mapped, Then it is 403 `forbidden`.
