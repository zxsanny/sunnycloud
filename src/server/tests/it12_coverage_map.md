# IT-12 coverage map (AZ-3813 AC-7)

Every expected result of IT-12-1..19 (`_docs/02_document/components/12_operator_cli/tests.md`) is assigned to exactly one place. `cli_suite.rs` checks this file against the spec: every case has rows, no part is listed twice, every place is one of the allowed kinds, and every test named in a `tests/...::fn` proof exists.

Places: `suite` (a test of `cli_suite.rs`); `command AZ-nnnn` (the test file of that command task, run by the CI job `cli-suite` through `it_cases.txt`); `pipeline AZ-nnnn` (a pipeline task on the Compose stack); `owner <task>` (the owning component's own tests); `E-31` (system tests on the Compose stack).
Status: `pass` (a test in the proof column passes in the job); `known failure` (a test exists, is `#[ignore]`d with its reason, and a leftover records it); `blocked` (the test cannot exist yet, a leftover or task names what is missing); `elsewhere` (not run in this job; the place runs it).

| Case | Part | Place | Status | Proof |
|------|------|-------|--------|-------|
| IT-12-1 | exit codes, usage errors, `--json` envelope for every command of the contract | suite | pass | `tests/cli_suite.rs::every_command_exits_0_on_success_and_prints_one_envelope_with_json` |
| IT-12-1 | owner refusal exits 2 with the owner's code | suite | pass | `tests/cli_suite.rs::every_command_exits_2_on_an_owner_refusal_with_the_owners_code_in_the_envelope` |
| IT-12-1 | doctor levels 0, 1 and 2 | suite | pass | `tests/cli_suite.rs::doctor_exits_0_for_ok_1_for_warnings_and_2_for_a_failure_and_json_names_the_level` |
| IT-12-1 | usage errors and missing arguments exit 3, no call | suite | pass | `tests/cli_suite.rs::usage_errors_exit_3_with_no_business_call_and_nothing_on_stdout_even_with_json` |
| IT-12-1 | missing confirmation exits 3, no call; with `--yes` one call | suite | pass | `tests/cli_suite.rs::the_four_destructive_commands_need_yes_on_a_non_tty_and_change_nothing_without_it` |
| IT-12-1 | text to stdout, diagnostics to stderr | command AZ-3788 | pass | `tests/exit_codes.rs::json_refusal_carries_the_owner_code_and_diagnostics_stay_on_stderr` |
| IT-12-1 | the same rules on the running stack through `e2e-ctl exec-operator` | E-31 | elsewhere | AT-12-1 install check, FT-P-64, FT-P-65 |
| IT-12-2 | setup token printed once with a hint; refused after an account exists | command AZ-3794 | pass | `tests/identity_commands.rs::setup_token_prints_the_token_once_with_the_hint_on_each_run` |
| IT-12-2 | `create` and `reset` print an invite once; `reset` of an unknown account exits 2 | command AZ-3794 | pass | `tests/identity_commands.rs::reset_prints_a_fresh_invite_once_and_maps_the_refusals` |
| IT-12-2 | no token in stderr, log lines or another command's output | suite | pass | `tests/cli_suite.rs::each_token_is_on_its_own_stdout_once_and_in_no_stderr_log_or_other_commands_output` |
| IT-12-2 | `serve` prints the setup token of a fresh install and stores only its hash | pipeline AZ-3654 | pass | `tests/serve_setup_token.rs::a_fresh_install_prints_one_setup_token_and_stores_only_its_hash` |
| IT-12-2 | old setup token refused; only the SHA-256 stored; sessions revoked after `reset` | pipeline AZ-3654 | elsewhere | AZ-3654 AC-2 and AC-4 (E-22/p01) |
| IT-12-2 | invite expiry with the test clock | owner E-15 | elsewhere | `--ttl` was dropped (contract open point 6); `invite_ttl` is the owner's config |
| IT-12-3 | `promote` and `demote` call `update_user` as the operator; the last admin exits 2 | command AZ-3794 | pass | `tests/identity_commands.rs::promote_and_demote_call_update_user_as_the_operator_and_the_last_admin_exits_2` |
| IT-12-3 | `invite` create, list and revoke; token once on create, never on list | command AZ-3794 | pass | `tests/identity_commands.rs::invite_create_list_and_revoke_make_one_call_each_and_list_shows_no_token` |
| IT-12-3 | `user list`, `set-quota`, `delete` call the owners; last admin refused | command AZ-3795 | pass | `tests/user_commands.rs::deleting_the_last_admin_exits_2_with_last_admin` |
| IT-12-3 | two concurrent demotes give exit codes {0, 2} | owner E-17/c04 | elsewhere | E-17/c04 |
| IT-12-3 | the new quota is visible to the user | E-31 | elsewhere | FT-N-46 |
| IT-12-3 | the deleted account is purged to 0 rows | pipeline AZ-3665 | elsewhere | AZ-3665 (E-22/p12) AC-2 |
| IT-12-3 | one `admin_action` row per mutating command | owner E-17/c03 | elsewhere | E-17/c03, E-17/c08 |
| IT-12-4 | prints revision and placement; `BeyondRetention`, `NotFound` exit 2; `--dry-run`; bad arguments exit 3 | command AZ-3796 | pass | `tests/maintenance_commands.rs::a_restore_prints_the_revision_and_placement_and_exits_0` |
| IT-12-4 | content hash back, feed change, `revisions` count unchanged | pipeline AZ-3663 | elsewhere | AZ-3663 (E-22/p11) AC-3; E-13/c11 |
| IT-12-5 | no `--yes` exits 3; the bump runs once with the clock, then the `restore` checks; error exits 1 | command AZ-3796 | pass | `tests/maintenance_commands.rs::epoch_bump_calls_once_with_the_clock_then_runs_only_the_restore_checks_and_exits_0` |
| IT-12-5 | marker equals the epoch; devices get `resync_required`; a lowered marker is not accepted | pipeline AZ-3666 | elsewhere | AZ-3666 (E-22/p13) AC-2; NFT-RES-07 |
| IT-12-6 | refusals a, b, e, f, j: pending migration, newer database, missing key file, blobs without `.keycheck`, schema-owner role | command AZ-3791 | pass | `tests/serve_start.rs::a_pending_migration_refuses_names_migrate_and_serve_does_not_migrate` |
| IT-12-6 | refusals g, h, i: unknown variable, retention 91, no provider | command AZ-3790 | pass | `tests/config_binary.rs::serve_refuses_a_start_without_any_sign_in_provider` |
| IT-12-6 | refusals c, d: marker above the epoch, wrong master key | pipeline AZ-3666 | elsewhere | AZ-3666 (E-22/p13) AC-1, AC-72 |
| IT-12-6 | ports 8080, 8081 and 9090 never accept; `healthcheck` non-zero; a valid control run starts | E-31 | elsewhere | FT-N-43 |
| IT-12-7 | start steps logged in order; `/readyz` 200 within 10 s | command AZ-3791 | pass | `tests/serve_start.rs::start_steps_are_logged_in_order_and_readyz_is_200_within_10s` |
| IT-12-7 | every janitor cycles; `/readyz` lists `scheduler_alive` | command AZ-3792 | pass | `tests/composition_scheduling.rs::every_registered_janitor_cycles_at_200ms_and_readyz_lists_scheduler_alive` |
| IT-12-7 | the hook and cleanup lists hold every owner | command AZ-3789 | pass | `tests/composition_root.rs::hook_list_follows_the_documented_order_and_reaches_the_owners_hooks` |
| IT-12-7 | every account table empty after deletion | pipeline AZ-3665 | elsewhere | AZ-3665 (E-22/p12) AC-2 |
| IT-12-7 | derived rows of a collected blob disappear | pipeline AZ-3663 | elsewhere | AZ-3663 (E-22/p11) AC-2 |
| IT-12-8 | SIGTERM and SIGINT run the shutdown in order; the pool closes last; a restart resumes | command AZ-3791 | pass | `tests/serve_stop.rs::sigterm_runs_the_shutdown_in_order_and_a_restart_resumes_only_the_unacknowledged_blocks` |
| IT-12-8 | `docker stop` of the api container with 20 held polls and 10 uploads | E-31 | elsewhere | NFT-RES-22 |
| IT-12-9 | failing and panicking cycles survive; cycles never overlap; 200 ms spacing | command AZ-3792 | pass | `tests/janitor_scheduler.rs::failing_and_panicking_cycles_are_logged_counted_and_survived` |
| IT-12-9 | `gc-run` beside the scheduler and a scrub: no double delete, no false finding | pipeline AZ-3667 | elsewhere | AZ-3667 (E-22/p14) AC-80; AZ-3663 (E-22/p11) |
| IT-12-10 | migrate once, twice, concurrently; checksum, newer, dirty; status; revert; no master key | command AZ-3793 | pass | `tests/migrate.rs::two_concurrent_runs_apply_each_version_exactly_once` |
| IT-12-10 | the api container has no `db_migrator_url` and `migrate` there exits 3 | E-31 | elsewhere | `ctl inspect`; the exit 3 without the secret is `tests/config_binary.rs::migrate_without_the_migrator_secret_exits_3_naming_it_and_dials_nothing` |
| IT-12-11 | a failing migration exits 2 naming its version; a killed one leaves it unapplied and `serve` refuses | command AZ-3793 | pass | `tests/migrate_interrupted.rs::a_failing_migration_exits_2_naming_its_version_and_the_next_run_retries_from_it` |
| IT-12-11 | the irreversible `down` and the release-image refusal of the test migration source | owner E-22/c01 | blocked | the feature `test-migrations` does not exist yet (G-12-1); needs E-22/c01 and E-22/c07 |
| IT-12-12 | exit codes and codes of `key-check`; `keycheck-init` outcomes; no key bytes | command AZ-3797 | pass | `tests/key_commands.rs::key_check_exits_0_on_a_match_and_2_with_the_code_for_every_refusal` |
| IT-12-12 | `.keycheck` is not created on a wrong key | owner E-10/c05 | elsewhere | E-10/c05 |
| IT-12-13 | the rewrap loops to 0 with progress and the keep-the-old-key message; no `--yes` exits 3 | command AZ-3797 | pass | `tests/key_commands.rs::rewrap_loops_to_zero_with_progress_and_a_keep_the_old_key_message` |
| IT-12-13 | blob files unchanged byte for byte; an interrupted run resumes | owner E-10/c13 | elsewhere | E-10/c13 |
| IT-12-13 | downloads hash-equal after a restart with the new key | E-31 | elsewhere | FT-P-41 |
| IT-12-14 | exit 0 and 2 with the counts; progress on stderr; `status`; bad `--rate` exits 3 | command AZ-3798 | pass | `tests/scrub_commands.rs::exit_is_0_without_findings_and_2_for_a_mismatch_or_a_missing_blob_with_the_counts_printed` |
| IT-12-14 | a flipped bit is found; `--rate` timing | owner E-10/c12 | elsewhere | E-10/c12 |
| IT-12-14 | a full scrub beside uploads and collection: exit 0, no false finding | pipeline AZ-3667 | elsewhere | AZ-3667 (E-22/p14) |
| IT-12-15 | levels, four-part findings, same facts in `--json`, options reach every check | command AZ-3807 | pass | `tests/doctor_command.rs::levels_map_to_exit_0_then_1_then_2_and_each_finding_prints_all_four_parts` |
| IT-12-15 | each root check: revisions with a missing blob, quota drift, role, configuration, no secret | command AZ-3809 | pass | `tests/doctor_findings.rs::each_revision_whose_blob_file_is_gone_is_a_fail_with_path_account_and_revision` |
| IT-12-15 | `--pre-flight` never dials the database | command AZ-3809 | pass | `tests/doctor_preflight.rs::pre_flight_lists_the_key_and_volume_checks_and_never_dials_the_database` |
| IT-12-15 | listings of folders with a missing blob answer 200 and 404 `content_unavailable` | pipeline AZ-3666 | elsewhere | AZ-3666 (E-22/p13) AC-73 |
| IT-12-15 | a healthy stack exits 0 | E-31 | elsewhere | AT-12-1; FT-P-69 |
| IT-12-16 | `gc-run` prints the counts of one cycle | command AZ-3796 | pass | `tests/gc_command.rs::gc_run_prints_the_counts_of_one_cycle_and_exits_0_when_nothing_is_due` |
| IT-12-16 | `media-requeue` filters and counts; `media-stats` facts | command AZ-3805 | pass | `tests/media_commands.rs::requeue_with_each_filter_makes_one_call_with_exactly_that_filter_and_prints_the_count` |
| IT-12-16 | requeued rows are `queued` | owner E-18/c05 | elsewhere | E-18/c05 |
| IT-12-16 | the blob is removed after the retention period | pipeline AZ-3663 | elsewhere | AZ-3663 (E-22/p11) AC-1 |
| IT-12-16 | no SQL and no blob path in the CLI crate | owner E-22/c02 | elsewhere | `scripts/ci/check-no-sql-now.sh` (job `rust-lint`) |
| IT-12-17 | `audit export` prints every row since the time as one JSON line, ids only | command AZ-3795 | pass | `tests/audit_export.rs::export_prints_every_row_since_the_time_as_one_json_line_across_two_pages` |
| IT-12-17 | `dev-seed` refuses production and writes nothing | command AZ-3795 | pass | `tests/dev_seed_commands.rs::the_production_refusal_exits_2_and_says_so` |
| IT-12-17 | one `admin_action` event per mutating command, actor `Operator` | owner E-17/c03 | elsewhere | E-17/c03, E-17/c08; E-31 |
| IT-12-18 | `openapi` is byte-identical on every run and equals the served document; `version` | command AZ-3811 | pass | `tests/local_commands.rs::openapi_is_valid_json_equal_to_the_document_the_server_serves` |
| IT-12-18 | `healthcheck` exits 0 on a ready answer and non-zero otherwise | command AZ-3811 | pass | `src/healthcheck.rs::not_ready_answer_is_non_zero` |
| IT-12-18 | `migrate` and `openapi` need no unrelated configuration | suite | pass | `tests/cli_suite.rs::migrate_succeeds_with_only_the_migrator_url` |
| IT-12-18 | `admin setup-token` with no provider and no worker token | suite | pass | `tests/cli_suite.rs::setup_token_succeeds_with_no_provider_and_no_worker_token` |
| IT-12-18 | `key-check` with no provider and no worker token | suite | pass | `tests/cli_suite.rs::key_check_succeeds_with_no_provider_and_no_worker_token` |
| IT-12-19 | a token is on stdout once and nowhere else, across four commands | suite | pass | `tests/cli_suite.rs::each_token_is_on_its_own_stdout_once_and_in_no_stderr_log_or_other_commands_output` |
| IT-12-19 | SIGINT during `admin rewrap`: no new batch, stops within 5 s, exit 130 recorded | command AZ-3797 | pass | `tests/rewrap_sigint.rs::sigint_stops_after_the_running_batch_and_starts_no_new_one` |
| IT-12-19 | SIGINT during `scrub run`: `finish_run` once, stops within 5 s, exit 130 recorded | command AZ-3798 | pass | `tests/scrub_sigint.rs::sigint_stops_after_the_running_step_and_closes_the_run_once` |
