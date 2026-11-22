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
  detail: "2026-11-22 (latest): AZ-3943 share dialog and public page done (67e06e0); password links now open (06 `open` takes the grant, contracts part 3 1.6.0), web e2e 51 of 51; Jira In Testing (read back); the sign-in flake was an e2e race on the Files h1, fixed. 2026-11-22: server gaps filled (batch_103 2b35756, batch_104 c6b099b), web wired (batch_105 49e87c6); AZ-3944 complete and archived, Jira In Testing (read back); AZ-3937 (email verified, last sign-in) and AZ-3946 (no revoked invites) partial; browser tier NOT RUN for all web pages; CI fixes fb03dc9 (secret scan allowlist), 891f0eb (abort fake), 0affb77 (quota wait). 2026-11-21 (evening): roadmap item 2 built as partial pages (account b04aa96, device approval b8e23b6, admin f8b8eff; reports batch_100 to 102) because the server lacks fields: sessions list kind/IP/current, approval platform, link completion, user name/email, invite URL and revoked marker, default quota, share expiry limits; AZ-3943 share dialog not started (needs share limits). Owner chose admin first; next owner decision is the one server change that fills these gaps. 2026-11-21 (later): roadmap section 7 item 1 split: AZ-3941/3942 web run in another session; cookie renewal done (872f1ec server, edaa78c web, contract 1.10.0, browser run is a leftover); desktop/mobile long-poll move has no client to move yet (sync-core-proto has the types only). 2026-11-21: MVP specs reconciled in batch_99_loop1_report.md: 12 of 14 are PARTIAL against their ACs, none archived, Jira left To Do; owner decides order (finish partial specs vs roadmap section 7). MVP build in progress (roadmap _docs/02_document/mvp_roadmap.md; owner scope 2026-11-20: Linux server + web first, native iOS later on the owner's Mac). Done and pushed, CI green (run 44886201366, 24 jobs, 8.8 min): live stack on drive.zxsanny.com (Caddy + Let's Encrypt, systemd user units, scripts/dev/mvp-*.sh, runbook _docs/04_deploy/mvp_runbook.md), decoder previews (AZ-3854/3855/3857/3858/3859), realtime WebSocket with packbin (contract 11_http_api/realtime.md v0.3.0, ThumbNotifier), web app on Bun (AZ-3935/3936/3938/3939/3940/3945/3947/3956, realtime client), acceptance harness tests/e2e + tests/mocks/idp (scripts/dev/acceptance-*.sh, 11 Playwright tests). NEXT: owner puts the Google OAuth client into secrets-dev/ (sign-in cannot complete before), owner decides Cloudflare proxy vs DNS only, live two-browser realtime tests done (effeb8f, 17/17), Jira In Testing replays done 2026-11-21 (AZ-3985, AZ-3989, AZ-3990, AZ-3991, AZ-3997, AZ-4002, AZ-4000 read back In Testing), post-MVP list in the roadmap section 7: AZ-3941 resume (a9e52a7) and AZ-3942 verified downloads (1c518ed) done and In Testing 2026-11-21, owner decisions in _process_leftovers/2026-11-21_az-3941-ac5-restart.md and 2026-11-21_az-3942-download-open-points.md; AZ-3943 share dialog and public page done 67e06e0 (password links wait for a server fix, see _process_leftovers/2026-11-21_az-3943-share-open-points.md; Jira In Testing pending: 2026-11-21_tracker-az-3943-in-testing.md); AZ-3937 and AZ-3944 were committed by another Autodev session; next: realtime for desktop/mobile, server fix for the share open grant. STILL GATED: Windows/Mac/phone spikes, native iOS (Mac session, AZ-4024 Sign in with Apple first), E-31 stack. LOCAL ENV: dev DB container sunnycloud-pg18 (DATABASE_URL=postgres://postgres:dev@127.0.0.1:57432/postgres), CARGO_INCREMENTAL=0, PATH needs $HOME/.bun/bin for the web app; CI runs Ubuntu 24.04 (libvips 8.15, ffmpeg 6.1), this host is 26.04: reproduce CI problems in an ubuntu:24.04 container."
retry_count: 0
tracker: jira
loop: 1
branch:
plan_artifact:
plan_confirmed: false
plan_step_index: 0
assess_round: 0
