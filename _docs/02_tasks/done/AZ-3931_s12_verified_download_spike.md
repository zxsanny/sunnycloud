# Spike S12: verified download up to 2 GiB in Firefox and Safari with a bounded heap

**Task**: AZ-3931_s12_verified_download_spike
**Name**: S12 Firefox and Safari verified-download spike
**Description**: Measure candidate mechanisms that download and hash-verify files up to 2 GiB in Firefox and Safari without holding the file in the JavaScript heap, and record the chosen mechanism or the gate-S12 decision needed
**Complexity**: 3 points
**Dependencies**: AZ-3930_vite_wasm_spike, AZ-3652_initial_structure
**Component**: 16 web_app (Svelte SPA)
**Tracker**: AZ-3931
**Epic**: AZ-3632 (E-26)
**Ref**: E-26/c02

## Problem

AC-7 requires Firefox and Safari to verify the hash in the browser for files up to 2 GiB, and AC-51 caps the browser heap at 200 MB. The description names no mechanism ("fixed by spike S12": streamed fetch with incremental hashing and a disk-backed save, no whole-file `Blob` in the heap). IT-16-8 is BLOCKED on it (`components/16_web_app/tests.md`, `tests/spec-gaps.md` row IT-16-8), and so is the Firefox/Safari path of E-26/c12 (risk R30, release gate S12).

## Outcome

- At least two candidate mechanisms measured in Firefox and Safari at 1 GiB and 2 GiB: peak heap, hash result and duration.
- A decision recorded in `components/16_web_app/description.md` §5 Downloads step 2, in IT-16-8 of `components/16_web_app/tests.md` (BLOCKED clause replaced by input and expected heap) and in the IT-16-8 row of `tests/spec-gaps.md`.
- If no candidate passes in an engine, the record says so and names the owner decision gate S12 needs; E-26/c12 then builds the labelled unverified path for that engine.

## Scope

### Included
- Candidate mechanisms that keep the file out of the JavaScript heap while hashing it block by block with the WASM hasher from the E-26/c01 probe.
- Runs in Firefox (`browsers` container) and Safari (owner Mac, `safaridriver`) at 1 GiB and 2 GiB; heap from the Firefox memory report and Safari `vmmap` footprint (weak evidence, labelled).
- A corrupt-block run per candidate and a check under the CSP of 16 §5.

### Excluded
- The production download code (E-26/c12); the Chromium save-picker path (already defined); downloads above 2 GiB (native download, labelled unverified).

## Acceptance Criteria

**AC-1: Candidates measured**
Given at least two candidate mechanisms and server files of 1 GiB and 2 GiB
When each file is downloaded with each candidate in Firefox and in Safari
Then the record holds, per candidate, engine and size, the peak heap, whether the saved file's hash equals the server's, and the duration; Safari values are labelled weak evidence, and a run without the Mac is recorded `NOT-RUN(HW-DEVICE)`, never as a pass

**AC-2: Pass rule applied**
Given the measurements
When a candidate keeps the peak heap at or below 200 MB (target 160 MB) and saves a 2 GiB file whose hash equals the server's in both engines
Then the record names it as the chosen mechanism; otherwise it names the failing engine and the owner decision gate S12 needs (lower verified limit, or the labelled unverified path)

**AC-3: Corruption is never saved as good**
Given the chosen candidate and a server copy with one corrupt block
When the file is downloaded
Then the mismatch is detected before the page could report the file as saved, and the record states per engine what is left on disk (partial file removed, or left under a non-final name)

**AC-4: Works under the CSP**
Given the chosen candidate
When it runs under exactly the CSP of 16 §5
Then 0 CSP violations are reported, or the record names the extra directive needed and hands it to the deployment owner of the CSP header

**AC-5: Spec gap closed**
Given the decision
When the spike closes
Then the 16 description §5, IT-16-8 in the 16 test spec and the IT-16-8 row in `tests/spec-gaps.md` are updated with the date, and E-26/c12 can write its Firefox and Safari criteria from the record

## Non-Functional Requirements

**Performance**
- Heap target 160 MB, failure above 200 MB (AC-51, PT-16-2).

## Unit Tests

| AC Ref | What to Test | Required Outcome |
|--------|-------------|-----------------|
| AC-3 | Incremental hash of a stream with one flipped block against the expected file hash | Mismatch reported at the end of the stream, never a match |

## Blackbox Tests

| AC Ref | Initial Data/Conditions | What to Test | Expected Behavior | NFR References |
|--------|------------------------|-------------|-------------------|----------------|
| AC-1, AC-2 | 1 GiB and 2 GiB files on the real server; Firefox; Safari on H-MAC | Download with each candidate, heap sampled | Values recorded; pass rule applied | AC-7, AC-51, IT-16-8 |
| AC-3 | One corrupt block (seeded on the volume) | Download with the chosen candidate | Mismatch detected; disk state recorded | NFT-SEC-21 |
| AC-4 | 16 §5 CSP | Chosen candidate | 0 violations or the needed directive recorded | IT-16-9 |

## Constraints

- No whole-file `Blob` or `ArrayBuffer` in the heap (16 §5 Downloads step 2).
- The hash must come from the WASM hasher, never a JavaScript implementation.

## Risks & Mitigation

**Risk 1: No candidate passes in Safari**
- *Risk*: Safari may offer no disk-backed streaming save.
- *Mitigation*: AC-2 records the failing engine and the owner decision; the labelled unverified path stays correct (AC-7 wording "not verified in the browser").

## Flagged concerns

| Concern | Policy / owner | Status | Severity |
|---------|----------------|--------|----------|
| If no candidate passes in Safari or Firefox, AC-7's "verify up to 2 GiB" cannot hold for that engine; gate S12 needs an owner decision | Owner (gate S12) | open | Medium |
| Safari runs need the owner Mac (H-MAC) | Owner (hardware) | open | Low |

## Results (2026-11-14, measured on a developer workstation, timings informational)

**Setup.** Probe code lived only in a scratch folder (not in the repo): a Node static server that sends exactly the 16 §5 CSP on every response, honours `Range` and can limit the link to N MiB/s; a page with no inline script; a module Worker and a module Service Worker that load the `sync-core-wasm` package (wasm-pack `--target web`, the package the vector gate `scripts/ci/check-wasm-vectors.sh` builds) and hash every 2 MiB block with its `hash_block`, the file hash with `rollup`. Files: random 1 GiB and 2 GiB, and a 1 GiB copy with one byte flipped in block 300 (the expected hash given to the page stays the good file's). "Server hash" = `rollup` of the block hashes from the same package in Node; the saved file was hashed again the same way after each run. Firefox 159.0 (Playwright 1.65.0 build) was started directly with a fresh profile and no automation, because under Playwright its automation layer kept about 1 GB of JavaScript in the parent process per GiB downloaded (memory report, first attempt). Chromium 158 and Playwright WebKit 27.2 (Linux, not Safari) ran under Playwright 1.65.0 with a persistent profile, labelled extra evidence. The `browsers` container was not used: the same Playwright browser builds AZ-3930 used ran on the workstation.

**Memory method.** Peak minus pre-download value of the proportional set size (PSS) summed over all browser processes, every 250 ms (an upper bound of the heap: it includes WASM memory and native network buffers). In Firefox also the memory report (`SIGRTMIN` to the parent, every 3 s): "JS" = growth of `explicit/` under windows, workers and `js-non-window`. Control: `fetch().arrayBuffer()` of 1 GiB in a Worker showed +1,783 MB (Firefox without automation; memory report JS +1,023 MB in the Worker; +3.8 GB under Playwright), so the method sees a whole file held in memory.

**Candidates.** A1 = Worker, one streamed `fetch`, blocks written to an OPFS file with `FileSystemSyncAccessHandle`, `<a download>` of the OPFS `File` (`blob:` URL) only after `rollup` matches. A2 = A1 with ranged pull (8 MiB `Range` requests, at most 2 in flight, bodies read as streams). B1 = module Service Worker answers a download navigation (hidden iframe) with a stream: one `fetch`, each block hashed on the way, the last block held back until `rollup` matches, `controller.error()` on a mismatch. B2 = B1 with the ranged pull of A2. A first A2/B2 version with 16 MiB ranges read by `arrayBuffer()` peaked at 197 to 275 MB in Firefox (JS up to 219 MB) and was replaced.

**Results** (memory "peak MB (Firefox JS MB)"; links: localhost unthrottled and 100 MiB/s; two runs per cell for Firefox A2 and B2, one otherwise; duration = verified + saved, 2 GiB):

| Candidate | Engine | Size | Peak memory | Hash equal | Duration localhost / 100 MiB/s | Corrupt block (1 GiB) | CSP |
|---|---|---|---|---|---|---|---|
| A1 | Firefox | 1 GiB | 142 (57) at 100 MiB/s; 1,017 (58) unthrottled | yes | 5.6 s / 12.4 s | detected, OPFS file deleted, download folder empty | 0 violations |
| A1 | Firefox | 2 GiB | 144 (68); 1,913 (82) unthrottled | yes | 12.4 s / 24.7 s | | 0 |
| A2 | Firefox | 1 GiB | 132 to 155 (55 to 102) | yes | 5.8 to 5.9 s / 11.9 s | detected, OPFS file deleted, download folder empty (peak 129) | 0 |
| A2 | Firefox | 2 GiB | 123 to 144 (56 to 106) | yes | 11.9 to 12.1 s / 23.0 to 23.5 s | | 0 |
| B1 | Firefox | 1 GiB | 125 (42); 1,049 (92) unthrottled | yes | 7.6 s / 10.9 s | detected, download failed; left `.part` with all but the last block and a 0-byte file under the final name | 0 |
| B1 | Firefox | 2 GiB | 125 (80); 2,012 (37) unthrottled | yes | 15.7 s / 21.7 s | | 0 |
| B2 | Firefox | 1 GiB | 175 to 184 (80 to 98) | yes | 7.6 s / 10.3 to 10.4 s | as B1 (peak 193) | 0 |
| B2 | Firefox | 2 GiB | 169 to 191 (67 to 93) | yes | 14.9 to 15.4 s / 20.7 s | | 0 |
| A1 | Chromium | 1 / 2 GiB | 132 to 141 / 242 to 244 | yes | 2 GiB 10.8 s / 26.9 s | detected, nothing saved, OPFS empty | 0 |
| A2 | Chromium | 1 / 2 GiB | 346 to 359 / 448 to 469 | yes | 2 GiB 16.3 s / 25.7 s | detected, nothing saved, OPFS empty | 0 |
| B1 | Chromium | 1 / 2 GiB | 86 to 87 / 87 to 88 | yes | 2 GiB 12.5 s / 25.1 s | detected, download cancelled, nothing left in the download folder | 0 |
| B2 | Chromium | 1 / 2 GiB | 280 to 289 / 288 to 298 | yes | 2 GiB 18.1 s / 24.0 s | detected, download cancelled, nothing left in the download folder | 0 |
| A1, A2 | Playwright WebKit | 1 / 2 GiB | n/a | no: `QuotaExceededError` after 896 MiB written to OPFS, both sizes | n/a | n/a (quota first; the staging file stays until the page deletes it) | 0 |
| B1 | Playwright WebKit | 1 / 2 GiB | 373 / 420 at 100 MiB/s; 873 / 1,449 unthrottled | yes | 2 GiB 13.1 s / 25.2 s | detected, download failed, partial file kept (956,301,312 B) | 0 |
| B2 | Playwright WebKit | 1 / 2 GiB | 119 to 202 / 95 to 236 | yes | 2 GiB 11.6 s / 23.8 s | detected, download failed, partial file kept (all but the last block) | 0 |
| any | Safari (H-MAC) | 1 / 2 GiB | `NOT-RUN(HW-DEVICE)` | | | | |

**Slow link (Service Worker lifetime).** 1 GiB at 8 MiB/s (about 128 s). Firefox B2 and B1: the download stopped at 238 MiB and 236 MiB (about 30 s), Firefox finalized it as a finished download under the final name (`s12-sw.bin`, 249,561,088 and 247,463,936 bytes, no `.part`), the server saw the range requests aborted and the page never got a result: a truncated file saved as complete. A third run (B2, with detection in the harness) repeated it: 249,561,088 bytes under the final name after about 30 s. Firefox A2: finished in 129.4 s, peak 33 MB, hash equal. Playwright WebKit B2: finished in 129.6 s, peak 81 MB, hash equal. Chromium B2 at 3 MiB/s (beyond its 5-minute event limit): finished in 343.2 s, peak 276 MB, hash equal.

**CSP (AC-4).** Every run above under exactly the 16 §5 CSP: 0 `securitypolicyviolation` events on the page, in the Worker and in the Service Worker, and no blocked request (every run either completed or failed for its stated reason). No extra directive is needed: the `blob:` download navigation, OPFS and the module Service Worker (`worker-src` falls back to `script-src 'self'`) are allowed. Positive control (source on another origin): the Service Worker reported the `connect-src` violation in all three engines and the Worker in Playwright WebKit; the Firefox and Chromium dedicated Workers blocked the fetch (`NetworkError` / `Failed to fetch`) without firing the event, so in those two the evidence for A is that every request succeeded.

**Pass rule (AC-2).** Firefox: A2 passes (peak 155 MB, at or below the 160 MB target, at 1 and 2 GiB at both link speeds; hash equal; 33 MB on a slow link). A1 and B1 fail the 200 MB limit when the link is faster than hashing (no flow control in Firefox). B2 stays under 200 MB but fails AC-3 on a slow link (Firefox ends the Service Worker after about 30 s and saves the truncated file as complete), so B is rejected for Firefox. **Chosen mechanism for Firefox: A2, OPFS ranged pull** (recorded in 16 description §5 Downloads step 2). Safari: no candidate measured (`NOT-RUN(HW-DEVICE)`), and Playwright WebKit, which is not Safari, refused A at 896 MiB (OPFS quota) while B2 passed there. So the pass rule "in both engines" is not met: **owner decision gate S12** (16 description §15 risk 2): (a) measure A2 and B2 in Safari on the owner Mac (`safaridriver`, `vmmap` footprint as weak evidence) before E-26/c12 closes; (b) Safari stays on the labelled unverified path at every size; or (c) a lower verified limit for Safari set from its OPFS quota. Until the owner decides, E-26/c12 builds the labelled unverified path for Safari.

**Corruption (AC-3).** A (chosen): the mismatch is found before anything is saved; the Worker deletes the OPFS staging file; nothing is in the download folder (Firefox and Chromium). B: the mismatch errors the stream, the download fails, and partial data stays on disk (Firefox: `.part` plus a 0-byte file under the final name). Unit-test row: covered by the probe (the flipped block always gave a different `rollup`, never a match); the production unit test belongs to E-26/c12.

**What E-26/c12 must still settle** (not measured here): when the OPFS copy is deleted after a good save (no "download finished" signal reaches the page; the copy doubles disk use until then); the quota check before start (`navigator.storage.estimate()`, fall back to the labelled unverified path when the file does not fit); `ETag` equality across range answers (the probe checked the total size only); private-browsing windows; after the hash matched, the `blob:` save of the OPFS `File` took 1.0 to 1.5 s (1 GiB) and 2.5 to 3.0 s (2 GiB) in Firefox until the file was complete in the download folder.

**Engines not run.** Safari: `NOT-RUN(HW-DEVICE)` (no Mac). Chromium without Playwright: not run (the directly started browser did not load the probe page on this workstation), so the higher Chromium A2/B2 memory is not separated from Playwright's network tracking; Chromium keeps its save-picker path. The `browsers` container: not used; the workstation ran the same Playwright browser builds as AZ-3930.

## Owner decision

2026-11-14: gate S12 option (a) — Safari is measured on the owner Mac with the kit in `_docs/00_research/s12_download_spike/` (README has the steps). Until that run is recorded, E-26/c12 builds the labelled unverified path for Safari.
