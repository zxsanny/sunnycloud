# Validation Log

## Scenario 1: 5 GiB upload interrupted at 50%
**Expected.** Client has a blocklist of 2,560 blocks; server reports which are present; client uploads missing blocks over 4 lanes; on resume at most 4 x 2 MiB = 8 MiB are re-sent; the file is committed by rename in constant time; final BLAKE3 equals the original.
**Check.** Consistent with AC-3, AC-4, AC-7 and expected result row Transfer #1. No counterexample.

## Scenario 2: Edit on device A, online device B
**Expected.** Settle 2 s + hash/upload < 1 s for 5 MB + commit + held long-poll wake < 0.1 s = about 3-3.5 s total, under the 5 s budget (AC-5).
**Check.** Holds only if the long-poll wakes on commit; polling would fail. Budget margin about 1.5 s: no extra debounce allowed.

## Scenario 3: Concurrent offline edits (AC-8)
**Expected.** Server commit order decides; later commit wins; the loser's content is overwritten silently (accepted).
**Counterexample.** Clock skew cannot reorder since client mtime is not used. Data loss for the earlier edit is the accepted model.

## Scenario 4: Vault file of 100 MiB
**Expected.** Client encrypts in 64 KiB-1 MiB chunks via the WASM or native core; server stores ciphertext; wrong passphrase fails at key unwrap.
**Check.** Argon2id 64 MiB takes about 128 ms native, about 200 ms in V8 wasm (phones and Safari unmeasured). Vite + SvelteKit glue is unproven (runtime gate).

## Scenario 5: 100 new videos after a phone import
**Expected.** Thumbnails use a reserved lane and finish first; transcodes queue behind and drain at worker speed; GPU workers drain about 3x faster.
**Counterexample.** Without a reserved lane, AC-10 fails during bulk import. Resolved by the lane in the design.

## Scenario 6: Hostile media file
**Expected.** Worker container has no network, cgroup limits and a time limit; a decoder exploit reaches only the worker container.
**Check.** `network_mode: none` workers reach Postgres only through a Unix socket volume; GPU passthrough inside this sandbox is untested for Docker (runtime gate).

## Review Checklist
- [x] Conclusions consistent with fact cards
- [x] No important dimension missed (observability, backup, upgrade path, admin UI recorded as open items in the decomposition)
- [x] No over-extrapolation: estimates are labelled; AC-15, AC-16, AC-2 are marked as spike-gated
- [x] Mismatches recorded as disqualifiers (see fit matrix)
- [ ] Issue found: the "Verify" cells in the MVE binding tables are runtime-quality measurements, not API-capability gaps; they are carried as runtime gates (see fit matrix)

## Conclusions Requiring Revision
- AC-11 and the media restriction were revised for operator hardware and GPU use (owner note, 2026-11-02).
- Vault plan changed from `aead::stream` to the `aead-stream` crate.
- iOS camera backup via the 26.1 extension is deferred.
