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
  detail: "2026-11-26 newest++: batch 123 done: AZ-3813 complete and archived (AC-5: keyring and invite commands bind only their keys, ConfigCheck::Services, IdentityInvites own builder; IT-12 19/19). Jira drift fixed for AZ-3855/3857/3946/4024. Next: AZ-3815, AZ-3835 AC-1 and AC-4, AZ-3847/3878 suites. Before: 2026-11-24: batch 122 done: AZ-3813 CLI suite part done and committed a4c4dc2 (cli_suite.rs, IT-12 map, job cli-suite, 90% gate on exit/guard/secret/output/serve start); AC-5 not met: admin setup-token and key-check need the whole server config, two ignored tests, leftover 2026-11-24_az-3813-lazy-config.md; AZ-3813 stays in todo. Next: AZ-3815, AZ-3835 AC-1 and AC-4, AZ-3847/3878 suites. Before: batch 121 done: AZ-3661 media pipeline archived (run.sh core 75/0/4; fixed decoder scratch path and job folder mode; hostile-class restart and kill at fetch/post are NOT-RUN, leftover 2026-11-24_az-3661-not-run-legs.md). Next: AZ-3813/3815 CLI suites, AZ-3835 AC-1 and AC-4, AZ-3847/3878 suites. Before: batch 120 done: AZ-3665 account deletion complete and archived (run.sh core 65/0/1; process-kill action). Next: AZ-3661, the CLI suites AZ-3813/3815, AZ-3835 AC-1 and AC-4. Before: batch 119 done: AZ-3662 share links complete and archived (run.sh core 61/0/1; Drive client in the harness). Next: AZ-3665, AZ-3661. Before: batch 118 done: AZ-3664 complete and archived (negative callbacks, session rules, limiter and audit; run.sh core 56/0/1). Next: AZ-3662, AZ-3665 (need an upload client in the harness). Before: batch 117 done (AZ-3664 part 1); batch 116 done: AZ-3654 first-run pipeline archived (serve prints the setup token; run.sh core 35/0/1; golden snapshot now holds the blob volume). Next: AZ-3664, AZ-3662, AZ-3665 on the stack. Before: batch 115 done: AZ-3835 Mode R of IT-11-9 (test image) and IT-11-17 pass on the stack (run.sh core 29/0/1); real ctl actions internal-http, db-outage, interrupted-upgrade; /readyz now has the migrations item. AZ-3835 stays in todo (AC-1, AC-4, AC-5, release-image leg: see its leftover). Next: AZ-3654 first run, AZ-3664, AZ-3662, AZ-3665 on the stack. 2026-11-24 later: completeness gate written (implementation_completeness_loop1_report.md); E-31 compose test stack built, tests/run.sh --profile core green 19/0/1 (batch_114). Next: suites on the stack (AZ-3835 Mode R, AZ-3654, AZ-3664, AZ-3662, AZ-3665), web partials, then Android/desktop/vault cores. 2026-11-24 earlier: AZ-3795 AC-4 admin dev-seed done and archived, Jira In Testing (batch_113_loop1_report.md; real run on a throwaway DB with the mock IdP). Before: 2026-11-23: AZ-3861 hello and self-check done and archived, Jira In Testing (batch_112_loop1_report.md, CI run 45133395977 green, 25 of 25 jobs). Open E-19: AZ-3862 NVENC probe (v1.1, no GPU container here), AZ-3865 NOT RUN list (IT-10-20 corpus, IT-10-13b, phone data), AZ-3866 perf and security (reference host), AZ-3849 and AZ-3850 spikes, AZ-3855/3857/3858 partials. Browser tier done 2026-11-23: 65 Playwright tests green on a fresh acceptance stack at https://192.168.88.100:18800 (ACC_HOST; see _docs/gotchas). Next: the compose test stack and E-31 decompose. Local env: dev DB container sunnycloud-pg18 (DATABASE_URL=postgres://postgres:dev@127.0.0.1:57432/postgres), CARGO_INCREMENTAL=0, PATH needs $HOME/.bun/bin for the web app; CI runs Ubuntu 24.04 (libvips 8.15, ffmpeg 6.1), this host is 26.04."
retry_count: 0
tracker: jira
loop: 1
branch:
plan_artifact:
plan_confirmed: false
plan_step_index: 0
assess_round: 0
