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
problem of the job, not a flake. Seen on 2026-11-27 for two pushes in a row: the repository's Actions cache was over
its 10 GB limit, the `mobile-gradle-*` entry had been evicted, and `actions/cache` saves only after a green job, so it
never came back. Check `gh cache list --key mobile-gradle` first.

Fixed by AZ-4389 (2026-11-27). The job restores the cache first and saves it at the end of the job, also after a
failed step, once the debug build got through. Only pushes to `dev` write the large caches; pull requests read
them. Daily Dependabot PRs had been writing about 5 GB of image caches under their own refs. Only the decoder
image keeps a BuildKit cache, so the `dev` set is about 8.3 GB of the 10 GB limit. If the entry is gone again,
`gh api repos/{owner}/{repo}/actions/cache/usage` shows whether the repository is back over 10 GB, and
`gh cache list --sort size_in_bytes` shows what grew. Cache layout: `_docs/02_document/deployment/ci_cd_pipeline.md`
§Caching and parallelism.
