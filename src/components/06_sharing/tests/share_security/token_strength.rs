//! ST-06-2 (AZ-3703 AC-2, AC-12, NFT-SEC-16, FT-P-38): 10,000 links made by the link factory through the real
//! `create` carry tokens of the share prefix and 256 random bits, all distinct, with no common prefix beyond the
//! marker and bytes a chi-square test does not reject as uniform at p = 0.001. `share_links` dumped as the schema
//! owner, the audit log and every log line captured while the links were made and visited hold no token and no
//! password in plaintext; the table holds exactly the SHA-256 of each token.

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use contracts::{PublicShareError, PublicShareService};
use sc_ids::sha256_token;
use sqlx::{Connection, PgConnection};
use sunnycloud_sharing::SHARE_TOKEN_PREFIX;
use tracing::Level;

use crate::chi_square::{statistic, uniform_not_rejected};
use crate::corpora::{BODY_LEN, Guess, passwords, plaintext_forms, token_corpus};
use crate::harness::Harness;
use crate::link_factory::{FactoryLink, LinkFactory};
use crate::logs::LogCapture;
use crate::owner_support::link_rows_as_text;
use crate::visitor_support::VISITOR;

const LINKS: usize = 10_000;
const VISITED: usize = 60;
const GUESSER: IpAddr = IpAddr::V4(Ipv4Addr::new(198, 51, 100, 200));
const WRONG: &str = "wrong-guess-of-the-visitor";

/// Opens, downloads and unlocks some links, trips one link's wrong-password limit and one address's invalid-token
/// limit, so the audit log and the logs hold every kind of share event and WARN line.
async fn visit(shares: &sunnycloud_sharing::PgShareService, links: &[FactoryLink]) {
    for link in &links[..VISITED] {
        let token = &link.created.token;
        shares.open(token, None, VISITOR).await.expect("open");
        let grant = match link.password {
            Some(password) => Some(
                shares
                    .verify_password(token, password, VISITOR)
                    .await
                    .expect("the right password"),
            ),
            None => None,
        };
        let content = shares
            .authorize_content(token, grant.as_ref(), VISITOR, true)
            .await
            .expect("authorized");
        let file = &link.file;
        assert_eq!(
            (content.account_id, content.node_id, content.revision_id),
            (file.account, file.node, file.revision)
        );
    }
    let protected = links
        .iter()
        .find(|link| link.password.is_some())
        .expect("a password link");
    for _ in 0..=10 {
        let answer = shares
            .verify_password(&protected.created.token, WRONG, VISITOR)
            .await;
        assert!(matches!(
            answer,
            Err(PublicShareError::WrongPassword | PublicShareError::RateLimited { .. })
        ));
    }
    let live: Vec<String> = links[..VISITED]
        .iter()
        .map(|link| link.created.token.expose().to_owned())
        .collect();
    // 105 near misses: past the 100 invalid tokens an address may try per minute.
    for (_, guess) in token_corpus(&live, 140, 2)
        .into_iter()
        .filter(|(kind, _)| *kind != Guess::Random)
    {
        let answer = shares.open(&guess, None, GUESSER).await;
        assert!(answer.is_ok() || matches!(answer, Err(PublicShareError::RateLimited { .. })));
    }
}

async fn audit_rows_as_text(conn: &mut PgConnection) -> Vec<String> {
    sqlx::query_scalar("SELECT row_to_json(a)::text FROM audit_log a ORDER BY id")
        .fetch_all(conn)
        .await
        .expect("read audit_log")
}

async fn stored_hashes(conn: &mut PgConnection) -> HashSet<Vec<u8>> {
    sqlx::query_scalar::<_, Vec<u8>>("SELECT token_hash FROM share_links")
        .fetch_all(conn)
        .await
        .expect("read token hashes")
        .into_iter()
        .collect()
}

/// The token bodies found in `text`: every run of 43 URL-safe base64 characters that is one of `bodies`.
fn leaked_bodies(text: &str, bodies: &HashSet<&[u8]>) -> usize {
    let bytes = text.as_bytes();
    let body_char = |b: &u8| b.is_ascii_alphanumeric() || *b == b'-' || *b == b'_';
    bytes
        .windows(BODY_LEN)
        .filter(|window| window.iter().all(body_char) && bodies.contains(window))
        .count()
}

fn common_prefix_len(tokens: &[&str]) -> usize {
    let first = tokens[0].as_bytes();
    tokens.iter().fold(first.len(), |len, token| {
        first[..len]
            .iter()
            .zip(token.as_bytes())
            .take_while(|(a, b)| a == b)
            .count()
    })
}

#[tokio::test]
async fn ten_thousand_tokens_are_strong_distinct_uniform_and_never_stored_or_logged() {
    // Arrange
    let harness = Harness::new().await;
    let mut factory = LinkFactory::new(&harness, 20, 5, 0x5106_0002).await;
    let shares = Arc::new(harness.shares());
    let logs = LogCapture::install();
    let links = factory.links(&shares, LINKS, 25).await;
    visit(&shares, &links).await;

    // Act
    let tokens: Vec<&str> = links
        .iter()
        .map(|link| link.created.token.expose())
        .collect();
    let bodies: Vec<&str> = tokens
        .iter()
        .filter_map(|token| token.strip_prefix(SHARE_TOKEN_PREFIX))
        .collect();
    let decoded: Vec<Vec<u8>> = bodies
        .iter()
        .map(|body| URL_SAFE_NO_PAD.decode(body).expect("URL-safe base64"))
        .collect();
    let mut conn = harness.fixture_conn().await;
    let dump = link_rows_as_text(&mut conn).await;
    let audit = audit_rows_as_text(&mut conn).await;
    let hashes = stored_hashes(&mut conn).await;
    conn.close().await.expect("close fixture connection");
    let lines: Vec<String> = logs.lines().into_iter().map(|line| line.text).collect();
    let warnings = logs.at(Level::WARN, "share abuse limit tripped").len();

    // Assert
    assert_eq!(SHARE_TOKEN_PREFIX, "sc_shr_");
    assert_eq!(bodies.len(), LINKS, "every token has the prefix");
    assert!(decoded.iter().all(|bytes| bytes.len() == 32), "256 bits");
    let distinct: HashSet<&str> = tokens.iter().copied().collect();
    assert_eq!(distinct.len(), LINKS);
    assert_eq!(common_prefix_len(&tokens), SHARE_TOKEN_PREFIX.len());
    let first_chars: HashSet<u8> = bodies.iter().map(|body| body.as_bytes()[0]).collect();
    assert_eq!(first_chars.len(), 64, "every first character occurs");
    let all_bytes: Vec<u8> = decoded.concat();
    assert!(
        uniform_not_rejected(&all_bytes),
        "chi-square {}",
        statistic(&all_bytes)
    );
    let expected_hashes: HashSet<Vec<u8>> = tokens
        .iter()
        .map(|token| sha256_token(token).as_bytes().to_vec())
        .collect();
    assert_eq!(hashes, expected_hashes, "the table holds the hashes only");
    assert_eq!(warnings, 2, "both trips were logged and captured");
    assert!(audit.len() > LINKS, "every create and visit was audited");
    let body_set: HashSet<&[u8]> = bodies.iter().map(|body| body.as_bytes()).collect();
    let secrets: Vec<String> = passwords()
        .into_iter()
        .chain([WRONG])
        .flat_map(plaintext_forms)
        .collect();
    for (place, texts) in [
        ("share_links", &dump),
        ("audit_log", &audit),
        ("logs", &lines),
    ] {
        for text in texts {
            assert!(!text.contains(SHARE_TOKEN_PREFIX), "{place}: {text}");
            assert_eq!(leaked_bodies(text, &body_set), 0, "{place}: {text}");
            for secret in &secrets {
                assert!(!text.contains(secret.as_str()), "{place}: {text}");
            }
        }
    }
    drop(shares);
    harness.finish().await;
}
