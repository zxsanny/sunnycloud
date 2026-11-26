//! AC-3 (IT-12-19, ST-12-1): a one-time secret is on stdout once and nowhere else, across commands.

use sunnycloud_server::{OneTimeSecret, Outcome};

use crate::common::words;
use crate::support::{self, Harness, answer};

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
