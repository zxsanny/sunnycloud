//! AZ-3813: the cross-command cases of IT-12 at the CLI boundary. The per-command cases live in the command tasks'
//! test files; this file owns what no single command task proves: one exit and JSON rule for every command, the
//! single-business-call check over the contract's command table, the secret channel across commands, and the
//! coverage map of IT-12-1..19 (`it12_coverage_map.md`, kept beside this file).

mod config_support;
mod support;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use sc_error::{BusinessError, BusinessParams, DomainError, ErrorCode};
use serde_json::Value;
use sunnycloud_server::{Class, CliError, OneTimeSecret, Outcome};
use support::{COMMANDS, Harness, answer};

fn words(line: &str) -> Vec<&str> {
    line.split_whitespace().collect()
}

fn owner_refusal() -> CliError {
    DomainError::Business(BusinessError::new(
        ErrorCode::LastAdmin,
        BusinessParams::default(),
    ))
    .into()
}

fn one_object(stdout: &str) -> serde_json::Map<String, Value> {
    assert_eq!(stdout.lines().count(), 1, "one line: {stdout:?}");
    match serde_json::from_str::<Value>(stdout).expect("stdout is JSON") {
        Value::Object(object) => object,
        other => panic!("not an object: {other}"),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// AC-1 (IT-12-1): the exit and JSON rules, for every command of the contract.
// ---------------------------------------------------------------------------------------------------------------

#[test]
fn every_command_exits_0_on_success_and_prints_one_envelope_with_json() {
    for (line, route) in COMMANDS {
        let mut args = vec!["--json"];
        args.extend(words(line));

        let result = Harness::new().run(&args);

        assert_eq!(result.exit, 0, "{line}: {}", result.stderr);
        let object = one_object(&result.stdout);
        assert_eq!(object["result"], "ok", "{line}");
        assert_eq!(object["code"], "ok", "{line}");
        assert!(object["data"].is_object(), "{line}");
        assert_eq!(result.calls, vec![(*route).to_owned()], "{line}");
    }
}

#[test]
fn every_command_exits_2_on_an_owner_refusal_with_the_owners_code_in_the_envelope() {
    for (line, _) in COMMANDS {
        // `serve` installs the process's one log subscriber; the success test above runs it once.
        if *line == "serve" {
            continue;
        }
        let mut args = vec!["--json"];
        args.extend(words(line));

        let result = Harness::new()
            .responder(answer(|| Err(owner_refusal())))
            .run(&args);

        assert_eq!(result.exit, 2, "{line}: {}", result.stderr);
        let object = one_object(&result.stdout);
        assert_eq!(object["code"], "last_admin", "{line}");
        assert_eq!(object["result"], "failed", "{line}");
    }
}

#[test]
fn doctor_exits_0_for_ok_1_for_warnings_and_2_for_a_failure_and_json_names_the_level() {
    let cases = [
        (Class::Success, 0, "ok"),
        (Class::Warnings, 1, "warnings"),
        (Class::Findings, 2, "findings"),
    ];

    for (class, exit, name) in cases {
        let text = Harness::new()
            .responder(answer(move || Ok(Outcome::new(class))))
            .run(&["doctor"]);
        let json = Harness::new()
            .responder(answer(move || Ok(Outcome::new(class))))
            .run(&["--json", "doctor"]);

        assert_eq!(text.exit, exit, "{name}");
        assert_eq!(json.exit, exit, "{name}");
        assert_eq!(one_object(&json.stdout)["result"], name);
    }
}

#[test]
fn usage_errors_exit_3_with_no_business_call_and_nothing_on_stdout_even_with_json() {
    let bad: [&[&str]; 6] = [
        &["key", "rewrap"],
        &["admin", "reset"],
        &["admin", "user", "set-quota", "alice"],
        &["admin", "restore", "--account", "alice"],
        &["scrub"],
        &["nonsense"],
    ];

    for args in bad {
        for json in [false, true] {
            let mut full = if json { vec!["--json"] } else { vec![] };
            full.extend(args);

            let result = Harness::new().run(&full);

            assert_eq!(result.exit, 3, "{full:?}");
            assert!(result.calls.is_empty(), "{full:?}");
            assert_eq!(result.stdout, "", "{full:?}");
            assert!(!result.stderr.is_empty(), "{full:?}");
        }
    }
}

#[test]
fn the_four_destructive_commands_need_yes_on_a_non_tty_and_change_nothing_without_it() {
    let destructive: [&[&str]; 4] = [
        &["admin", "user", "delete", "alice"],
        &["admin", "rewrap", "--from-key-id", "1"],
        &["admin", "epoch-bump"],
        &["migrate", "revert"],
    ];

    for args in destructive {
        for json in [false, true] {
            let mut refused = if json { vec!["--json"] } else { vec![] };
            refused.extend(args);
            let mut confirmed = refused.clone();
            confirmed.push("--yes");

            let without = Harness::new().run(&refused);
            let with = Harness::new().run(&confirmed);

            assert_eq!(without.exit, 3, "{refused:?}");
            assert!(without.calls.is_empty(), "{refused:?}");
            assert_eq!(without.stdout, "", "{refused:?}");
            assert_eq!(with.exit, 0, "{confirmed:?}: {}", with.stderr);
            assert_eq!(with.calls.len(), 1, "{confirmed:?}");
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// AC-2: one business operation per command, checked against the contract's own command table.
// ---------------------------------------------------------------------------------------------------------------

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

fn crate_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()))
}

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

// ---------------------------------------------------------------------------------------------------------------
// AC-7: the coverage map assigns every expected result of IT-12-1..19 to exactly one place.
// ---------------------------------------------------------------------------------------------------------------

const PLACES: [&str; 5] = ["suite", "command AZ-", "pipeline AZ-", "owner ", "E-31"];
const STATUSES: [&str; 4] = ["pass", "known failure", "blocked", "elsewhere"];

struct Row {
    case: String,
    part: String,
    place: String,
    status: String,
    proof: String,
}

fn map_rows(map: &str) -> Vec<Row> {
    map.lines()
        .filter(|line| line.starts_with("| IT-12-"))
        .map(|line| {
            let cells: Vec<String> = line
                .trim_matches('|')
                .split(" | ")
                .map(|cell| cell.trim().to_owned())
                .collect();
            assert_eq!(cells.len(), 5, "a map row has five cells: {line}");
            Row {
                case: cells[0].clone(),
                part: cells[1].clone(),
                place: cells[2].clone(),
                status: cells[3].clone(),
                proof: cells[4].clone(),
            }
        })
        .collect()
}

/// The test a proof names as `` `path::name` ``: its file text and the attributes just above it.
fn named_test(proof: &str) -> Option<(String, String, bool)> {
    let token = proof
        .split('`')
        .nth(1)
        .filter(|token| token.contains("::"))?;
    let (path, name) = token.split_once("::")?;
    let text = read(&crate_dir().join(path));
    let at = text.find(&format!("fn {name}("))?;
    let above = text[..at].rsplit("\n\n").next().unwrap_or("");
    Some((path.to_owned(), name.to_owned(), above.contains("#[ignore")))
}

#[test]
fn every_case_of_the_spec_has_rows_each_part_has_one_place_and_each_named_test_exists() {
    // Arrange
    let spec =
        read(&crate_dir().join("../../_docs/02_document/components/12_operator_cli/tests.md"));
    let cases: Vec<String> = (1..=19).map(|number| format!("IT-12-{number}")).collect();
    let rows = map_rows(&read(&crate_dir().join("tests/it12_coverage_map.md")));

    // Act
    let unknown: Vec<&str> = rows
        .iter()
        .map(|row| row.case.as_str())
        .filter(|case| !cases.iter().any(|c| c == case))
        .collect();
    let uncovered: Vec<&String> = cases
        .iter()
        .filter(|case| !rows.iter().any(|row| &row.case == *case))
        .collect();
    let missing_heading: Vec<&String> = cases
        .iter()
        .filter(|case| !spec.contains(&format!("### {case}:")))
        .collect();
    let mut seen = BTreeSet::new();
    let twice: Vec<String> = rows
        .iter()
        .filter(|row| !seen.insert((row.case.clone(), row.part.clone())))
        .map(|row| format!("{}: {}", row.case, row.part))
        .collect();
    let bad_place: Vec<String> = rows
        .iter()
        .filter(|row| !PLACES.iter().any(|place| row.place.starts_with(place)))
        .map(|row| format!("{}: {}", row.case, row.place))
        .collect();
    let bad_status: Vec<String> = rows
        .iter()
        .filter(|row| !STATUSES.contains(&row.status.as_str()))
        .map(|row| format!("{}: {}", row.case, row.status))
        .collect();
    let mut wrong_proof = Vec::new();
    for row in rows
        .iter()
        .filter(|row| row.status == "pass" || row.status == "known failure")
    {
        match named_test(&row.proof) {
            None => wrong_proof.push(format!(
                "{} / {}: no existing test named",
                row.case, row.part
            )),
            Some((path, name, ignored)) => {
                if ignored != (row.status == "known failure") {
                    wrong_proof.push(format!(
                        "{path}::{name}: ignored is {ignored}, status is {}",
                        row.status
                    ));
                }
            }
        }
    }

    // Assert
    assert!(
        missing_heading.is_empty(),
        "the spec lacks {missing_heading:?}"
    );
    assert!(
        unknown.is_empty(),
        "rows for cases the spec lacks: {unknown:?}"
    );
    assert!(uncovered.is_empty(), "no row for {uncovered:?}");
    assert!(twice.is_empty(), "a part is assigned twice: {twice:?}");
    assert!(bad_place.is_empty(), "not an allowed place: {bad_place:?}");
    assert!(
        bad_status.is_empty(),
        "not an allowed status: {bad_status:?}"
    );
    assert!(wrong_proof.is_empty(), "{wrong_proof:?}");
}

// ---------------------------------------------------------------------------------------------------------------
// AC-3 (IT-12-19, ST-12-1): a one-time secret is on stdout once and nowhere else, across commands.
// ---------------------------------------------------------------------------------------------------------------

const SECRET_COMMANDS: [(&str, &str); 4] = [
    ("admin setup-token", "at_1111111111111111111111111111111a"),
    ("admin create", "at_2222222222222222222222222222222b"),
    ("admin reset alice", "at_3333333333333333333333333333333c"),
    (
        "admin invite --admin alice create",
        "at_4444444444444444444444444444444d",
    ),
];

fn issue(line: &str, token: &'static str, json: bool) -> support::Captured {
    let mut args = if json { vec!["--json"] } else { vec![] };
    args.extend(words(line));
    Harness::new()
        .log_json()
        .responder(answer(move || {
            Ok(Outcome::success()
                .field("hint", "sign in")
                .with_secret(OneTimeSecret::new("token", token.to_owned())))
        }))
        .run(&args)
}

#[test]
fn each_token_is_on_its_own_stdout_once_and_in_no_stderr_log_or_other_commands_output() {
    for json in [false, true] {
        // Arrange and Act
        let runs: Vec<_> = SECRET_COMMANDS
            .iter()
            .map(|(line, token)| (issue(line, token, json), *token))
            .collect();

        // Assert
        for (index, (run, token)) in runs.iter().enumerate() {
            assert_eq!(run.exit, 0, "{json}: {}", run.stderr);
            assert_eq!(run.stdout.matches(token).count(), 1, "{token}");
            assert_eq!(run.stderr.matches(token).count(), 0, "{token}");
            assert_eq!(
                run.log.len(),
                1,
                "the log is live, so the check is not vacuous"
            );
            assert_eq!(
                run.log
                    .iter()
                    .map(|line| line.matches(token).count())
                    .sum::<usize>(),
                0
            );
            for (other, (elsewhere, _)) in
                runs.iter().enumerate().filter(|(other, _)| *other != index)
            {
                let output = format!(
                    "{}{}{}",
                    elsewhere.stdout,
                    elsewhere.stderr,
                    elsewhere.log.join("")
                );
                assert_eq!(
                    output.matches(token).count(),
                    0,
                    "{token} leaked into command {other}"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// AC-5 (IT-12-18): lazy construction. Each command reads only the keys and builds only the services it needs.
// ---------------------------------------------------------------------------------------------------------------

mod lazy {
    use std::process::{Command, Output, Stdio};

    use super::config_support::Setup;
    use sc_testdb::TestDb;

    /// The binary with the given environment only: no provider, no worker token, nothing else.
    fn run(setup: &Setup, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_sunnycloud"))
            .args(args)
            .env_clear()
            .envs(std::env::var("LLVM_PROFILE_FILE").map(|path| ("LLVM_PROFILE_FILE", path)))
            .envs(&setup.env)
            .stdin(Stdio::null())
            .output()
            .expect("the binary runs")
    }

    fn text(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes).into_owned()
    }

    #[tokio::test]
    async fn migrate_succeeds_with_only_the_migrator_url() {
        // Arrange
        let db = TestDb::clone_empty().await;
        let mut setup = Setup::empty();
        let url = db
            .config()
            .db_migrator_url
            .expect("a migrator URL")
            .expose()
            .clone();
        setup.secret("db_migrator_url", &url);

        // Act
        let out = tokio::task::spawn_blocking(move || run(&setup, &["migrate"]))
            .await
            .expect("the run");

        // Assert
        assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
        db.drop_db().await;
    }

    /// KNOWN FAILURE (leftover `2026-11-24_az-3813-lazy-config.md`): every service of the composition root reads
    /// `ServeSettings`, which binds and validates every key of the server, so `admin setup-token` refuses with
    /// "providers, share_grant_key, blob_root, worker_api_token, master_key_file: required setting is missing".
    #[tokio::test]
    #[ignore = "AZ-3813 AC-5 fails today: admin setup-token needs the whole server config (leftover 2026-11-24_az-3813-lazy-config.md)"]
    async fn setup_token_succeeds_with_no_provider_and_no_worker_token() {
        // Arrange
        let db = TestDb::clone_template().await;
        let mut setup = Setup::empty();
        setup.secret("db_api_url", db.config().db_api_url.expose());
        setup.set("PUBLIC_URL", "https://cloud.example.org");

        // Act
        let out = tokio::task::spawn_blocking(move || run(&setup, &["admin", "setup-token"]))
            .await
            .expect("the run");

        // Assert
        assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
        db.drop_db().await;
    }

    /// KNOWN FAILURE, same cause: `key-check` builds the keyring from `ServeSettings`.
    #[tokio::test]
    #[ignore = "AZ-3813 AC-5 fails today: key-check needs the whole server config (leftover 2026-11-24_az-3813-lazy-config.md)"]
    async fn key_check_succeeds_with_no_provider_and_no_worker_token() {
        // Arrange
        let db = TestDb::clone_template().await;
        let mut setup = Setup::empty();
        setup.secret("db_api_url", db.config().db_api_url.expose());
        setup.secret("master_key_file", &"k".repeat(32));
        setup.set("BLOB_ROOT", &setup.dir.join("blobs").display().to_string());

        // Act
        let out = tokio::task::spawn_blocking(move || run(&setup, &["key-check"]))
            .await
            .expect("the run");

        // Assert
        assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
        db.drop_db().await;
    }

    #[test]
    fn openapi_succeeds_with_no_database_and_no_key() {
        // Arrange
        let setup = Setup::empty();

        // Act
        let out = run(&setup, &["openapi"]);

        // Assert
        assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
        assert!(serde_json::from_slice::<serde_json::Value>(&out.stdout).is_ok());
    }
}
