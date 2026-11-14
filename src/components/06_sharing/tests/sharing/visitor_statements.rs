//! IT-06-9 constant-work lookup and the confinement of system scope (AZ-3698 AC-4, AC-6): the statement log of
//! `open` in the five not-found cases is the same (the lookup by `token_hash`, then the two primary-key target
//! reads), and so is that of `authorize_content` (the lookup, then its one target read; AZ-3700), the system-scope
//! read is the one `run_system` call of the crate, and no visitor path reaches Argon2.

use std::path::Path;

use contracts::{PublicShareError, ShareOpen};
use sc_ids::RawToken;
use sc_testdb::statements::StatementLog;
use sunnycloud_sharing::PgShareService;

use crate::harness::Harness;
use crate::visitor_support::{VISITOR, cases, malformed_tokens, unknown_token};

/// The data statements of one `open`, with its answer.
async fn logged_open(
    shares: &PgShareService,
    log: &StatementLog,
    token: &RawToken,
) -> (ShareOpen, Vec<String>) {
    log.take();
    let answer = shares.open(token, VISITOR).await.expect("open");
    (answer, log.take_data_statements())
}

#[tokio::test]
async fn the_five_not_found_cases_run_the_same_statements_as_each_other() {
    // Arrange
    let harness = Harness::new().await;
    let cases = cases(&harness).await;
    let log = StatementLog::install();
    let (unknown, [empty, long, non_ascii]) = (unknown_token(), malformed_tokens());
    let dead = [
        &unknown,
        &cases.expired,
        &cases.revoked,
        &cases.tombstoned,
        &cases.purged,
        &cases.expired_password,
        &empty,
        &long,
        &non_ascii,
    ];

    // Act
    let mut logged = Vec::new();
    for token in dead {
        logged.push(logged_open(&cases.shares, &log, token).await);
    }
    let (live_answer, live) = logged_open(&cases.shares, &log, &cases.live).await;

    // Assert
    let (_, reference) = &logged[0];
    assert_eq!(reference.len(), 3, "{reference:#?}");
    assert!(reference[0].contains("FROM share_links WHERE token_hash = $1"));
    assert!(reference[1].contains("WHERE n.id = $1 AND n.account_id = $2"));
    assert!(reference[2].contains("WHERE n.id = $1 AND n.account_id = $2"));
    for (answer, statements) in &logged {
        assert_eq!(*answer, ShareOpen::NotFound);
        assert_eq!(statements, reference);
    }
    assert!(matches!(live_answer, ShareOpen::Available { .. }));
    // A live target adds 02's ancestor walk inside each of the same two reads (a discovery of AZ-3698).
    assert_eq!(live.len(), 5, "{live:#?}");
    let live_without_ancestor_walk: Vec<String> = live
        .into_iter()
        .filter(|sql| !sql.starts_with("WITH RECURSIVE chain"))
        .collect();
    assert_eq!(&live_without_ancestor_walk, reference);
    drop(cases);
    harness.finish().await;
}

#[tokio::test]
async fn the_not_found_cases_of_authorize_content_run_the_same_statements_as_each_other() {
    // Arrange
    let harness = Harness::new().await;
    let cases = cases(&harness).await;
    let log = StatementLog::install();
    let (unknown, [empty, long, non_ascii]) = (unknown_token(), malformed_tokens());
    let dead = [
        &unknown,
        &cases.expired,
        &cases.revoked,
        &cases.tombstoned,
        &cases.purged,
        &cases.expired_password,
        &empty,
        &long,
        &non_ascii,
    ];

    // Act
    let mut logged = Vec::new();
    for token in dead {
        log.take();
        let answer = cases
            .shares
            .authorize_content(token, None, VISITOR, true)
            .await;
        logged.push((answer, log.take_data_statements()));
    }

    // Assert
    let (_, reference) = &logged[0];
    assert_eq!(reference.len(), 2, "{reference:#?}");
    assert!(reference[0].contains("FROM share_links WHERE token_hash = $1"));
    assert!(reference[1].contains("WHERE n.id = $1 AND n.account_id = $2"));
    for (answer, statements) in &logged {
        assert!(
            matches!(answer, Err(PublicShareError::NotFound)),
            "{answer:?}"
        );
        assert_eq!(statements, reference, "no audit write for a refused call");
    }
    drop(cases);
    harness.finish().await;
}

/// The call that enters system scope, spelled in two parts so the CI gate that lists its call sites does not
/// count this probe as one.
const SYSTEM_SCOPE_CALL: &str = concat!("run", "_system(");

/// The crate's source files, relative to the crate root, with their text.
fn crate_sources() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut found = Vec::new();
    let mut dirs = vec![root.join("src")];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).expect("read the source directory") {
            let path = entry.expect("read a directory entry").path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let text = std::fs::read_to_string(&path).expect("read a source file");
                let relative = path.strip_prefix(root).expect("under the crate root");
                found.push((relative.display().to_string(), text));
            }
        }
    }
    found
}

#[test]
fn the_token_lookup_is_the_only_system_scope_path_of_the_crate() {
    // Arrange
    let sources = crate_sources();

    // Act
    let calls: Vec<&str> = sources
        .iter()
        .flat_map(|(path, text)| {
            text.lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .filter(|line| line.contains(SYSTEM_SCOPE_CALL))
                .map(move |_| path.as_str())
        })
        .collect();

    // Assert
    assert_eq!(calls, ["src/lookup_repo.rs"]);
}

#[test]
fn the_visitor_open_path_never_reaches_argon2() {
    // Arrange
    let sources = crate_sources();
    let visitor = ["src/public.rs", "src/content.rs", "src/lookup_repo.rs"];

    // Act
    let mentions: Vec<&str> = sources
        .iter()
        .filter(|(path, _)| visitor.contains(&path.as_str()))
        .filter(|(_, text)| {
            text.contains("argon2") || text.contains("hash_password") || text.contains("secret::")
        })
        .map(|(path, _)| path.as_str())
        .collect();

    // Assert
    assert_eq!(
        sources
            .iter()
            .filter(|(path, _)| visitor.contains(&path.as_str()))
            .count(),
        3
    );
    assert!(mentions.is_empty(), "{mentions:?}");
}
