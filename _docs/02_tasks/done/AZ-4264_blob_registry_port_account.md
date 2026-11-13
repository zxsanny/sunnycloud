# Blob registry port takes the account; writer plan map release

**Task**: AZ-4264_blob_registry_port_account
**Name**: Blob registry port with account
**Description**: The `BlobRegistry` port takes the account, 04 implements it as a trait so 12 can wire it for 03, and a blob plan that never reaches `create_file` is released.
**Complexity**: 2 points
**Dependencies**: AZ-3734
**Component**: contracts, 04 blobstore (03 tests)
**Tracker**: AZ-4264
**Epic**: AZ-3627 (E-10)

## Problem

Found during AZ-3734 (2026-11-13). `contracts::BlobRegistry::register_in_tx(tx, blob, state)` has no account, while 04's `sunnycloud_blobstore::BlobRegistry` needs one and implements none of the port's methods as a trait; 03's tests use a per-account adapter, and 12 cannot wire `Arc<dyn BlobRegistry>`. `FileBlobWriter::plan` keeps each plan in memory until `create_file`; a start refused before it (quota, cap, inactive account, lost key race) leaves the entry for good, and `discard` does not clear it.

## Acceptance Criteria

**AC-1: Port with account**
Given the `BlobRegistry` port
When 03 registers a blob in its transaction
Then the port methods take the account and 04's registry implements them as the trait

**AC-2: 03 on the real registry**
Given 03's start tests
When they run
Then they use 04's registry through the port and the test adapter is gone

**AC-3: No plan leak**
Given 100 starts refused before `create_file`
When the writer's plan map is inspected
Then it holds no entry for them

## Scope

Included: the port signatures, 04's trait implementation, the plan release, 03's tests.
Excluded: 12's wiring.
