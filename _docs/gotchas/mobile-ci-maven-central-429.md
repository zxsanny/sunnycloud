---
title: The CI `mobile` job can fail with Maven Central 429 on a cold Gradle cache; that is not the change
applies_when: reading a red CI run whose only failed job is `mobile` with "Received status code 429 from server: Too Many Requests"
source: 2026-11-26 (first seen, noted in the state file) and 2026-11-27, run 45977837087 for 07ee255 (a test-harness change, nothing under mobile)
---

The `mobile` job resolves Gradle plugins from Maven Central (`dev.flutter.flutter-plugin-loader`, the Kotlin Gradle
plugin BOM). On a cold Gradle cache GitHub runners share Maven Central's rate limit, and a 429 fails the build ("Error
resolving plugin ... Repository Gradle Central Plugin Repository is disabled due to earlier error"). Check that the run
has no other red job and that the change touched nothing under `src/mobile/` or `scripts/ci/mobile-*`; then the next
push's run is the re-check (do not change code for it). If it fails twice in a row on the same commit, it is a cache
problem of the job (cache key, `CACHE_ON_FAILURE`), not a flake: open a task for the job's Gradle cache.
