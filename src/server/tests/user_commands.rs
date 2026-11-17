//! AZ-3795: `admin user list`, `set-quota` and `delete` through the real handler with recording services.

mod support;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use contracts::{
    AccountDeletionService, AccountState, AccountStatus, Actor, AdminError, AdminService,
    AdminUserView, DeletionAccepted, DeletionError, HealthReport, Page, Principal, Role,
};
use sc_clock::Timestamp;
use serde_json::Value;
use support::{Captured, Harness, locked};
use uuid::Uuid;

struct Recorder {
    users: Vec<AdminUserView>,
    page_rows: usize,
    pages: Mutex<Vec<(Actor, Page)>>,
    quotas: Mutex<Vec<(Actor, Uuid, Option<i64>)>>,
    quota_answer: fn() -> Result<(), AdminError>,
}

#[async_trait]
impl AdminService for Recorder {
    async fn list_users(
        &self,
        actor: &Actor,
        page: Page,
    ) -> Result<Vec<AdminUserView>, AdminError> {
        locked(&self.pages).push((actor.clone(), page));
        let start = usize::try_from(page.offset).expect("offset");
        Ok(self
            .users
            .iter()
            .skip(start)
            .take(self.page_rows)
            .cloned()
            .collect())
    }

    async fn update_user(
        &self,
        _: &Actor,
        _: Uuid,
        _: Option<Role>,
        _: Option<AccountState>,
    ) -> Result<(), AdminError> {
        unreachable!("not a user list or set-quota call")
    }

    async fn set_quota(
        &self,
        actor: &Actor,
        account: Uuid,
        limit: Option<i64>,
    ) -> Result<(), AdminError> {
        locked(&self.quotas).push((actor.clone(), account, limit));
        (self.quota_answer)()
    }

    async fn health(&self, _: &Actor) -> Result<HealthReport, AdminError> {
        unreachable!("not a user list or set-quota call")
    }
}

fn user(n: u128, role: Role, used: i64, limit: Option<i64>) -> AdminUserView {
    AdminUserView {
        account: Uuid::from_u128(n),
        role,
        state: AccountStatus::Active,
        last_login: Some(Timestamp::from_unix_micros(1_794_052_800_000_000)),
        used,
        limit,
    }
}

fn recorder(quota_answer: fn() -> Result<(), AdminError>) -> Arc<Recorder> {
    Arc::new(Recorder {
        users: vec![
            user(1, Role::Admin, 10, None),
            user(2, Role::User, 2_048, Some(1 << 30)),
            user(3, Role::User, 0, None),
        ],
        page_rows: 2,
        pages: Mutex::default(),
        quotas: Mutex::default(),
        quota_answer,
    })
}

fn run(admin: &Arc<Recorder>, args: &[&str]) -> Captured {
    let served = Arc::clone(admin);
    Harness::new()
        .real_handler()
        .service::<dyn AdminService, _, _>(move |_| {
            let admin: Arc<dyn AdminService> = served.clone();
            async move { Ok(admin) }
        })
        .run(args)
}

#[test]
fn user_list_reads_every_page_as_the_operator_and_prints_role_state_used_and_limit() {
    let admin = recorder(|| Ok(()));

    let text = run(&admin, &["admin", "user", "list"]);
    let json = run(&admin, &["--json", "admin", "user", "list"]);

    assert_eq!(text.exit, 0, "{}", text.stderr);
    for part in [
        "role: admin",
        "role: user",
        "state: active",
        "used: 2048",
        "limit: 1073741824",
    ] {
        assert!(text.stdout.contains(part), "{part}: {}", text.stdout);
    }
    let envelope: Value = serde_json::from_str(&json.stdout).expect("one JSON object");
    assert_eq!(envelope["data"]["users"].as_array().map(Vec::len), Some(3));
    let pages = locked(&admin.pages);
    assert!(pages.iter().all(|(actor, _)| *actor == Actor::Operator));
    let offsets: Vec<u64> = pages.iter().take(3).map(|(_, page)| page.offset).collect();
    assert_eq!(
        offsets,
        [0, 2, 3],
        "both pages, then the empty one that ends the loop"
    );
}

#[test]
fn user_list_prints_no_token_or_secret_field() {
    let admin = recorder(|| Ok(()));

    let result = run(&admin, &["--json", "admin", "user", "list"]);

    let envelope: Value = serde_json::from_str(&result.stdout).expect("envelope");
    for row in envelope["data"]["users"].as_array().expect("users") {
        let mut keys: Vec<&str> = row
            .as_object()
            .expect("row")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "account",
                "last_login_micros",
                "limit",
                "role",
                "state",
                "used"
            ]
        );
    }
}

#[test]
fn set_quota_makes_one_call_with_the_size_and_a_bad_size_exits_3_with_no_call() {
    let admin = recorder(|| Ok(()));
    let account = Uuid::from_u128(2).to_string();

    let good = run(&admin, &["admin", "user", "set-quota", &account, "1GiB"]);
    let bad = run(&admin, &["admin", "user", "set-quota", &account, "lots"]);
    let not_an_account = run(&admin, &["admin", "user", "set-quota", "nobody", "1GiB"]);

    assert_eq!(good.exit, 0, "{}", good.stderr);
    assert_eq!(bad.exit, 3);
    assert_eq!(not_an_account.exit, 3);
    assert_eq!(
        *locked(&admin.quotas),
        vec![(Actor::Operator, Uuid::from_u128(2), Some(1 << 30))]
    );
}

#[test]
fn an_unknown_account_exits_2_with_not_found() {
    let admin = recorder(|| Err(AdminError::NotFound));
    let account = Uuid::from_u128(9).to_string();

    let result = run(
        &admin,
        &["--json", "admin", "user", "set-quota", &account, "1GiB"],
    );

    assert_eq!(result.exit, 2);
    let envelope: Value = serde_json::from_str(&result.stdout).expect("envelope");
    assert_eq!(envelope["code"], "not_found");
}

struct Deleter {
    calls: Mutex<Vec<Uuid>>,
    answer: fn() -> Result<DeletionAccepted, DeletionError>,
}

#[async_trait]
impl AccountDeletionService for Deleter {
    async fn request(&self, _: &Principal, _: &str) -> Result<DeletionAccepted, DeletionError> {
        unreachable!("the operator has no principal")
    }

    async fn request_as_operator(&self, account: Uuid) -> Result<DeletionAccepted, DeletionError> {
        locked(&self.calls).push(account);
        (self.answer)()
    }
}

const DEADLINE: Timestamp = Timestamp::from_unix_micros(1_794_052_800_000_000);

fn deleter(answer: fn() -> Result<DeletionAccepted, DeletionError>) -> Arc<Deleter> {
    Arc::new(Deleter {
        calls: Mutex::default(),
        answer,
    })
}

fn run_delete(deleter: &Arc<Deleter>, args: &[&str]) -> Captured {
    let served = Arc::clone(deleter);
    Harness::new()
        .real_handler()
        .service::<dyn AccountDeletionService, _, _>(move |_| {
            let deletion: Arc<dyn AccountDeletionService> = served.clone();
            async move { Ok(deletion) }
        })
        .run(args)
}

#[test]
fn delete_without_yes_exits_3_with_no_call_and_with_yes_makes_one_call_and_prints_the_deadline() {
    // Arrange
    let deleter = deleter(|| Ok(DeletionAccepted { deadline: DEADLINE }));
    let account = Uuid::from_u128(2).to_string();

    // Act
    let refused = run_delete(&deleter, &["admin", "user", "delete", &account]);
    let accepted = run_delete(
        &deleter,
        &["--json", "admin", "user", "delete", &account, "--yes"],
    );

    // Assert
    assert_eq!(refused.exit, 3);
    assert_eq!(accepted.exit, 0, "{}", accepted.stderr);
    assert_eq!(*locked(&deleter.calls), vec![Uuid::from_u128(2)]);
    let envelope: Value = serde_json::from_str(&accepted.stdout).expect("envelope");
    assert_eq!(
        envelope["data"]["deadline_micros"],
        1_794_052_800_000_000_i64
    );
}

#[test]
fn deleting_the_last_admin_exits_2_with_last_admin() {
    // Arrange
    let deleter = deleter(|| Err(DeletionError::LastAdmin));
    let account = Uuid::from_u128(1).to_string();

    // Act
    let result = run_delete(
        &deleter,
        &["--json", "admin", "user", "delete", &account, "--yes"],
    );

    // Assert
    assert_eq!(result.exit, 2);
    let envelope: Value = serde_json::from_str(&result.stdout).expect("envelope");
    assert_eq!(envelope["code"], "last_admin");
}

#[test]
fn delete_of_a_name_that_is_not_an_account_id_exits_3_with_no_call() {
    // Arrange
    let deleter = deleter(|| Ok(DeletionAccepted { deadline: DEADLINE }));

    // Act
    let result = run_delete(&deleter, &["admin", "user", "delete", "alice", "--yes"]);

    // Assert
    assert_eq!(result.exit, 3);
    assert!(locked(&deleter.calls).is_empty());
}
