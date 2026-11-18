# Leftover: AZ-4002 release signing and first CI run

kind: owner / environment
recorded: 2026-11-18
blocked action: (1) a shippable release APK: `src/mobile/android/app/build.gradle.kts` reads the gitignored `android/key.properties` (`storeFile`, `storePassword`, `keyAlias`, `keyPassword`); without it the release build is debug-signed; (2) the `mobile` job on a GitHub runner: `scripts/ci/mobile-setup.sh` installs Flutter 3.47.7 and the NDK and needs `sdkmanager` at `$ANDROID_HOME/cmdline-tools/latest` and `xz-utils`; only its "already installed" path ran locally
reason: the Play signing keys are the owner's (R18); the job has never run on GitHub

Replay: owner provides the keys as CI secrets when a shippable release is wanted; on the first push of `dev` read the `mobile` job and fix the setup step if the runner image differs; delete this file when both are done.
