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
  detail: "MVP build in progress (roadmap _docs/02_document/mvp_roadmap.md; owner scope 2026-11-20: Linux server + web first, native iOS later on the owner's Mac). Done and pushed, CI green (run 44886201366, 24 jobs, 8.8 min): live stack on drive.zxsanny.com (Caddy + Let's Encrypt, systemd user units, scripts/dev/mvp-*.sh, runbook _docs/04_deploy/mvp_runbook.md), decoder previews (AZ-3854/3855/3857/3858/3859), realtime WebSocket with packbin (contract 11_http_api/realtime.md v0.3.0, ThumbNotifier), web app on Bun (AZ-3935/3936/3938/3939/3940/3945/3947/3956, realtime client), acceptance harness tests/e2e + tests/mocks/idp (scripts/dev/acceptance-*.sh, 11 Playwright tests). NEXT: owner puts the Google OAuth client into secrets-dev/ (sign-in cannot complete before), owner decides Cloudflare proxy vs DNS only, live two-browser realtime tests done (effeb8f, 17/17), Jira In Testing replays done 2026-11-21 (AZ-3985, AZ-3989, AZ-3990, AZ-3991, AZ-3997, AZ-4002, AZ-4000 read back In Testing), post-MVP list in the roadmap section 7. STILL GATED: Windows/Mac/phone spikes, native iOS (Mac session, AZ-4024 Sign in with Apple first), E-31 stack. LOCAL ENV: dev DB container sunnycloud-pg18 (DATABASE_URL=postgres://postgres:dev@127.0.0.1:57432/postgres), CARGO_INCREMENTAL=0, PATH needs $HOME/.bun/bin for the web app; CI runs Ubuntu 24.04 (libvips 8.15, ffmpeg 6.1), this host is 26.04: reproduce CI problems in an ubuntu:24.04 container."
retry_count: 0
tracker: jira
loop: 1
branch:
plan_artifact:
plan_confirmed: false
plan_step_index: 0
assess_round: 0
