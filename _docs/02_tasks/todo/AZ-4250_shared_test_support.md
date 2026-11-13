# Shared test support

**Task**: AZ-4250_shared_test_support
**Name**: Shared test support
**Description**: One sqlx statement capture and one boundary-backed `AccountGuard` test double, shared by the component test suites that copy them today.
**Complexity**: 2 points
**Dependencies**: none
**Component**: shared test helpers (`sc_testdb` or a sibling test-support crate)
**Tracker**: AZ-4250
**Epic**: AZ-3623 (E-01)

## Problem

The cumulative review of batches 48 to 51 (2026-11-13) found the statement capture copied five times (`statements.rs` in the 01, 02, 05 feed, 05 via_tree and 08 test suites) and the `AccountGuard` test double three times (02, 05, 08). A change to sqlx's statement events or to the guard's rule has to be repeated in every copy.

## Acceptance Criteria

**AC-1: One statement capture**
Given the five suites
When they count or order statements
Then they use one capture from a shared test helper crate, and their own copies are deleted

**AC-2: One guard double**
Given the 02, 05 and 08 suites
When they wire `PgTreeService`
Then they use one `AccountGuard` test double from the same helper crate

**AC-3: Same assertions**
Given the moved helpers
When every affected suite runs
Then it passes and asserts what it asserted before

**AC-4: One zeroize declaration**
Given the crates that depend on `zeroize`
When their manifests are read
Then they declare it the same way

## Scope

Included: the helper crate (dev-dependency only), the five suites, the manifests.
Excluded: product code; new assertions.
