//! AC-5: the destructive-command guard.

mod support;

use support::Harness;

const DESTRUCTIVE: &[(&[&str], &str)] = &[
    (
        &["admin", "user", "delete", "alice"],
        "/cli/admin-user-delete",
    ),
    (
        &["admin", "rewrap", "--new-key-file", "/run/new.key"],
        "/cli/admin-rewrap",
    ),
    (&["admin", "epoch-bump"], "/cli/admin-epoch-bump"),
    (&["migrate", "revert"], "/cli/migrate-revert"),
];

fn with_yes(args: &[&str]) -> Vec<String> {
    args.iter()
        .map(ToString::to_string)
        .chain(["--yes".to_owned()])
        .collect()
}

#[test]
fn non_tty_without_yes_exits_3_with_zero_calls_and_with_yes_makes_one() {
    for (args, route) in DESTRUCTIVE {
        let refused = Harness::new().run(args);
        let confirmed_args = with_yes(args);
        let confirmed = Harness::new().run(
            &confirmed_args
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        );

        assert_eq!(refused.exit, 3, "{args:?}");
        assert!(refused.calls.is_empty(), "{args:?}");
        assert!(
            refused.stderr.contains("--yes"),
            "{args:?}: {}",
            refused.stderr
        );
        assert_eq!(refused.stdout, "", "{args:?}");
        assert_eq!(confirmed.exit, 0, "{args:?}: {}", confirmed.stderr);
        assert_eq!(confirmed.calls, vec![(*route).to_owned()], "{args:?}");
    }
}

#[test]
fn a_tty_prompt_names_the_target_and_only_a_confirming_answer_calls() {
    for answer in ["y\n", "yes\n", "YES\r\n"] {
        let result = Harness::new()
            .tty(answer)
            .run(&["admin", "user", "delete", "alice"]);

        assert_eq!(result.exit, 0, "{answer:?}");
        assert!(
            result.stderr.contains("alice"),
            "prompt names the target: {}",
            result.stderr
        );
        assert_eq!(result.calls, ["/cli/admin-user-delete"]);
    }
    for answer in ["n\n", "\n", "no\n", "maybe\n", ""] {
        let result = Harness::new()
            .tty(answer)
            .run(&["admin", "user", "delete", "alice"]);

        assert_eq!(result.exit, 3, "{answer:?}");
        assert!(result.stderr.contains("alice"), "{answer:?}");
        assert!(result.calls.is_empty(), "{answer:?}");
    }
}

#[test]
fn yes_on_a_tty_skips_the_prompt() {
    let result = Harness::new()
        .tty("")
        .run(&["admin", "user", "delete", "alice", "--yes"]);

    assert_eq!(result.exit, 0);
    assert_eq!(result.stderr, "");
    assert_eq!(result.calls.len(), 1);
}

#[test]
fn switching_the_requirement_off_lets_a_non_tty_run_without_yes() {
    let result = Harness::new()
        .confirm_required(false)
        .run(&["admin", "epoch-bump"]);

    assert_eq!((result.exit, result.calls.len()), (0, 1));
}

#[test]
fn commands_that_are_not_destructive_never_ask() {
    let result = Harness::new()
        .tty("")
        .run(&["admin", "user", "set-quota", "alice", "1GiB"]);

    assert_eq!(
        (result.exit, result.calls.len(), result.stderr.as_str()),
        (0, 1, "")
    );
}
