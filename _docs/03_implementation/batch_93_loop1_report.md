# Batch Report

**Batch**: 93
**Tasks**: AZ-3985_android_uniffi_device_spike (parent, on this workstation's emulators)
**Date**: 2026-11-18

## Task Results

| Task | Status | Files Modified | Tests | Issues |
|------|--------|---------------|-------|--------|
| AZ-3985 | Done; AC-5 phone leg `NOT-RUN(HW-DEVICE)` (leftover written), arm ABIs built and alignment-checked, not run | `_docs/02_document/components/17_android_app/description.md` (section 5 record, risk 1), `_docs/_process_leftovers/2026-11-18_az-3985-phone-leg.md` | AC-1..AC-4 on device | none |

No repository code changed. The throwaway probe (a Rust `cdylib` with UniFFI 0.32.2, SQLite and a JNI init export, plus a Gradle app with a WorkManager worker) lives in the session scratchpad and is not merged, as the spec requires. Installed for it (owner chose "run it here"): Rust targets `aarch64-linux-android`, `armv7-linux-androideabi`, `x86_64-linux-android` on toolchain 1.97.1, and the Android 15 `google_apis_ps16k` x86_64 system image plus an AVD `probe16k`.

| AC | Evidence | Result |
|---|---|---|
| AC-1 | `ping` returned 42 from the UI thread and a WorkManager worker on the 16 KB image (`PAGESIZE` 16384) and the 4 KB image (4096); every `LOAD` segment of `libprobe.so` and JNA 5.17.0's `libjnidispatch.so` is `0x4000` in all three ABIs, `zipalign -c -P 16` passes; the seeded 4 KB build is `0x1000` and the check script exits 1 on it, and it dies with `UnsatisfiedLinkError` on the 16 KB image while loading on the 4 KB one | pass (x86_64 run; arm ABIs build + alignment only) |
| AC-2 | 20 `kill -9` at random moments on each image: every restart loads, opens the WAL file, `integrity_check` ok, `max(id) >= last acked id`, no gaps | pass x2 |
| AC-3 | one process `org.probe` for UI and worker; merged manifest has no `android:process` | pass |
| AC-4 | recorded in the 17 description section 5; the verifier mechanism (`nativeInit` JNI export, already decided and implemented in `sync-core/src/ffi/android.rs`) works in the same `.so` as UniFFI: a coroutine-thread UniFFI call attached to the stored `JavaVM` and loaded an app class | pass |
| AC-5 | no phone attached | NOT-RUN(HW-DEVICE) |

## Code Review Verdict: PASS_WITH_WARNINGS

| # | Sev | Category | Location | Finding | Outcome |
|---|---|---|---|---|---|
| 1 | Low | Evidence | AC-1 | arm64-v8a and armeabi-v7a never ran; the `x86_64` runs cannot show an arm-only failure | Kept, stated in the record and the leftover |
| 2 | Low | Evidence | load time | measured on an emulator with swiftshader, not a phone | Kept; labelled as emulator figures |

## Test Suite

Documents only: no source changed, so the CI-parity gate does not apply. Device runs listed above.

## Discovered during implementation

| # | Scenario or question | Spec ref | Proposed handling | clear / unclear |
|---|----------------------|----------|-------------------|-----------------|
| 1 | NDK r28 already links with 16 KB alignment; the explicit `max-page-size=16384` flag is belt and braces | AZ-3985 AC-1 | Keep the flag, the CI alignment check is the proof | clear |
| 2 | A 4 KB library fails on a 16 KB image with a misleading `DT_HASH` message | AZ-3985 AC-1 | CI alignment check is the diagnostic | clear |
| 3 | A worker may start in a process the UI never touched, so `nativeInit` must run at worker start too | E-23/c15, E-27/c12 | Handed to those tasks in the record | clear |

## Commit

`[AZ-3985] Android single-library device spike: record and phone-leg leftover` — Loop: 1

## Next Batch: AZ-3989 (mobile_backup checkpoint DB, 5 points) is now ready; recompute
