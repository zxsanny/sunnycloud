//! AC-6: a one-time secret is printed once, on stdout only.

mod support;

use sunnycloud_server::{OneTimeSecret, Outcome};
use support::{Harness, answer};

const TOKEN: &str = "at_7f3a9c1e2b4d6f80a1b2c3d4e5f60718";

fn issue() -> Harness {
    Harness::new().log_json().responder(answer(|| {
        Ok(Outcome::success()
            .field("hint", "sign in at /setup")
            .with_secret(OneTimeSecret::new("token", TOKEN.to_owned())))
    }))
}

fn count(haystack: &str) -> usize {
    haystack.matches(TOKEN).count()
}

#[test]
fn text_mode_prints_the_token_once_on_stdout() {
    let result = issue().run(&["admin", "create"]);

    assert_eq!(result.exit, 0);
    assert_eq!(count(&result.stdout), 1);
    assert_eq!(count(&result.stderr), 0);
    assert_eq!(result.log.iter().map(|line| count(line)).sum::<usize>(), 0);
    assert_eq!(
        result.log.len(),
        1,
        "the log is live, so the check is not vacuous"
    );
}

#[test]
fn json_mode_prints_the_token_once_inside_data() {
    let result = issue().run(&["--json", "admin", "create"]);

    let object: serde_json::Value = serde_json::from_str(&result.stdout).expect("one JSON object");
    assert_eq!(object["data"]["token"], TOKEN);
    assert_eq!(count(&result.stdout), 1);
    assert_eq!(count(&result.stderr), 0);
    assert_eq!(result.log.iter().map(|line| count(line)).sum::<usize>(), 0);
    assert_eq!(result.log.len(), 1);
}

#[test]
fn the_secret_type_redacts_itself() {
    let secret = OneTimeSecret::new("token", TOKEN.to_owned());
    let outcome = Outcome::success().with_secret(OneTimeSecret::new("token", TOKEN.to_owned()));

    assert!(!format!("{secret:?}").contains(TOKEN));
    assert!(!format!("{outcome:?}").contains(TOKEN));
}

#[test]
fn a_refusal_prints_no_secret() {
    let result = Harness::new()
        .responder(answer(|| {
            Err(sunnycloud_server::CliError::Usage("nothing issued".into()))
        }))
        .run(&["admin", "setup-token"]);

    assert_eq!(count(&result.stdout) + count(&result.stderr), 0);
}
