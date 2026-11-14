//! ST-06-7 (AZ-3703 AC-4, AC-12, NFT-SEC-51): every visitor outcome of `open` and `verify_password` (available,
//! password required, wrong password, not found, rate limited per address and per link, a grant), written out as
//! a visitor-facing layer would (`Debug` of the result, `Display` of the error, the grant's fields), carries no
//! account id, owner name, folder path, node id or link id. `Available` has exactly the four fields name, size,
//! media type and expiry: the patterns below name every field without `..`, so a fifth field does not compile, and
//! its written-out field list is read back. `ShareContent` is internal to 11/03 and asserted there.

use std::net::{IpAddr, Ipv4Addr};

use contracts::{AccessGrant, PublicShareError, PublicShareService, ShareOpen, ShareService};
use sqlx::Connection;
use uuid::Uuid;

use crate::harness::Harness;
use crate::owner_support::{account_with_root, node};
use crate::visitor_support::{HOUR, PASSWORD, VISITOR, named_file, unknown_token};

const LIMITED: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 77));
const FOLDERS: [&str; 2] = ["Private Projects", "client-x"];
/// `display_name` of every harness account.
const OWNER_NAME: &str = "Fixture";

/// The top-level field names of a `Debug` text `Variant { a: .., b: .. }`; quoted strings are skipped.
fn debug_fields(text: &str) -> Vec<String> {
    let (_, rest) = text.split_once(" { ").expect("a struct variant");
    let inner = rest.strip_suffix(" }").expect("closed");
    let (mut parts, mut start, mut depth) = (Vec::new(), 0, 0i32);
    let (mut quoted, mut escaped) = (false, false);
    for (at, c) in inner.char_indices() {
        if quoted {
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => quoted = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => quoted = true,
            '(' | '{' | '[' => depth += 1,
            ')' | '}' | ']' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&inner[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    parts.push(&inner[start..]);
    parts
        .iter()
        .map(|part| {
            part.trim()
                .split_once(':')
                .expect("name: value")
                .0
                .to_owned()
        })
        .collect()
}

fn written_out<T: std::fmt::Debug>(outcome: &Result<T, PublicShareError>) -> String {
    match outcome {
        Ok(value) => format!("{outcome:?} {value:?}"),
        Err(error) => format!("{outcome:?} {error}"),
    }
}

#[tokio::test]
async fn visitor_outcomes_carry_the_four_file_fields_and_no_identifier() {
    // Arrange
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let account = Uuid::new_v4();
    let root = account_with_root(&mut conn, account).await;
    let outer = node(&mut conn, account, Some(root), "folder", FOLDERS[0]).await;
    let inner = node(&mut conn, account, Some(outer), "folder", FOLDERS[1]).await;
    let plain_file = named_file(&mut conn, account, inner, "report.jpg", 900, "image/jpeg").await;
    let locked_file = named_file(
        &mut conn,
        account,
        inner,
        "plan.pdf",
        700,
        "application/pdf",
    )
    .await;
    conn.close().await.expect("close fixture connection");
    let shares = harness.shares();
    let plain = shares
        .create(account, plain_file, Some(HOUR), None)
        .await
        .expect("plain link");
    let locked = shares
        .create(account, locked_file, Some(HOUR), Some(PASSWORD))
        .await
        .expect("password link");

    // Act
    let mut opens = vec![
        shares.open(&plain.token, VISITOR).await,
        shares.open(&locked.token, VISITOR).await,
        shares.open(&unknown_token(), VISITOR).await,
    ];
    let mut checks = vec![
        shares
            .verify_password(&locked.token, PASSWORD, VISITOR)
            .await,
        shares
            .verify_password(&locked.token, "not it", VISITOR)
            .await,
        shares
            .verify_password(&plain.token, PASSWORD, VISITOR)
            .await,
        shares
            .verify_password(&unknown_token(), PASSWORD, VISITOR)
            .await,
    ];
    for _ in 0..harness.config.wrong_password_limit {
        checks.push(
            shares
                .verify_password(&locked.token, "not it", VISITOR)
                .await,
        );
    }
    for _ in 0..harness.config.invalid_token_limit {
        opens.push(shares.open(&unknown_token(), LIMITED).await);
    }
    opens.push(shares.open(&plain.token, LIMITED).await);
    checks.push(
        shares
            .verify_password(&locked.token, PASSWORD, LIMITED)
            .await,
    );

    // Assert
    let ShareOpen::Available {
        name,
        size,
        media_type,
        expires_at,
    } = opens[0].as_ref().expect("available")
    else {
        panic!("expected Available: {:?}", opens[0]);
    };
    assert_eq!(
        (name.as_str(), *size, media_type.as_deref(), *expires_at),
        ("report.jpg", 900, Some("image/jpeg"), plain.expires_at)
    );
    assert_eq!(
        debug_fields(&format!("{:?}", opens[0].as_ref().expect("available"))),
        ["name", "size", "media_type", "expires_at"]
    );
    let AccessGrant { value, expires_at } = checks[0].as_ref().expect("a grant");
    assert_eq!(value.len(), 43);
    assert!(matches!(opens[1], Ok(ShareOpen::PasswordRequired)));
    assert!(matches!(opens[2], Ok(ShareOpen::NotFound)));
    assert!(matches!(checks[1], Err(PublicShareError::WrongPassword)));
    assert!(matches!(
        checks[2..4],
        [
            Err(PublicShareError::NotFound),
            Err(PublicShareError::NotFound)
        ]
    ));
    let last_open = opens.last().expect("an open");
    let last_check = checks.last().expect("a check");
    assert!(matches!(
        last_open,
        Err(PublicShareError::RateLimited { .. })
    ));
    assert!(matches!(
        last_check,
        Err(PublicShareError::RateLimited { .. })
    ));
    assert!(matches!(
        checks[checks.len() - 2],
        Err(PublicShareError::RateLimited { .. })
    ));
    let mut texts: Vec<String> = opens.iter().map(written_out).collect();
    texts.extend(checks.iter().map(written_out));
    texts.push(format!("{value} {expires_at:?}"));
    let ids = [
        account,
        root,
        outer,
        inner,
        plain_file,
        locked_file,
        plain.id,
        locked.id,
    ];
    let mut forbidden: Vec<String> = ids
        .iter()
        .flat_map(|id| [id.to_string(), id.simple().to_string()])
        .collect();
    forbidden.extend(FOLDERS.iter().map(|folder| (*folder).to_owned()));
    forbidden.push(OWNER_NAME.to_owned());
    for text in &texts {
        for secret in &forbidden {
            assert!(!text.contains(secret.as_str()), "{secret} in {text}");
        }
    }
    drop(shares);
    harness.finish().await;
}
