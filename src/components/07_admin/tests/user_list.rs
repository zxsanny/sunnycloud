//! AC-1: the paged listing with usage.

mod support;

use contracts::{AccountStatus, AdminUserView, Page, Role};
use sc_clock::Timestamp;
use sqlx::PgConnection;
use support::harness::{Harness, START, admin_actor};
use support::seed;
use uuid::Uuid;

const MIB: i64 = 1 << 20;
const GIB: i64 = 1 << 30;
const ACCOUNTS: usize = 45;
const PAGE: u32 = 20;
const SECRET_FIELDS: [&str; 4] = ["token", "password", "secret", "hash"];

/// 45 accounts, oldest first: positions 0 to 2 are admins, 3 and 4 disabled, position 10 has a 1 GiB limit
/// with 300 MiB used and 100 MiB reserved, and every fifth account has signed in.
async fn seed_45(conn: &mut PgConnection) -> Vec<AdminUserView> {
    let mut expected = Vec::new();
    for position in 0..ACCOUNTS {
        let id = Uuid::new_v4();
        let (role, state) = match position {
            0..=2 => ("admin", "active"),
            3 | 4 => ("user", "disabled"),
            _ => ("user", "active"),
        };
        let created = START.as_unix_micros() + position as i64;
        seed::account(
            conn,
            id,
            &format!("Person {position}"),
            role,
            state,
            created,
        )
        .await;
        let (limit, used, reserved) = if position == 10 {
            (Some(GIB), 300 * MIB, 100 * MIB)
        } else {
            (None, position as i64 * 1000, position as i64)
        };
        seed::quota(conn, id, limit, used, reserved).await;
        let last_login =
            (position % 5 == 0).then_some(START.as_unix_micros() + 7 * position as i64);
        if last_login.is_some() {
            seed::identity(conn, id, &format!("p{position}@example.test"), last_login).await;
        }
        expected.push(AdminUserView {
            account: id,
            role: if role == "admin" {
                Role::Admin
            } else {
                Role::User
            },
            state: if state == "disabled" {
                AccountStatus::Disabled
            } else {
                AccountStatus::Active
            },
            last_login: last_login.map(Timestamp::from_unix_micros),
            used,
            limit,
        });
    }
    expected
}

async fn all_pages(
    harness: &Harness,
    caller: &contracts::Actor,
    size: u32,
) -> Vec<Vec<AdminUserView>> {
    let mut pages = Vec::new();
    let mut offset = 0;
    loop {
        let page = harness
            .service
            .list_users(
                caller,
                Page {
                    offset,
                    limit: size,
                },
            )
            .await
            .expect("list a page");
        if page.is_empty() {
            return pages;
        }
        offset += page.len() as u64;
        pages.push(page);
    }
}

#[tokio::test]
async fn pages_of_20_hold_20_20_and_5_users_equal_to_the_seeded_values() {
    // Arrange
    let harness = Harness::new().await;
    let expected = seed_45(&mut harness.fixture().await).await;
    let caller = admin_actor(Uuid::new_v4());

    // Act
    let pages = all_pages(&harness, &caller, PAGE).await;

    // Assert
    let sizes: Vec<usize> = pages.iter().map(Vec::len).collect();
    assert_eq!(sizes, [20, 20, 5]);
    let listed: Vec<AdminUserView> = pages.into_iter().flatten().collect();
    assert_eq!(listed, expected);
    harness.finish().await;
}

#[tokio::test]
async fn every_account_appears_exactly_once_and_a_second_run_gives_the_same_order() {
    // Arrange
    let harness = Harness::new().await;
    seed_45(&mut harness.fixture().await).await;
    let caller = admin_actor(Uuid::new_v4());

    // Act
    let first: Vec<Uuid> = all_pages(&harness, &caller, PAGE)
        .await
        .into_iter()
        .flatten()
        .map(|view| view.account)
        .collect();
    let second: Vec<Uuid> = all_pages(&harness, &caller, PAGE)
        .await
        .into_iter()
        .flatten()
        .map(|view| view.account)
        .collect();

    // Assert
    let mut unique = first.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(first.len(), ACCOUNTS);
    assert_eq!(unique.len(), ACCOUNTS);
    assert_eq!(first, second);
    harness.finish().await;
}

#[tokio::test]
async fn each_page_makes_one_accounts_call_and_one_usage_call() {
    // Arrange
    let harness = Harness::new().await;
    seed_45(&mut harness.fixture().await).await;
    let caller = admin_actor(Uuid::new_v4());
    let mut per_page = Vec::new();

    // Act
    for offset in [0, 20, 40] {
        let before = harness.port_calls();
        let page = harness
            .service
            .list_users(
                &caller,
                Page {
                    offset,
                    limit: PAGE,
                },
            )
            .await
            .expect("list a page");
        let after = harness.port_calls();
        per_page.push((
            page.len(),
            [0, 1, 2].map(|index| after[index] - before[index]),
        ));
    }

    // Assert
    assert_eq!(per_page, [(20, [1, 0, 1]), (20, [1, 0, 1]), (5, [1, 0, 1])]);
    harness.finish().await;
}

#[test]
fn a_listed_user_carries_no_token_or_password_field() {
    // Arrange
    let view = AdminUserView {
        account: Uuid::nil(),
        role: Role::User,
        state: AccountStatus::Active,
        last_login: None,
        used: 0,
        limit: None,
    };

    // Act
    let AdminUserView {
        account: _,
        role: _,
        state: _,
        last_login: _,
        used: _,
        limit: _,
    } = &view;
    let printed = format!("{view:?}").to_lowercase();

    // Assert
    for word in SECRET_FIELDS {
        assert!(!printed.contains(word), "{word} in {printed}");
    }
}

#[tokio::test]
async fn an_account_without_a_ledger_row_is_listed_with_no_use_and_no_limit() {
    // Arrange
    let harness = Harness::new().await;
    let id = Uuid::new_v4();
    seed::account(
        &mut harness.fixture().await,
        id,
        "No Ledger",
        "user",
        "deleting",
        START.as_unix_micros(),
    )
    .await;
    let caller = admin_actor(Uuid::new_v4());

    // Act
    let page = harness
        .service
        .list_users(
            &caller,
            Page {
                offset: 0,
                limit: PAGE,
            },
        )
        .await
        .expect("list a page");

    // Assert
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].state, AccountStatus::Deleting);
    assert_eq!((page[0].used, page[0].limit), (0, None));
    harness.finish().await;
}

#[tokio::test]
async fn a_page_never_asks_for_more_than_500_rows_or_the_configured_size() {
    // Arrange
    let capped = Harness::with_settings(&[("SUNNYCLOUD_ADMIN_LIST_PAGE_SIZE", "10")]).await;
    let open = Harness::new().await;
    seed_45(&mut capped.fixture().await).await;
    let caller = admin_actor(Uuid::new_v4());

    // Act
    let capped_page = capped
        .service
        .list_users(
            &caller,
            Page {
                offset: 0,
                limit: 10_000,
            },
        )
        .await
        .expect("list a capped page");
    let open_page = open
        .service
        .list_users(
            &caller,
            Page {
                offset: 0,
                limit: 10_000,
            },
        )
        .await
        .expect("list an open page");

    // Assert
    assert_eq!(capped_page.len(), 10);
    assert_eq!(
        capped
            .accounts
            .max_list_limit
            .load(std::sync::atomic::Ordering::SeqCst),
        10
    );
    assert!(open_page.is_empty());
    assert_eq!(
        open.accounts
            .max_list_limit
            .load(std::sync::atomic::Ordering::SeqCst),
        500
    );
    capped.finish().await;
    open.finish().await;
}
