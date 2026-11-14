//! ST-06-8 (AZ-3703 AC-5, AC-59, NFT-SEC-46), with links of A and B made by the real `create`: the API role scoped
//! to B reads only B's rows of `share_links`; the one cross-account lookup finds a link of either account by its
//! exact token and nothing for partial or pattern input (a prefix, the marker, a body without it, `LIKE` wildcards,
//! a token with a character more). The rest of ST-06-8 is in the `sharing` binary: `rls.rs` (unscoped reads, writes
//! and DDL of the API role) and `visitor_statements.rs` (the lookup is the crate's one `run_system` call, an
//! equality on `token_hash`).

use std::net::{IpAddr, Ipv4Addr};

use contracts::{PublicShareError, ShareOpen, ShareService};
use sc_db::Scope;
use sqlx::Connection;
use sunnycloud_sharing::SHARE_TOKEN_PREFIX;
use uuid::Uuid;

use crate::harness::Harness;
use crate::owner_support::account_with_root;
use crate::visitor_support::{HOUR, VISITOR, named_file, presented};

const PROBER: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 88));

/// Partial and pattern forms of `token`; none is a whole token.
fn partial_inputs(token: &str) -> Vec<String> {
    let body = &token[SHARE_TOKEN_PREFIX.len()..];
    let mut inputs = vec![
        SHARE_TOKEN_PREFIX.to_owned(),
        body.to_owned(),
        format!("{token}A"),
        format!("{SHARE_TOKEN_PREFIX}%"),
        "%".to_owned(),
        format!("{SHARE_TOKEN_PREFIX}{}", "_".repeat(body.len())),
        format!("{}%", &token[..token.len() - 1]),
        token[1..].to_owned(),
    ];
    for cut in [1, 2, 21, body.len() - 1] {
        inputs.push(token[..SHARE_TOKEN_PREFIX.len() + cut].to_owned());
    }
    inputs
}

#[tokio::test]
async fn b_reads_only_its_rows_and_the_lookup_answers_only_an_exact_token() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let root_of_a = account_with_root(&mut conn, a).await;
    let root_of_b = account_with_root(&mut conn, b).await;
    let file_of_a = named_file(&mut conn, a, root_of_a, "a.jpg", 10, "image/jpeg").await;
    let file_of_b = named_file(&mut conn, b, root_of_b, "b.jpg", 20, "image/jpeg").await;
    conn.close().await.expect("close fixture connection");
    let shares = harness.shares();
    let mut links_of_a = Vec::new();
    for _ in 0..3 {
        let link = shares.create(a, file_of_a, Some(HOUR), None).await;
        links_of_a.push(link.expect("A's link"));
    }
    let link_of_b = shares
        .create(b, file_of_b, Some(HOUR), None)
        .await
        .expect("B's link");

    // Act
    let visible: Vec<(Uuid, Uuid)> = harness
        .api
        .run(Scope::Account(b), async |tx| {
            sqlx::query_as("SELECT id, account_id FROM share_links")
                .fetch_all(tx.conn())
                .await
        })
        .await
        .expect("scoped read");
    let exact_a = shares.open(&links_of_a[0].token, VISITOR).await;
    let exact_b = shares.open(&link_of_b.token, VISITOR).await;
    let mut partial = Vec::new();
    for input in partial_inputs(links_of_a[0].token.expose()) {
        let answer = shares.open(&presented(input.clone()), PROBER).await;
        let content = shares
            .authorize_content(&presented(input.clone()), None, PROBER, false)
            .await;
        partial.push((input, answer, content));
    }

    // Assert
    assert_eq!(visible, [(link_of_b.id, b)]);
    assert!(
        matches!(&exact_a, Ok(ShareOpen::Available { name, .. }) if name == "a.jpg"),
        "{exact_a:?}"
    );
    assert!(
        matches!(&exact_b, Ok(ShareOpen::Available { name, .. }) if name == "b.jpg"),
        "{exact_b:?}"
    );
    for (input, answer, content) in &partial {
        assert!(
            matches!(answer, Ok(ShareOpen::NotFound)),
            "{input}: {answer:?}"
        );
        assert!(
            matches!(content, Err(PublicShareError::NotFound)),
            "{input}: {content:?}"
        );
    }
    drop(shares);
    harness.finish().await;
}
