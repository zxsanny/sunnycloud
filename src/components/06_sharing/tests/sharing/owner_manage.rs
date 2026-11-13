//! IT-06-1 list and revoke, IT-06-11 owner events (AZ-3697 AC-5 to AC-7): an owner sees and revokes only his
//! own links, revoke is idempotent, and each create and first revoke writes one event with ids only.

use std::time::Duration;

use contracts::{CreatedShare, ShareError, ShareService};
use sqlx::Connection;
use sunnycloud_sharing::{PgShareService, SHARE_TOKEN_PREFIX};
use uuid::Uuid;

use crate::harness::{Harness, START};
use crate::owner_support::{
    AuditRow, FILE_NAME_PREFIX, account_with_root, audit_rows, live_file, revoked_at_of,
};

const PASSWORD: &str = "correct horse";

/// Account A with links on its files f and g, account B with one link on its own file.
struct TwoOwners {
    harness: Harness,
    shares: PgShareService,
    a: Uuid,
    f: Uuid,
    link_f: CreatedShare,
    link_g: CreatedShare,
    link_b: CreatedShare,
}

async fn two_owners() -> TwoOwners {
    let harness = Harness::new().await;
    let mut conn = harness.fixture_conn().await;
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let root_of_a = account_with_root(&mut conn, a).await;
    let root_of_b = account_with_root(&mut conn, b).await;
    let f = live_file(&mut conn, a, root_of_a).await;
    let g = live_file(&mut conn, a, root_of_a).await;
    let file_of_b = live_file(&mut conn, b, root_of_b).await;
    conn.close().await.expect("close fixture connection");
    let shares = harness.shares();
    let link_f = shares
        .create(a, f, None, Some(PASSWORD))
        .await
        .expect("link on f");
    let link_g = shares.create(a, g, None, None).await.expect("link on g");
    let link_b = shares
        .create(b, file_of_b, None, None)
        .await
        .expect("link of b");
    TwoOwners {
        harness,
        shares,
        a,
        f,
        link_f,
        link_g,
        link_b,
    }
}

impl TwoOwners {
    /// The service holds pool handles: it goes before the database does.
    async fn finish(self) {
        drop(self.shares);
        self.harness.finish().await;
    }
}

fn share_events(rows: Vec<AuditRow>) -> Vec<AuditRow> {
    rows.into_iter()
        .filter(|row| row.0.starts_with("share_"))
        .collect()
}

#[tokio::test]
async fn an_owner_lists_only_his_links_without_tokens() {
    // Arrange
    let owners = two_owners().await;

    // Act
    let all = owners.shares.list(owners.a, None).await.expect("list all");
    let for_f = owners
        .shares
        .list(owners.a, Some(owners.f))
        .await
        .expect("list for f");

    // Assert
    // Both links have the same `created_at` (manual clock), so the id breaks the tie.
    let mut ids: Vec<Uuid> = all.iter().map(|info| info.id).collect();
    let mut expected = [owners.link_f.id, owners.link_g.id];
    ids.sort();
    expected.sort();
    assert_eq!(ids, expected);
    assert_eq!(for_f.len(), 1);
    assert_eq!(for_f[0].id, owners.link_f.id);
    assert!(for_f[0].has_password);
    assert_eq!(for_f[0].created_at, START);
    assert_eq!(for_f[0].expires_at, owners.link_f.expires_at);
    let listed = format!("{all:?}");
    assert!(!listed.contains(SHARE_TOKEN_PREFIX), "{listed}");
    owners.finish().await;
}

#[tokio::test]
async fn a_revoked_link_is_no_longer_listed() {
    // Arrange
    let owners = two_owners().await;
    owners
        .shares
        .revoke(owners.a, owners.link_g.id)
        .await
        .expect("revoke g");

    // Act
    let all = owners.shares.list(owners.a, None).await.expect("list all");

    // Assert
    let ids: Vec<Uuid> = all.iter().map(|info| info.id).collect();
    assert_eq!(ids, [owners.link_f.id]);
    owners.finish().await;
}

#[tokio::test]
async fn revoke_is_idempotent_for_the_owner_and_not_found_for_others() {
    // Arrange
    let owners = two_owners().await;
    let first_at = START.as_unix_micros();

    // Act
    let first = owners.shares.revoke(owners.a, owners.link_f.id).await;
    owners.harness.clock.tick(Duration::from_secs(5));
    let second = owners.shares.revoke(owners.a, owners.link_f.id).await;
    let foreign = owners.shares.revoke(owners.a, owners.link_b.id).await;
    let random = owners.shares.revoke(owners.a, Uuid::new_v4()).await;
    let mut conn = owners.harness.fixture_conn().await;
    let revoked_f = revoked_at_of(&mut conn, owners.link_f.id).await;
    let revoked_b = revoked_at_of(&mut conn, owners.link_b.id).await;

    // Assert
    assert!(first.is_ok() && second.is_ok(), "{first:?} {second:?}");
    assert_eq!(revoked_f, Some(first_at));
    assert!(matches!(foreign, Err(ShareError::NotFound)), "{foreign:?}");
    assert!(matches!(random, Err(ShareError::NotFound)), "{random:?}");
    assert_eq!(revoked_b, None);
    conn.close().await.expect("close fixture connection");
    owners.finish().await;
}

#[tokio::test]
async fn each_create_and_first_revoke_writes_one_event_with_ids_only() {
    // Arrange
    let owners = two_owners().await;

    // Act
    for _ in 0..2 {
        owners
            .shares
            .revoke(owners.a, owners.link_f.id)
            .await
            .expect("revoke f");
    }
    let refused = owners.shares.revoke(owners.a, owners.link_b.id).await;
    let mut conn = owners.harness.fixture_conn().await;
    let events = share_events(audit_rows(&mut conn).await);

    // Assert
    assert!(matches!(refused, Err(ShareError::NotFound)), "{refused:?}");
    let shapes: Vec<(&str, Option<Uuid>)> =
        events.iter().map(|row| (row.0.as_str(), row.3)).collect();
    assert_eq!(
        shapes,
        [
            ("share_created", Some(owners.link_f.id)),
            ("share_created", Some(owners.link_g.id)),
            ("share_created", Some(owners.link_b.id)),
            ("share_revoked", Some(owners.link_f.id)),
        ]
    );
    let revoked = &events[3];
    assert_eq!(revoked.1, Some(owners.a));
    assert_eq!(revoked.2.as_deref(), Some("share_link"));
    assert_eq!(
        revoked.4.as_deref(),
        Some(format!("{{\"node_id\": \"{}\"}}", owners.f).as_str())
    );
    let tokens = [&owners.link_f, &owners.link_g, &owners.link_b].map(|link| {
        link.token
            .expose()
            .strip_prefix(SHARE_TOKEN_PREFIX)
            .expect("prefix")
            .to_owned()
    });
    for row in &events {
        let text = format!("{row:?}");
        assert!(!text.contains(FILE_NAME_PREFIX), "{text}");
        assert!(!text.contains(PASSWORD), "{text}");
        assert!(!text.contains(SHARE_TOKEN_PREFIX), "{text}");
        assert!(
            tokens.iter().all(|token| !text.contains(token.as_str())),
            "{text}"
        );
    }
    conn.close().await.expect("close fixture connection");
    owners.finish().await;
}
