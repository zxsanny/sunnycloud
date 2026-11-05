# `key-check`, `admin keycheck-init`, `admin rewrap`

**Task**: AZ-3797_key_commands
**Name**: Master key commands
**Description**: Verify the master key, initialize the key check after a sample unwrap, and rotate the key by rewrapping data keys in a loop until none remain, never printing key material
**Complexity**: 3 points
**Dependencies**: AZ-3788_cli_skeleton, AZ-3789_composition_root, AZ-3725_master_keyring, AZ-3746_blob_maintenance_ports
**Component**: 12 operator_cli (`sunnycloud` binary, composition root)
**Tracker**: AZ-3797
**Epic**: AZ-3647 (E-22)
**Ref**: E-22/c11

## Dependencies

### Document Dependencies
- `_docs/02_document/contracts/contracts/part3_blob_share_media_vault.md` — `MasterKeyring` (`load_and_verify`, `init_key_check`, `rewrap_batch`).
- `_docs/02_document/contracts/12_operator_cli/operator_command_surface.md` — command rows (E-22/c02).

## Problem

Losing or mixing up the master key makes data unreadable (AC-43, AC-72). The operator needs a quick check, a safe way to create the key check on a restored volume (refusing a wrong key before anything is written), and a rotation that rewrites only wrapped keys and survives interruption. Key bytes must never reach a terminal, a log or `ps`.

## Outcome

- `key-check`: one `MasterKeyring::load_and_verify` call; exit 0 on a match; `FileMissing`, `WrongLength`, `KeyCheckMismatch`, `KeyCheckAbsentButBlobsExist` exit 2 with that code.
- `admin keycheck-init`: one `init_key_check` call; `KeyCheckSampleFailed` exits 2; success exits 0.
- `admin rewrap --new-key-file <path> [--yes]`: without `--yes` on a non-TTY exit 3 with no call; with it `rewrap_batch` is called repeatedly until it reports 0 rows; a progress line on stderr every `cli_progress_interval`; a closing message that the old key must be kept; exit 0. SIGINT stops the loop between batches (E-22/c02 cancellation).
- The new key is given as a file path; no key bytes on the command line or in any output.

Narrowed against the plan: the wrong-key refusal of `serve` and `key-check` end to end and the `keycheck-init` caller are AZ-3666 (E-22/p13) AC-72 and AC-4. This task owns the command side.

## Scope

### Included
- The three commands: parsing, guard, the business call or loop, progress, output and exit mapping.

### Excluded
- Key loading, the key check, sampling and rewrap (04, E-10/c05, E-10/c13), the `serve` key refusal (E-22/c05).

## Acceptance Criteria

**AC-1: Key check outcomes (IT-12-12, FT-N-33)**
Given recording `load_and_verify` answers: match, `FileMissing`, `WrongLength`, `KeyCheckMismatch`, `KeyCheckAbsentButBlobsExist`
When `key-check` runs for each, in text and in `--json`
Then the match exits 0 and each failure exits 2 with its code in the `--json` object

**AC-2: Key check init (IT-12-12)**
Given recording `init_key_check` answers success and `KeyCheckSampleFailed`
When `admin keycheck-init` runs for each
Then success exits 0 and the failure exits 2 with `KeyCheckSampleFailed`

**AC-3: Rewrap needs confirmation (IT-12-13, ST-12-2)**
Given stdin that is not a TTY
When `admin rewrap --new-key-file k2` runs without `--yes`
Then it exits 3 and no `rewrap_batch` call is made

**AC-4: Rewrap loops to zero (IT-12-13, FT-P-41)**
Given a recording `rewrap_batch` that reports 200, 200, 100, then 0 rows, each call taking longer than `cli_progress_interval` (500 ms in tests)
When `admin rewrap --new-key-file k2 --yes` runs
Then `rewrap_batch` is called four times, a progress line appears on stderr at least every 500 ms, the closing message says the old key must be kept, and the exit code is 0

**AC-5: Rewrap stops cleanly on Ctrl-C (IT-12-19)**
Given a rewrap loop in progress
When SIGINT arrives
Then no new batch starts after the running one returns, and the process ends within 5 s

**AC-6: No key material anywhere (ST-12-1, IT-12-12)**
Given a known master key and a known new key
When the three commands run with `log = json` and their outputs and `ps` are searched for the keys in hex and base64
Then 0 occurrences are found

## Non-Functional Requirements

**Security**
- `--new-key-file` takes a path; key bytes are never an argument.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1, AC-2 | Mapping of each keyring outcome | Exit 0 / 2 with the code |
| AC-3 | Guard | No call on non-TTY |
| AC-4 | Loop and progress timing with a manual clock | Calls until 0; progress each interval |
| AC-5 | Cancellation between batches | No new batch after SIGINT |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | Harness, recording `MasterKeyring` | `key-check`, `keycheck-init` outcomes (IT-12-12) | Exit 0 / 2 with codes | AC-43, AC-72 |
| AC-3, AC-4 | Harness, stdin `/dev/null`; scripted batches | `rewrap` without and with `--yes` (IT-12-13) | Exit 3 no call; four calls, progress, exit 0 | FT-P-41 |
| AC-5 | Harness, blocking batch | SIGINT (IT-12-19) | Stops within 5 s | |
| AC-6 | Known keys, `log = json` | All outputs and `ps` | 0 key occurrences | ST-12-1 |

The owner-effect parts of IT-12-12 and IT-12-13 (`.keycheck` not created on a wrong key, blob files unchanged byte for byte, downloads after restart with k2, resumable rewrap after a kill) are proven by E-10/c05, E-10/c13, AZ-3666 (E-22/p13) and FT-P-41 in E-31.

## Constraints

- No crypto in 12: the CLI passes paths and options; 04 reads keys.

## Risks & Mitigation

**Risk 1: Old key discarded too early**
- *Risk*: the operator deletes the old key before every row is rewrapped.
- *Mitigation*: the loop runs to 0 rows and the closing message says to keep the old key.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| `rewrap_batch(from_id, to_id, n)` (E-10/c13) works on key ids of the configured ring, while the command takes `--new-key-file`; how the file becomes a ring id (config change first, or the command registering it) is not stated | E-10 / E-22 owners | open | Medium |
| The batch size `n` of the rewrap loop is not stated | E-10 / E-22 owners | open | Low |
