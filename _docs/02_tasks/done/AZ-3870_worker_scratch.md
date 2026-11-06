# Scratch: job tree lifecycle, start-up sweep, statvfs refusal and safe-open of decoder outputs

**Task**: AZ-3870_worker_scratch
**Name**: Worker Scratch
**Description**: Create and remove each job's scratch tree with the shared group and no world-readable modes, sweep both scratch roots at start-up, refuse inputs that do not fit the mount, and read decoder outputs only as regular single-link files within a size cap, without following links or blocking
**Complexity**: 3 points
**Dependencies**: AZ-3868_worker_crate_skeleton
**Component**: 09_worker_supervisor
**Tracker**: AZ-3870
**Epic**: AZ-3639 (E-20)
**Ref**: E-20/c04

## Problem

The scratch tree holds decrypted user data and is shared with an untrusted decoder. Leftovers after a crash would leak one job's plaintext to the next (AC-71). A compromised decoder can plant a symlink to `/run/secrets/worker_api_token`, a hard link, a FIFO or a sparse giant file as its "thumbnail"; reading it blindly would upload the secret or block the slot (AC-25, R01). An input that does not fit the mount would fill the host disk (R21).

## Outcome

- Each job has its own tree; it is removed on every exit path and everything is swept at start-up.
- Outputs are read only when they are what they claim to be; anything else fails the job as `output_rejected` without reading content.
- Inputs that do not fit the free space are refused before download as `input_too_large`.

## Scope

### Included
- `create(job_id)`: job directory with `in/` and `out/`, group `decoder_gid`, no world-readable modes; `in/source` created for the fetch with mode 0440.
- `remove()` and removal on drop on every exit path; `sweep_all()` of both scratch roots before the first claim.
- Clearing `out/` between the two renders of a `stills` job and before a rerun.
- Safe-open of `out/result.<ext>`: resolve beneath the job directory with no symlinks, open as a path without following links, `fstat` regular file with `st_nlink == 1` and `st_size <= cap`, then reopen through the descriptor read-only and non-blocking; failure -> `output_rejected` (permanent, G-09-3 resolved).
- `statvfs` check: a declared input larger than the free space of the slot's mount -> `input_too_large` (permanent, G-09-2 resolved), before any byte is fetched.

### Excluded
- The fetch itself (E-20/c03); decoder-side `work/` handling (E-19/c07a).

## Acceptance Criteria

**AC-1: Tree and modes**
Given a new job
When its tree is created and the input fetched
Then `in/source` has mode 0440 and group `decoder_gid`, `out/` is writable by that group, and no entry is world-readable

**AC-2: Start-up sweep (IT-09-1, AC-71)**
Given both scratch roots pre-filled with leftover job trees and a stray file
When the worker starts
Then both roots are empty before the first claim request is sent

**AC-3: Removal on every path (AC-71)**
Given jobs ending in success, permanent failure, transient failure, `obsolete` and lease lost
When each ends
Then its job tree no longer exists

**AC-4: Hostile outputs refused (IT-09-12, R01)**
Given `out/result.webp` as a symlink to `/run/secrets/worker_api_token`, a hard link to a file in `in/`, a FIFO, a sparse file above the cap, a directory, and a correct WebP
When each is opened for posting
Then the first five are rejected without reading content (`output_rejected`), the FIFO case returns within 1 s, and only the correct WebP is readable

**AC-5: Swap race (ST-09-3)**
Given a decoder that swaps `out/result.webp` between a regular file and a symlink to the secret 10,000 times while it is opened
When the supervisor reads it each time
Then every read yields the regular file's bytes or a rejection, never the secret's bytes

**AC-6: Input that does not fit (IT-09-7 (e), R21)**
Given a scratch mount whose free space is smaller than a job's declared input
When the job is checked before fetch
Then it fails as `input_too_large` with no byte fetched

## Non-Functional Requirements

**Security**
- Size comes from `fstat`, never from the decoder's report (09 §7).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Modes and group on a temp root | As specified |
| AC-2, AC-3 | Sweep and removal paths | Roots and trees empty |
| AC-4 | Safe-open matrix (symlink, hardlink, FIFO, device, directory, oversize, good) | Rejections without read; FIFO non-blocking |
| AC-5 | Swap race loop | Never the target's bytes |
| AC-6 | Free-space check with a small tmpfs | `input_too_large` before fetch |

## Blackbox Tests

Run in harness H2 (E-20/c11).

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-2 | Pre-filled roots | IT-09-1 sweep part | Empty before first claim | AC-71 |
| AC-4 | FD as compromised decoder | IT-09-12 | Rejections; no secret posted; next job runs | AC-25 |
| AC-6 | Small tmpfs scratch | IT-09-7 (e) | Refused before download | NFT-RES-LIM-04 |
| AC-3 | Kills at each step | IT-09-14 scratch part | Empty scratch | AC-71, NFT-RES-36 |

## Constraints

- Linux only (`openat2`); the supervisor never parses the bytes it posts (the server's sniffer re-validates).

## Risks & Mitigation

**Risk 1: Sweep deletes another pair's data**
- *Risk*: two supervisors share a root.
- *Mitigation*: each supervisor/decoder pair owns its scratch volumes exclusively (09 §2, containerization).

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| Who creates `work/` inside the job tree (supervisor at `create` or the decoder) is not stated; 10 §3 says the decoder writes and removes it | E-19 and E-20 owners | open | Low |
