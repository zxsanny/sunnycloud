//! AC-2: one business operation per command, checked against the contract's own command table.

use std::collections::BTreeSet;

use crate::common::{crate_dir, read};
use crate::support::COMMANDS;

/// (command as the contract names it, test file under `tests/` or a path under the crate, the test that proves one
/// business operation). A command with a recording owner port counts the calls of that port; a command whose owner
/// is the real component (`migrate`, `admin gc-run`, `admin audit export`, `doctor`, the local commands) shows its one
/// run through the owner's own effect.
const ONE_OPERATION: &[(&str, &str, &str)] = &[
    (
        "serve",
        "tests/serve_start.rs",
        "start_steps_are_logged_in_order_and_readyz_is_200_within_10s",
    ),
    (
        "migrate",
        "tests/migrate.rs",
        "a_fresh_database_is_migrated_once_and_the_second_run_applies_nothing",
    ),
    (
        "migrate status",
        "tests/migrate.rs",
        "status_works_as_the_api_role_and_lists_applied_and_pending",
    ),
    (
        "migrate revert",
        "tests/migrate.rs",
        "revert_without_yes_on_a_non_tty_exits_3_and_with_yes_reverts_one",
    ),
    (
        "admin setup-token",
        "tests/identity_commands.rs",
        "setup_token_prints_the_token_once_with_the_hint_on_each_run",
    ),
    (
        "admin create",
        "tests/identity_commands.rs",
        "create_makes_one_call_and_prints_the_token_once_in_text_and_json",
    ),
    (
        "admin reset",
        "tests/identity_commands.rs",
        "reset_prints_a_fresh_invite_once_and_maps_the_refusals",
    ),
    (
        "admin promote",
        "tests/identity_commands.rs",
        "promote_and_demote_call_update_user_as_the_operator_and_the_last_admin_exits_2",
    ),
    (
        "admin demote",
        "tests/identity_commands.rs",
        "promote_and_demote_call_update_user_as_the_operator_and_the_last_admin_exits_2",
    ),
    (
        "admin invite",
        "tests/identity_commands.rs",
        "invite_create_list_and_revoke_make_one_call_each_and_list_shows_no_token",
    ),
    (
        "admin user list",
        "tests/user_commands.rs",
        "user_list_reads_every_page_as_the_operator_and_prints_role_state_used_and_limit",
    ),
    (
        "admin user set-quota",
        "tests/user_commands.rs",
        "set_quota_makes_one_call_with_the_size_and_a_bad_size_exits_3_with_no_call",
    ),
    (
        "admin user delete",
        "tests/user_commands.rs",
        "delete_without_yes_exits_3_with_no_call_and_with_yes_makes_one_call_and_prints_the_deadline",
    ),
    (
        "admin dev-seed",
        "tests/dev_seed_commands.rs",
        "a_development_run_makes_one_seed_call_and_exits_0",
    ),
    (
        "admin audit export",
        "tests/audit_export.rs",
        "export_prints_every_row_since_the_time_as_one_json_line_across_two_pages",
    ),
    (
        "admin epoch-bump",
        "tests/maintenance_commands.rs",
        "epoch_bump_calls_once_with_the_clock_then_runs_only_the_restore_checks_and_exits_0",
    ),
    (
        "admin restore",
        "tests/maintenance_commands.rs",
        "a_restore_prints_the_revision_and_placement_and_exits_0",
    ),
    (
        "admin gc-run",
        "tests/gc_command.rs",
        "gc_run_prints_the_counts_of_one_cycle_and_exits_0_when_nothing_is_due",
    ),
    (
        "key-check",
        "tests/key_commands.rs",
        "key_check_exits_0_on_a_match_and_2_with_the_code_for_every_refusal",
    ),
    (
        "admin keycheck-init",
        "tests/key_commands.rs",
        "keycheck_init_passes_the_configured_key_file_and_maps_the_outcomes",
    ),
    (
        "admin rewrap",
        "tests/key_commands.rs",
        "rewrap_loops_to_zero_with_progress_and_a_keep_the_old_key_message",
    ),
    (
        "scrub run",
        "tests/scrub_commands.rs",
        "full_with_a_rate_and_plain_runs_call_start_once_step_until_done_and_finish_once",
    ),
    (
        "scrub status",
        "tests/scrub_commands.rs",
        "status_prints_the_same_facts_in_text_and_json",
    ),
    (
        "admin media-requeue",
        "tests/media_commands.rs",
        "requeue_with_each_filter_makes_one_call_with_exactly_that_filter_and_prints_the_count",
    ),
    (
        "admin media-stats",
        "tests/media_commands.rs",
        "stats_print_every_lane_and_status_with_the_same_facts_in_text_and_json",
    ),
    (
        "doctor",
        "tests/doctor_command.rs",
        "levels_map_to_exit_0_then_1_then_2_and_each_finding_prints_all_four_parts",
    ),
    (
        "healthcheck",
        "src/healthcheck.rs",
        "ready_answer_is_exit_zero",
    ),
    (
        "openapi",
        "tests/local_commands.rs",
        "openapi_prints_the_same_bytes_every_run",
    ),
    (
        "version",
        "tests/local_commands.rs",
        "version_prints_a_semver_and_the_git_commit_without_configuration",
    ),
];

/// The commands of the contract's `### Commands` table, each as its leading words (`migrate [run]` is `migrate`,
/// `admin invite --admin <account> create|list|revoke` is `admin invite`).
fn contract_commands(contract: &str) -> BTreeSet<String> {
    let table = contract
        .split("### Commands")
        .nth(1)
        .and_then(|rest| rest.split("\n## ").next())
        .expect("the contract has a Commands section");
    let mut found = BTreeSet::new();
    for row in table.lines().filter(|line| line.starts_with("| `")) {
        let first_cell = row
            .trim_start_matches("| ")
            .split(" | ")
            .next()
            .unwrap_or("");
        for (index, segment) in first_cell.split('`').enumerate() {
            if index % 2 == 0 {
                continue;
            }
            let lead: Vec<&str> = segment
                .split_whitespace()
                .take_while(|word| !word.starts_with(['[', '<', '-']))
                .collect();
            found.insert(lead.join(" "));
        }
    }
    found
}

/// Commands of the contract that have no proof row.
fn commands_without_a_row(
    contract: &BTreeSet<String>,
    table: &[(&str, &str, &str)],
) -> Vec<String> {
    contract
        .iter()
        .filter(|command| !table.iter().any(|(name, _, _)| name == command))
        .cloned()
        .collect()
}

/// Rows whose test does not exist in its file.
fn rows_without_a_test(table: &[(&str, &str, &str)]) -> Vec<String> {
    table
        .iter()
        .filter(|(_, file, test)| !read(&crate_dir().join(file)).contains(&format!("fn {test}(")))
        .map(|(command, file, test)| format!("{command}: {file}::{test}"))
        .collect()
}

#[test]
fn every_command_of_the_contract_has_a_one_operation_test_that_exists() {
    // Arrange
    let contract = contract_commands(&read(
        &crate_dir()
            .join("../../_docs/02_document/contracts/12_operator_cli/operator_command_surface.md"),
    ));

    // Act
    let unproven = commands_without_a_row(&contract, ONE_OPERATION);
    let dangling = rows_without_a_test(ONE_OPERATION);

    // Assert
    assert!(
        contract.len() >= 28,
        "the contract table was not read: {contract:?}"
    );
    assert!(
        unproven.is_empty(),
        "no one-operation test for {unproven:?}"
    );
    assert!(
        dangling.is_empty(),
        "the named test is missing: {dangling:?}"
    );
}

#[test]
fn every_command_of_the_contract_is_in_the_route_table_that_counts_one_call() {
    // Arrange
    let contract = contract_commands(&read(
        &crate_dir()
            .join("../../_docs/02_document/contracts/12_operator_cli/operator_command_surface.md"),
    ));

    // Act
    let missing: Vec<&String> = contract
        .iter()
        .filter(|command| {
            !COMMANDS
                .iter()
                .any(|(line, _)| line.starts_with(command.as_str()))
        })
        .collect();

    // Assert
    assert!(missing.is_empty(), "not in the command table: {missing:?}");
}

#[test]
fn the_check_fails_for_a_command_without_a_row_and_for_a_row_without_a_test() {
    // Arrange
    let contract: BTreeSet<String> = ["scrub run", "scrub status"].map(String::from).into();
    let partial = [(
        "scrub run",
        "tests/scrub_commands.rs",
        "status_prints_the_same_facts_in_text_and_json",
    )];
    let dangling = [("scrub status", "tests/scrub_commands.rs", "no_such_test")];

    // Act and Assert
    assert_eq!(
        commands_without_a_row(&contract, &partial),
        ["scrub status"]
    );
    assert_eq!(rows_without_a_test(&dangling).len(), 1);
    assert!(rows_without_a_test(&partial).is_empty());
}
