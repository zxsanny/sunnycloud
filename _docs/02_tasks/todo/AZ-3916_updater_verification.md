# Updater verification and staging, and the offline signing tool

**Task**: AZ-3916_updater_verification
**Name**: Updater
**Description**: Check the fixed GitHub Releases source for a signed update manifest, accept it only when it verifies against one of the two pinned Ed25519 keys, has not expired, has a higher serial and a higher version, then download this OS's payload, check size and SHA-256, stage it for the next start and show it in status; plus the offline tool that signs the manifest
**Complexity**: 5 points
**Dependencies**: AZ-3887_update_manifest_adr_spike, AZ-3891_local_index_store, AZ-3763_sc_config, AZ-3765_sc_clock; notes: E-31/c14 (test epic E-31, decomposed at greenfield Step 9)
**Component**: 14 desktop_daemon
**Tracker**: AZ-3916
**Epic**: AZ-3625 (E-24)
**Ref**: E-24/c18

## Dependencies

### Document Dependencies
- `_docs/02_document/adr/017_release-signing-and-offline-updater-keys.md` — manifest layout and signing-tool decision as confirmed by E-24/c04.
- `_docs/02_document/contracts/shared_config/sc_config.md` — release-fixed values (update source, two public keys) that env and settings cannot override (E-01/c01).
- `_docs/02_document/contracts/shared_clock/sc_clock.md` — clock for `expires` (E-03/c01).

## Problem

An updater is the easiest way to own every desktop: a stolen CI secret, a replayed old manifest or a redirected download would install attacker code (R17, AC-49). The owner decided (S4-3, ADR-017): manifests are signed offline, never by an automatic CI secret; the app pins two keys (active and recovery); the manifest carries `version`, per-OS SHA-256, a monotonic `serial` and `expires`; the source is fixed at build time and is never the sync server. All eight bad-build cases must be rejected (14 §14).

## Outcome

- A check every `desktop.update_check_interval` (24 h) against the compiled-in source only; env variables and the settings file cannot change the source or the keys.
- Verification order: signature against either pinned key (a recovery-key manifest is accepted); `expires` not passed; `serial` greater than the last accepted one stored in `meta`; `version` strictly greater than the installed one; the asset for this OS resolved by name against the fixed source (no URL from the manifest, no redirect followed); `size` and SHA-256 match the signed entry.
- On success: payload staged, serial stored, `DaemonStatus.update_available` set, INFO "update staged"; the update applies at the next start. On any failure: nothing staged, no restart, a WARN naming the failed check.
- The daemon never fetches keys and never contacts the sync server for updates.
- The offline signing tool (form and location as decided by E-24/c04): reads the manifest and a private key file supplied by the operator at signing time, writes the detached signature over the exact bytes, refuses a key from an environment variable.
- The `desktop.update_check_interval` row added to helper 01's key table.

## Scope

### Included
- Fetch, verify, download, stage, status flag; the signing tool; test-key builds for the tests.

### Excluded
- The release workflow and the manual signing step (CI epic, owner); applying the staged package mechanics beyond "at next start" (flagged); Windows code signing (R19).

## Acceptance Criteria

**AC-1: Good builds accepted**
Given `mock-update-server` with an active-key manifest (v1.0.1, serial 7, expires in 30 days, matching SHA-256) and, separately, a recovery-key manifest
When the updater checks
Then each is accepted: `update_available` is set, the payload is staged and the serial is stored in `meta`

**AC-2: Eight bad builds rejected (AC-49)**
Given DS-31's bad builds: unsigned; signed by an unpinned key; expired; replayed serial after 7 was accepted; same version; lower version; altered payload (digest mismatch); truncated payload
When the updater checks each
Then none is staged, the daemon does not restart, and the WARN line names the failed check, 8 of 8 (IT-14-17)

**AC-3: Fixed source, no overrides**
Given `SUNNYCLOUD_*` variables and a settings file that try to change the source or the keys
When the updater runs
Then they change nothing, and the only host contacted is the fixed update host (0 requests to the sync server)

**AC-4: Downgrade and key confusion (ST-14-3)**
Given a replayed older valid manifest, a recovery-key manifest with a lower version, a manifest that lists a key inside itself, and a manifest URL redirected to another host
When each is served
Then 0 payloads are staged, the stored serial is unchanged, no key is fetched and the redirect is not followed

**AC-5: Signing tool**
Given a manifest and a test private key file
When the tool signs it
Then the daemon built with the matching test public key accepts the pair, and the tool refuses to run when the key is given through an environment variable

## Non-Functional Requirements

**Security**
- Unit coverage target 90% on verification (E-24 Testing Strategy).
- No updater private key in the repository, CI or any test image except test keys.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Verification with active and recovery test keys | Accepted |
| AC-2 | Each of the eight failure checks in isolation | Rejected with the named check |
| AC-4 | Lower-version recovery manifest; key in manifest | Rejected; no key use |
| AC-5 | Tool sign and daemon verify; env-supplied key | Pair accepted; tool refuses |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1..AC-3 | `mock-update-server`, DS-31 (8 bad, 2 good), test-key daemon build | IT-14-17 | Good staged; 8 of 8 bad rejected; overrides ignored; only update host contacted | AC-49, NFT-SEC-33 |
| AC-4 | `mock-update-server` replay, downgrade, key-in-manifest, redirect | ST-14-3 | 0 staged; serial unchanged; redirect not followed | AC-49, NFT-SEC-33 |

## Runtime Completeness

- Named capability: Ed25519 verification of an offline-signed manifest with two pinned keys, and the offline signing tool (ADR-017).
- Production code that must exist: real signature, expiry, serial, version, size and digest checks; a real signing tool.
- Allowed external stubs: `mock-update-server` (E-31/c14) as the release source in tests, test key pairs.
- Unacceptable substitutes: TLS or the GitHub API as the trust anchor instead of the signature; keys read from configuration.

## Constraints

- Update source and public keys are build-time constants (release-fixed values of helper 01), not config keys.
- IT tier: the updater runs in-process against `mock-update-server`.

## Risks & Mitigation

**Risk 1: Clock set back to accept an expired manifest**
- *Risk*: a user or attacker moves the clock.
- *Mitigation*: the serial check still refuses any manifest at or below the last accepted serial.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| How a staged payload is applied "on restart" (a deb needs root; MSI per-user or per-machine) is not specified | E-24 / owner | open | Medium |
| Building the signing tool here is a proposal of E-24/c04 (no epic row owns it) | E-24 | open | Low |
| Layout fields depend on E-24/c04's owner confirmation | Owner | open | Low |
