//! AC-3 of the admin lifecycle: the batched view answers for 500 accounts, omits unknown ids and reads under
//! the API role's row-level security. The one-statement half is in `views_statement_repo.rs`.

mod support;

use contracts::{QuotaAdminError, QuotaView};
use support::{GIB, Harness, Seeded, admin, initialized, seeded};
use uuid::Uuid;

const DEFAULT: i64 = 10 * GIB;
const ACCOUNTS: usize = 500;

#[tokio::test]
async fn views_of_500_ids_equal_the_seeded_values_and_unknown_ids_are_omitted() {
    // Arrange
    let Seeded { harness, expected } = seeded().await;
    let quota = admin(&harness.api, DEFAULT);
    let mut ids: Vec<Uuid> = expected.keys().copied().collect();

    // Act
    let all = quota.views(&ids).await.unwrap();
    ids.extend([Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()]);
    let with_unknown = quota.views(&ids).await.unwrap();
    let none = quota.views(&[]).await.unwrap();

    // Assert
    assert_eq!(all.len(), ACCOUNTS);
    assert_eq!(all, expected);
    assert_eq!(with_unknown, expected);
    assert!(none.is_empty());
    harness.finish().await;
}

#[tokio::test]
async fn view_matches_views_and_an_unknown_id_is_not_found() {
    // Arrange
    let harness = Harness::new().await;
    let (with_limit, with_default) = (Uuid::new_v4(), Uuid::new_v4());
    initialized(&harness, DEFAULT, with_limit, Some(2 * GIB), 5, 6).await;
    initialized(&harness, DEFAULT, with_default, None, 7, 8).await;
    let quota = admin(&harness.api, DEFAULT);

    // Act
    let batch = quota.views(&[with_limit, with_default]).await.unwrap();
    let first = quota.view(with_limit).await.unwrap();
    let second = quota.view(with_default).await.unwrap();
    let unknown = quota.view(Uuid::new_v4()).await;

    // Assert
    assert_eq!(batch[&with_limit], first);
    assert_eq!(batch[&with_default], second);
    assert_eq!(
        first,
        QuotaView {
            limit_bytes: Some(2 * GIB),
            effective_limit: 2 * GIB,
            used: 5,
            reserved: 6
        }
    );
    assert_eq!(second.limit_bytes, None);
    assert_eq!(second.effective_limit, DEFAULT);
    assert!(matches!(unknown, Err(QuotaAdminError::NotFound)));
    harness.finish().await;
}

#[tokio::test]
async fn repeated_ids_answer_once() {
    // Arrange
    let harness = Harness::new().await;
    let id = Uuid::new_v4();
    initialized(&harness, DEFAULT, id, None, 1, 2).await;
    let quota = admin(&harness.api, DEFAULT);

    // Act
    let views = quota.views(&[id, id]).await.unwrap();

    // Assert
    assert_eq!(views.len(), 1);
    harness.finish().await;
}
