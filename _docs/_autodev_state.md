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
  detail: "2026-11-24 newest: batch 117 done: AZ-3664 part 1 (AC-1, 4, 6, 75; run.sh core 38/0/1); AC-2, 3, 5 in _process_leftovers/2026-11-24_az-3664-remaining.md. Next: AZ-3662, AZ-3665. Before: batch 116 done: AZ-3654 first-run pipeline archived (serve prints the setup token; run.sh core 35/0/1; golden snapshot now holds the blob volume). Next: AZ-3664, AZ-3662, AZ-3665 on the stack. Before: batch 115 done: AZ-3835 Mode R of IT-11-9 (test image) and IT-11-17 pass on the stack (run.sh core 29/0/1); real ctl actions internal-http, db-outage, interrupted-upgrade; /readyz now has the migrations item. AZ-3835 stays in todo (AC-1, AC-4, AC-5, release-image leg: see its leftover). Next: AZ-3654 first run, AZ-3664, AZ-3662, AZ-3665 on the stack. 2026-11-24 later: completeness gate written (implementation_completeness_loop1_report.md); E-31 compose test stack built, tests/run.sh --profile core green 19/0/1 (batch_114). Next: suites on the stack (AZ-3835 Mode R, AZ-3654, AZ-3664, AZ-3662, AZ-3665), web partials, then Android/desktop/vault cores. 2026-11-24 earlier: AZ-3795 AC-4 admin dev-seed done and archived, Jira In Testing (batch_113_loop1_report.md; real run on a throwaway DB with the mock IdP). Before: 2026-11-23: AZ-3861 hello and self-check done and archived, Jira In Testing (batch_112_loop1_report.md, CI run 45133395977 green, 25 of 25 jobs). Open E-19: AZ-3862 NVENC probe (v1.1, no GPU container here), AZ-3865 NOT RUN list (IT-10-20 corpus, IT-10-13b, phone data), AZ-3866 perf and security (reference host), AZ-3849 and AZ-3850 spikes, AZ-3855/3857/3858 partials. Browser tier done 2026-11-23: 65 Playwright tests green on a fresh acceptance stack at https://192.168.88.100:18800 (ACC_HOST; see _docs/gotchas). Next: the compose test stack and E-31 decompose. Local env: dev DB container sunnycloud-pg18 (DATABASE_URL=postgres://postgres:dev@127.0.0.1:57432/postgres), CARGO_INCREMENTAL=0, PATH needs $HOME/.bun/bin for the web app; CI runs Ubuntu 24.04 (libvips 8.15, ffmpeg 6.1), this host is 26.04."
retry_count: 0
tracker: jira
loop: 1
branch:
plan_artifact:
plan_confirmed: false
plan_step_index: 0
assess_round: 0
