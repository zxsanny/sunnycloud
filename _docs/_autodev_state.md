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
  detail: "Batch 92 pushed (05038aa): AZ-3667 scrub pipeline done, scrub run now loads the key ring (production bug found by its tests), cumulative 89-91 fixes, Jira AZ-3667 In Testing. HOPPER EMPTY of work the agent can run: 140 specs in todo/, every ready one is gated. OWNER/HARDWARE NEEDED (tasks each unlocks): decoder spikes AZ-3849/3850 need REF-CPU4 + real clips (20); Android spike AZ-3985 needs emulator + phones (25); Caddy S1 AZ-3799 needs 1 GbE 20 ms RTT + root tc (18); desktop spike AZ-3881 needs WIN-VM + Linux ref (9), AZ-3883 Windows (3); AZ-4005 Argon2 devices (4); AZ-4006 E-13 owner agreement on client_meta bound (2); AZ-3893 Windows (2); AZ-3932 Safari (2). E-31 test stack (mock-idp, api+Caddy compose, test images) gates AZ-3664, AZ-3654, AZ-3971, AZ-3835 AC-2/3, AZ-3795 AC-4. Owner to confirm: AZ-4332 enqueue adoption; AZ-4004 vault review sign-off. LOCAL ENV: container sunnycloud-pg18 (unless-stopped), DATABASE_URL=postgres://postgres:dev@127.0.0.1:57432/postgres, CARGO_INCREMENTAL=0; PG18 tool wrapper for pipeline_db_restore = dir with executable pgtool (exec docker exec -i -e PGUSER -e PGPASSWORD -e PGDATABASE sunnycloud-pg18 $(basename $0) $@) and symlinks pg_dump, pg_restore, psql, then SC_PG_TOOLS=<dir>. Run cargo feature builds one at a time."
retry_count: 0
tracker: jira
loop: 1
branch:
plan_artifact:
plan_confirmed: false
plan_step_index: 0
assess_round: 0
