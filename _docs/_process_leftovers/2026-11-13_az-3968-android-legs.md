# Leftover: AZ-3968 Android and Kotlin legs not run

kind: environment / downstream task
blocked action: (1) compile sync-core with `--features uniffi` for an Android target; (2) AC-6 device run of `TlsTrustBridge.nativeInit` and a TLS request on an emulator; (3) AC-3's Kotlin leg (a foreign `FileSource`/`CheckpointStore` written in Kotlin driving hashing); (4) `nm -D` on the built `mobile_backup` `.so` shows `Java_org_sunnycloud_backup_TlsTrustBridge_nativeInit`
reason: no Android Rust target is installed on this workstation (not installed on purpose: no toolchain changes for one check) and no emulator harness exists yet (E-27/c19); the Kotlin leg needs a server-less hashing entry point (`uniffi_surface.md` open point 2, E-23/c16 or E-27)

Replay: when the Android target and the E-27 emulator harness exist, run `cargo check -p sync-core --features uniffi --target aarch64-linux-android`, build `mobile_backup`, check the exported symbol, run the AC-6 device test, then the Kotlin AC-3 leg once open point 2 is decided; delete this file when all pass.
