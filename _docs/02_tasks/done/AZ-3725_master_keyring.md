# Master keyring: load and verify, key check, data-key wrap, keycheck-init

**Task**: AZ-3725_master_keyring
**Name**: Master keyring
**Description**: Load the master key ring from secret files, verify it against the `.keycheck` file before anything is served, wrap and unwrap per-blob data keys, and initialise the key check on a restored volume only after a sample proves the key
**Complexity**: 5 points
**Dependencies**: AZ-3723_blobstore_skeleton, AZ-3724_blob_file_format, AZ-3784_sc_audit
**Component**: 04 blobstore
**Tracker**: AZ-3725
**Epic**: AZ-3627 (E-10)
**Ref**: E-10/c05

## Problem

Every data key is wrapped by an operator-supplied master key (ADR-006). A wrong or missing key must stop the server before it serves anything, with a message the operator understands (AC-43, AC-72). After a volume restore the `.keycheck` file can be missing while blobs exist; recreating it with an unproven key would make a wrong key the installation's check value.

## Outcome

- `MasterKeyring::load_and_verify` builds the ring (`master_key_file` as key id 1, optional `master_key_ring` of `id=path` entries, `master_key_current` defaulting to the highest id), requires exactly 32 bytes per key, and checks each key id against its sealed constant in `.keycheck`.
- Failures `FileMissing`, `WrongLength`, `KeyCheckMismatch`, `KeyCheckAbsentButBlobsExist`, each with the operator text "data is unreadable without the master key".
- On an empty install (empty `blobs` table and empty volume) `.keycheck` is created at first start.
- Per-blob data keys are wrapped with XChaCha20-Poly1305 under the current key id with AAD = blob id and unwrapped with one AEAD call per request (no key cache); an id the ring lacks is `KeyUnavailable`.
- `init_key_check` unwraps the wrapped keys of a random sample of up to 100 committed blobs with the candidate key; any failure returns `KeyCheckSampleFailed{blob_id}` and writes nothing; success writes `.keycheck` and one `admin_action` audit event; an empty `blobs` table skips the sample.

## Scope

### Included
- Ring loading from secret files, length check, key check read and verify, first-start creation, wrap and unwrap, `init_key_check`, INFO log "startup key check passed", ERROR log on unwrap failure.

### Excluded
- `rewrap_batch` (E-10/c13). The `MasterKeyring` port is complete only after E-10/c13; no method is a placeholder in between.
- The `serve` refusal before the listener binds and the `key-check`/`admin keycheck-init` commands (E-22/c05, E-22/c11).

## Acceptance Criteria

**AC-1: Start refusals are typed**
Given in turn a missing key file, a 31-byte and a 33-byte key, another key while `.keycheck` exists, and the correct key with `.keycheck` deleted while blobs exist
When `load_and_verify` runs
Then it returns `FileMissing`, `WrongLength`, `WrongLength`, `KeyCheckMismatch` and `KeyCheckAbsentButBlobsExist`, each with the operator text, and no keyring is produced

**AC-2: Key check created only on an empty install**
Given the correct 32-byte key
When `load_and_verify` runs on an empty `blobs` table with an empty volume, and again where either holds data without `.keycheck`
Then the first creates `.keycheck`, and the second creates nothing and returns `KeyCheckAbsentButBlobsExist`

**AC-3: keycheck-init proves the key first**
Given a volume without `.keycheck`
When `init_key_check` runs with a wrong key and at least one committed blob, with the right key, and with an empty `blobs` table
Then the wrong key returns `KeyCheckSampleFailed{blob_id}` and `.keycheck` is not written; the right key writes it and one `admin_action` audit event; the empty table skips the sample and writes it

**AC-4: Wrap binds the blob and the key id**
Given a data key wrapped for blob B1 under key id 1
When it is unwrapped as B2, or with a ring that lacks id 1
Then the first fails and the second returns `KeyUnavailable`; no data key is produced

**AC-5: Ring rotation**
Given a ring `1=k1,2=k2` with `master_key_current` unset
When new data keys are wrapped and old ones unwrapped
Then new wraps use id 2 and keys wrapped under id 1 still unwrap

**AC-72: A wrong master key fails closed (component part, R5)**
Given a blob volume with a stored key check
When `load_and_verify` runs with a different master key
Then it returns `KeyCheckMismatch` with the operator text and no keyring exists, so no blob can be opened in that process (the `serve` exit before any listener and `key-check` exit 2 are checked end to end by E-22/p13)

## Non-Functional Requirements

**Performance**
- One AEAD unwrap per blob open; no key cache (keeps the 4 KiB range budget of PT-04-1).

**Reliability**
- Key material buffers zeroized on drop; `Debug` prints a redaction.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Key length check (31, 32, 33 bytes) | Only 32 accepted |
| AC-4 | Wrap and unwrap with other blob id | Unwrap fails |
| AC-5 | Ring parsing and current-id default | Highest id is current |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Key file variants, `.keycheck` present or deleted | IT-04-13 startup cases | Typed errors with the operator text | FT-N-12, NFT-SEC-23 |
| AC-2 | Empty and non-empty install | IT-04-13 empty-install case | `.keycheck` only on empty install | |
| AC-3 | Volume without `.keycheck`, wrong and right key | IT-04-13 `init_key_check` cases | Refusal writes nothing; success audited | |
| AC-4 | B1/B2 wraps, ring without id | Cross-wrap and missing id | No key; `KeyUnavailable` (full ST-04-2/3 in E-10/c14) | |
| AC-72 | Stored key check, other key | `load_and_verify` | `KeyCheckMismatch`, no keyring | FT-N-43 (worker route, E-18) |

## Runtime Completeness

- **Capability**: XChaCha20-Poly1305 wrap of per-blob data keys and the sealed key-check constant.
- **Production code that must exist**: real authenticated wrap and unwrap, real key check on every start.
- **Allowed external stubs**: none.
- **Unacceptable substitutes**: a setting or build flag that skips the key check, a default or generated master key when the file is missing, a key cache that serves an unwrap without the AEAD call.

## Constraints

- Keys only from secret files; the master key file is mounted in the API container only and never lies on the blob volume.
- The sample read of `init_key_check` runs in system scope and holds no transaction across file I/O.
- Key bytes never appear in logs, metrics or error text (ST-04-7 in E-10/c14).

## Risks & Mitigation

**Risk 1: Wrong key becomes the check value**
- *Risk*: an operator runs keycheck-init with the wrong key on a restored volume.
- *Mitigation*: the sample unwrap must pass first (AC-3); with blobs present nothing is written on failure.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| R22: one master key; rewrap does not change data keys after a key leak | Epic owner | accepted-risk | Medium |
