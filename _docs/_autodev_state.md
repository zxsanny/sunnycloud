# Autodev State

## Current Step
flow: greenfield
kind: setup
step: 7
name: Implement
status: in_progress
sub_step:
  phase: 2
  name: batch-loop
  detail: "Pushed 9241e8a: batches 93 AZ-3985, 94 AZ-3989, 95 AZ-3990, 96 AZ-3991, 97 AZ-3997+AZ-4002, 98 AZ-4000. Hopper empty again for the agent: after AZ-4000 the Android chain (AZ-3992, AZ-3995, AZ-4003 emulator harness) waits on the E-31 test stack SUT (tests/compose.test.yml has no api service); AZ-3975 iOS static library needs a Mac. CI ON GITHUB IS RED since 2026-11-17, pre-existing: supply-chain (RUSTSEC-2026-0459 libsqlite3-sys 0.37.0 + 5 unmaintained-crate warnings) and rust-test (watchdog stall at 120 s in the llvm-cov compile); see _docs/_process_leftovers/2026-11-18_ci-red-supply-chain-and-rust-test.md; fixing it is the best next step (owner policy: advisory ignores). The mobile job of the AZ-3997/AZ-4002 push was still running when last read. Jira In Testing PENDING for AZ-3985, AZ-3989, AZ-3990, AZ-3991, AZ-3997, AZ-4002, AZ-4000 (tracker hook timed out; leftover 2026-11-18_tracker-az-3985-in-testing.md). Other leftovers: az-3985-phone-leg, az-3990-real-server-legs (+3991/3997), az-4002-release-signing-and-runner, az-4000-emulator-and-commands. STILL GATED: decoder spikes AZ-3849/3850 (REF-CPU4 + clips), Caddy S1 AZ-3799 (1 GbE 20 ms RTT, root tc), desktop spikes AZ-3881/3882/3883/3884/3886/3893/3921 (Windows, Mac, Linux ref), AZ-3987/3988 (phones), AZ-4005 (devices), AZ-4006 (E-13 owner), AZ-3932 (Safari); E-31 stack gates AZ-3664, AZ-3654, AZ-3971, AZ-3835 AC-2/3, AZ-3795 AC-4, AZ-4003 and the real-server legs. Owner to confirm: AZ-4332 enqueue adoption; AZ-4004 vault review sign-off; device_id storage (AZ-3990); run_backup export shape (c08); Play signing keys (R18); default buckets (AZ-3993). LOCAL ENV: container sunnycloud-pg18, DATABASE_URL=postgres://postgres:dev@127.0.0.1:57432/postgres, CARGO_INCREMENTAL=0; mobile builds need ANDROID_HOME=$HOME/Android/sdk JAVA_HOME=$HOME/.local/share/jdk21-temurin/jdk PATH=$HOME/flutter/bin:$PATH; AVDs probe16k and arch_api34 (headless: emulator -avd NAME -no-window -no-audio -gpu swiftshader_indirect); PG18 tool wrapper for pipeline_db_restore = dir with executable pgtool (exec docker exec -i -e PGUSER -e PGPASSWORD -e PGDATABASE sunnycloud-pg18 $(basename $0) $@) and symlinks pg_dump, pg_restore, psql, then SC_PG_TOOLS=<dir>."
retry_count: 0
tracker: jira
loop: 1
branch:
plan_artifact:
plan_confirmed: false
plan_step_index: 0
assess_round: 0
