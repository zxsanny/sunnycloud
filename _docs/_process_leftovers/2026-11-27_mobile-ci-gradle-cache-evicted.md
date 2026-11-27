# Leftover: the CI `mobile` job's Gradle cache is evicted, so every run fetches cold and Maven Central answers 429

kind: ci
recorded: 2026-11-27 (batches 133-135)
blocked action: a green `ci` run on `dev`; runs 45977837087 (07ee255) and 45980247548 (4eda9c8) failed only in `mobile`

## What happens

- `mobile` fails at "Debug build ...": Gradle cannot resolve `dev.flutter.flutter-plugin-loader` / the Kotlin Gradle
  plugin BOM, "Received status code 429 from server: Too Many Requests" from `repo.maven.apache.org`.
- The repository's Actions cache holds 11.3 GB (`gh api repos/{owner}/{repo}/actions/cache/usage`, 25 entries), over the
  10 GB per-repository limit: six `buildx-*` image caches of 0.75 to 1.14 GiB and the `v0-rust-*` caches. No
  `mobile-gradle-*` entry is left (`gh cache list --key mobile-gradle`): it was evicted.
- `.github/workflows/ci.yml:494-500` restores and saves `~/.gradle/caches` with `actions/cache`, which saves in its post
  step only when the job succeeds. A job that fails on the cold fetch never saves, so the cache stays empty and every run
  hits Maven Central cold.

## Proposed handling (a CI task, not a test or product change)

1. Save the Gradle cache even when the build fails: `actions/cache/restore` before the build and `actions/cache/save`
   with `if: always()` after it (same key), so one run that got past dependency resolution refills it.
2. Keep the cache under 10 GB: the buildx caches of each image are written twice (`-test` and release variants, about
   5.5 GB together); cap them (`mode=min`, or only `dev` writes them) so the small caches are not the ones evicted.
3. Optionally resolve Gradle plugins through Google's Maven mirror first, so a Maven Central rate limit does not fail the
   build.

Verify: two consecutive `ci` runs on `dev` green, and `gh cache list --key mobile-gradle` shows the entry.
