//! AC-1 and AC-2: every command of the contract parses; the old names and bad invocations are usage errors.

mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use sunnycloud_server::ServiceContext;
use support::Harness;

/// The command table of `operator_command_surface.md`: minimal valid arguments and the route each logs under.
const COMMANDS: &[(&str, &str)] = &[
    ("serve", "/cli/serve"),
    ("migrate", "/cli/migrate"),
    ("migrate run", "/cli/migrate"),
    ("migrate status", "/cli/migrate-status"),
    ("migrate revert --steps 2 --yes", "/cli/migrate-revert"),
    ("admin setup-token", "/cli/admin-setup-token"),
    ("admin create --note first --ttl 24h", "/cli/admin-create"),
    ("admin reset alice", "/cli/admin-reset"),
    ("admin promote alice", "/cli/admin-promote"),
    ("admin demote alice", "/cli/admin-demote"),
    ("admin invite create --note n", "/cli/admin-invite-create"),
    ("admin invite list", "/cli/admin-invite-list"),
    ("admin invite revoke inv-1", "/cli/admin-invite-revoke"),
    ("admin user list", "/cli/admin-user-list"),
    (
        "admin user set-quota alice 10GiB",
        "/cli/admin-user-set-quota",
    ),
    ("admin user delete alice --yes", "/cli/admin-user-delete"),
    ("admin dev-seed", "/cli/admin-dev-seed"),
    (
        "admin audit export --since 2026-01-01T00:00:00Z",
        "/cli/admin-audit-export",
    ),
    ("admin epoch-bump --yes", "/cli/admin-epoch-bump"),
    (
        "admin restore --account alice --path /a/b.txt --at 2026-01-01T00:00:00Z --dry-run",
        "/cli/admin-restore",
    ),
    ("admin gc-run", "/cli/admin-gc-run"),
    ("key-check", "/cli/key-check"),
    ("admin keycheck-init", "/cli/admin-keycheck-init"),
    (
        "admin rewrap --new-key-file /run/new.key --yes",
        "/cli/admin-rewrap",
    ),
    ("scrub run --full --rate 50", "/cli/scrub-run"),
    ("scrub status", "/cli/scrub-status"),
    (
        "admin media-requeue --account alice --kind thumbnail",
        "/cli/admin-media-requeue",
    ),
    ("admin media-stats", "/cli/admin-media-stats"),
    (
        "doctor --pre-flight --backup-interval 24h --attest-backup",
        "/cli/doctor",
    ),
    ("healthcheck", "/cli/healthcheck"),
    ("openapi", "/cli/openapi"),
    ("version", "/cli/version"),
];

fn words(line: &str) -> Vec<&str> {
    line.split_whitespace().collect()
}

#[test]
fn every_contract_command_parses_and_reaches_its_port_once() {
    for (line, route) in COMMANDS {
        let result = Harness::new().run(&words(line));

        assert_eq!(result.exit, 0, "{line}: {}", result.stderr);
        assert_eq!(result.calls, vec![(*route).to_owned()], "{line}");
    }
}

#[test]
fn every_contract_command_has_help() {
    for (line, _) in COMMANDS {
        let command: Vec<&str> = words(line)
            .into_iter()
            .take_while(|word| !word.starts_with("--"))
            .collect();
        let mut args = command.clone();
        args.push("--help");

        let result = Harness::new().run(&args);

        assert_eq!(result.exit, 0, "{line}: {}", result.stderr);
        assert!(
            result.stdout.contains("Usage:"),
            "{line}: {}",
            result.stdout
        );
        assert!(result.calls.is_empty(), "{line}");
    }
}

#[test]
fn help_lists_the_final_names_and_no_old_ones() {
    let top = Harness::new().run(&["--help"]).stdout;
    let admin = Harness::new().run(&["admin", "--help"]).stdout;

    for name in [
        "serve",
        "migrate",
        "doctor",
        "key-check",
        "admin",
        "scrub",
        "healthcheck",
        "openapi",
        "version",
    ] {
        assert!(top.contains(name), "top level lacks {name}");
    }
    for name in [
        "media-requeue",
        "media-stats",
        "restore",
        "rewrap",
        "keycheck-init",
        "epoch-bump",
        "gc-run",
    ] {
        assert!(admin.contains(name), "admin lacks {name}");
    }
    assert!(!top.contains("rewrap") && !top.contains("requeue"));
}

#[test]
fn the_old_names_are_usage_errors_without_a_business_call() {
    for line in [
        "key rewrap",
        "media requeue",
        "restore",
        "restore --account a --path /p --at 2026-01-01T00:00:00Z",
    ] {
        let result = Harness::new().run(&words(line));

        assert_eq!(result.exit, 3, "{line}");
        assert!(
            result.stderr.contains("Usage:"),
            "{line}: {}",
            result.stderr
        );
        assert_eq!(result.stdout, "", "{line}");
        assert!(result.calls.is_empty(), "{line}");
    }
}

#[test]
fn unknown_command_and_missing_arguments_exit_3_and_build_no_service() {
    let builds = Arc::new(AtomicUsize::new(0));
    for line in [
        "frobnicate",
        "",
        "admin promote",
        "admin user set-quota alice",
        "admin user set-quota alice lots",
        "admin restore --account a",
        "admin rewrap --yes",
        "migrate revert --steps 0 --yes",
        "doctor --backup-interval soon",
    ] {
        let counter = Arc::clone(&builds);
        let harness = Harness::new().service::<String, _, _>(move |_: ServiceContext| {
            counter.fetch_add(1, Ordering::SeqCst);
            async { Ok(Arc::new(String::new())) }
        });

        let result = harness.run(&words(line));

        assert_eq!(result.exit, 3, "{line:?}");
        assert!(
            result.stderr.contains("Usage:") || result.stderr.contains("error:"),
            "{line:?}: {}",
            result.stderr
        );
        assert_eq!(result.stdout, "", "{line:?}");
        assert!(result.calls.is_empty(), "{line:?}");
    }

    assert_eq!(builds.load(Ordering::SeqCst), 0);
}
