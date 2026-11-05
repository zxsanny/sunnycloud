# Blob file format and hand-built STREAM nonce

**Task**: AZ-3724_blob_file_format
**Name**: Blob file format and nonce
**Description**: The on-disk layout, the per-chunk XChaCha20-Poly1305 sealing with a hand-built STREAM nonce, and the range-to-chunk mapping, proven byte for byte against the pinned `aead-stream` oracle
**Complexity**: 5 points
**Dependencies**: AZ-3723_blobstore_skeleton, AZ-3766_sc_ids
**Component**: 04 blobstore
**Tracker**: AZ-3724
**Epic**: AZ-3627 (E-10)
**Ref**: E-10/c04

## Problem

Random-access reads and out-of-order block writes need each chunk at a fixed offset and sealable on its own. The sequential STREAM encryptor of `aead-stream` cannot jump to chunk `i`, so chunks are sealed with the raw cipher and a nonce built in the STREAM format by hand. A mistake in that nonce silently breaks confidentiality (R08); only known-answer tests against the oracle catch it.

## Outcome

- Blob files live at `blobs/<id[0:2]>/<id[2:4]>/<id>` under the blob root, built only from the server-generated v4 blob id.
- A fixed 64-byte header (magic, format version, STREAM nonce prefix, zero pad), then 64 KiB plaintext chunks each stored with a 16-byte tag; chunk `i` starts at `64 + i * (65536 + 16)`.
- Each chunk is sealed with XChaCha20-Poly1305, nonce = prefix, 32-bit chunk counter, last-chunk flag; the last chunk of the declared size carries the flag; AAD = blob id.
- A pure mapping from a byte range to the chunk range, and from a declared size to the file length.
- A per-blob random 256-bit data key type that is zeroized on drop and prints a redaction in `Debug`.

## Scope

### Included
- Path construction, header encode and decode, chunk seal and open, final-flag rule, counter limit, range and length arithmetic, data-key type.
- Known-answer vectors checked against `aead-stream =0.6.0` (dev-dependency only).

### Excluded
- Wrapping data keys with the master key (E-10/c05), file I/O, durability and preallocation (E-10/c06, c07), reading streams (E-10/c08).
- The block-rewrite rule (E-10/c01).

## Acceptance Criteria

**AC-1: Nonce equals the oracle**
Given fixed prefix, key and AAD = blob id
When chunks 0, 1 and n-1 (final), a single-chunk file and an empty file are sealed by the component
Then each sealed output equals the `aead-stream =0.6.0` output byte for byte

**AC-2: Counter limit refused**
Given a chunk index at the 32-bit counter limit (2^32 - 1)
When it is sealed
Then a typed error is returned and the counter never wraps

**AC-3: AAD and position bind each chunk**
Given a chunk sealed for blob B1 at index k
When it is opened as blob B2, or as index k+1, or with the final flag flipped
Then opening fails with `Corrupt`

**AC-4: Layout arithmetic**
Given declared sizes 0, 1, 65,535, 65,536, 65,537, 2 MiB and 5 MiB + 7
When the file length and chunk offsets are computed
Then file length = 64 + n_chunks * 16 + size, chunk `i` starts at `64 + i * 65,552`, and any 4 KiB range maps to at most 2 chunks

**AC-5: Unsatisfiable ranges classified**
Given a 3 MiB declared size
When the ranges `start == size`, `end > size` and `start > end` are mapped
Then each is classified as not satisfiable without touching any file

**AC-6: Paths only from blob ids**
Given the strings `../../etc/passwd`, `00/../..`, a non-UUID, and a UUID with uppercase or braces
When a blob path is requested for each
Then no path is produced outside the blob root: construction fails with a typed error or the id cannot match any file (full ST-04-4 in E-10/c14)

## Non-Functional Requirements

**Performance**
- Sealing and opening must allow the PT-04-2 targets (write >= 300 MB/s, read >= 500 MB/s on REF-SRV); measured in E-10/c14.

**Reliability**
- The format version in the header lets a later format be read side by side; this task defines version 1 only.

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-1 | Known-answer vectors vs `aead-stream` | Byte-equal for every vector |
| AC-2 | Seal at counter 2^32 - 1 | Typed error, no wrap |
| AC-3 | Open with other blob id, other index, flipped final flag | `Corrupt` each time |
| AC-4 | Length and offset table for the size corpus | Matches the formula |
| AC-5 | Range classification | Not satisfiable for the three cases |
| AC-6 | Path construction from hostile strings | Typed failure or no path outside the root |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1 | Oracle vectors generated at build by the pinned crate | IT-04-2 | Each vector equal; AAD change makes open fail | |
| AC-2 | Counter at limit | IT-04-2 refusal case | Typed error | |
| AC-4 | Chunk-size corpus | Layout part of IT-04-1 (asserted on real files in E-10/c06) | Formula holds | |

## Runtime Completeness

- **Capability**: XChaCha20-Poly1305 per chunk with a STREAM-format nonce (`chacha20poly1305 =0.11.0`).
- **Production code that must exist**: real sealing and opening of every chunk, header and final-flag handling, counter-limit refusal.
- **Allowed external stubs**: none; `aead-stream =0.6.0` is a test oracle only.
- **Unacceptable substitutes**: a plaintext or identity "cipher" in any build or feature, a fixed or reused nonce, a nonce built from anything other than prefix, counter and final flag, a build flag that turns encryption off.

## Constraints

- Pins: `chacha20poly1305 =0.11.0`, `aead-stream =0.6.0` (dev-dependency), MSRV 1.88.
- Chunk 64 KiB, tag 16 bytes, header 64 bytes; AAD = blob id. No user input ever enters a path.

## Risks & Mitigation

**Risk 1: Oracle agrees with a wrong reading of the format**
- *Risk*: both the component and the test share a misunderstanding.
- *Mitigation*: vectors come from the pinned crate's own encryptor, not from hand-computed bytes; the external review (E-10/c01 scope) checks the construction before v1.0.

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| External review of the construction is a v1.0 release gate (scope written in E-10/c01) | Owner | open | Medium |
