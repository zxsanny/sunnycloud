//! AZ-3794: the identity operator commands through the real handler with recording owner ports.

mod support;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use contracts::{
    AccountState, Actor, AdminError, AdminService, AdminUserPage, CreatedInvite, Fault,
    HealthReport, InviteError, InviteInfo, InviteService, InviteToken, OperatorIdentityError,
    OperatorIdentityOps, Page, Role,
};
use sc_clock::Timestamp;
use sc_ids::{RawToken, TokenKind};
use serde_json::Value;
use support::{Captured, Harness, locked};
use uuid::Uuid;

const TOKEN: &str = "at_7f3a9c1e2b4d6f80a1b2c3d4e5f60718";
const ADMIN: Uuid = Uuid::from_u128(7);
const ACCOUNT: Uuid = Uuid::from_u128(9);

type Calls = Arc<Mutex<Vec<String>>>;

fn token() -> RawToken {
    RawToken::from_presented(TokenKind::Setup, TOKEN.to_owned())
}

#[derive(Clone, Copy)]
struct Answers {
    setup: fn() -> Result<Option<RawToken>, OperatorIdentityError>,
    reset: fn() -> Result<InviteToken, OperatorIdentityError>,
    role: fn() -> Result<(), AdminError>,
    revoke: fn() -> Result<(), InviteError>,
}

impl Default for Answers {
    fn default() -> Self {
        Self {
            setup: || Ok(Some(token())),
            reset: || Ok(InviteToken(token())),
            role: || Ok(()),
            revoke: || Ok(()),
        }
    }
}

struct Ports {
    calls: Calls,
    answers: Answers,
}

#[async_trait]
impl OperatorIdentityOps for Ports {
    async fn ensure_setup_token(&self) -> Result<Option<RawToken>, OperatorIdentityError> {
        locked(&self.calls).push("ensure_setup_token".to_owned());
        (self.answers.setup)()
    }

    async fn create_admin_invite(&self) -> Result<InviteToken, OperatorIdentityError> {
        locked(&self.calls).push("create_admin_invite".to_owned());
        Ok(InviteToken(token()))
    }

    async fn reset_admin(&self, account: Uuid) -> Result<InviteToken, OperatorIdentityError> {
        locked(&self.calls).push(format!("reset_admin {account}"));
        (self.answers.reset)()
    }
}

#[async_trait]
impl InviteService for Ports {
    async fn create_invite(&self, admin: Uuid, note: &str) -> Result<CreatedInvite, InviteError> {
        locked(&self.calls).push(format!("create_invite {admin} [{note}]"));
        Ok(CreatedInvite {
            token: InviteToken(token()),
            expires_at: Timestamp::from_unix_micros(1_794_052_800_000_000),
        })
    }

    async fn list(&self, admin: Uuid) -> Result<Vec<InviteInfo>, InviteError> {
        locked(&self.calls).push(format!("list {admin}"));
        Ok(vec![
            InviteInfo {
                id: Uuid::from_u128(1),
                note: "for bob".to_owned(),
                created_at: Timestamp::from_unix_micros(1_000),
                expires_at: Timestamp::from_unix_micros(2_000),
                redeemed_at: None,
            },
            InviteInfo {
                id: Uuid::from_u128(2),
                note: String::new(),
                created_at: Timestamp::from_unix_micros(3_000),
                expires_at: Timestamp::from_unix_micros(4_000),
                redeemed_at: Some(Timestamp::from_unix_micros(3_500)),
            },
        ])
    }

    async fn revoke(&self, admin: Uuid, invite: Uuid) -> Result<(), InviteError> {
        locked(&self.calls).push(format!("revoke {admin} {invite}"));
        (self.answers.revoke)()
    }
}

#[async_trait]
impl AdminService for Ports {
    async fn list_users(&self, _: &Actor, _: Page) -> Result<AdminUserPage, AdminError> {
        unreachable!("not an identity command")
    }

    async fn update_user(
        &self,
        actor: &Actor,
        account: Uuid,
        role: Option<Role>,
        state: Option<AccountState>,
    ) -> Result<(), AdminError> {
        locked(&self.calls).push(format!(
            "update_user {} {account} {role:?} {state:?}",
            matches!(actor, Actor::Operator)
        ));
        (self.answers.role)()
    }

    async fn set_quota(&self, _: &Actor, _: Uuid, _: Option<i64>) -> Result<(), AdminError> {
        unreachable!("not an identity command")
    }

    async fn health(&self, _: &Actor) -> Result<HealthReport, AdminError> {
        unreachable!("not an identity command")
    }
}

fn run_with(answers: Answers, args: &[&str]) -> (Captured, Vec<String>) {
    let calls: Calls = Arc::default();
    let ports = Arc::new(Ports {
        calls: Arc::clone(&calls),
        answers,
    });
    let (ops, invites, admin) = (Arc::clone(&ports), Arc::clone(&ports), ports);
    let result = Harness::new()
        .real_handler()
        .log_json()
        .service::<dyn OperatorIdentityOps, _, _>(move |_| {
            let ops: Arc<dyn OperatorIdentityOps> = ops.clone();
            async move { Ok(ops) }
        })
        .service::<dyn InviteService, _, _>(move |_| {
            let invites: Arc<dyn InviteService> = invites.clone();
            async move { Ok(invites) }
        })
        .service::<dyn AdminService, _, _>(move |_| {
            let admin: Arc<dyn AdminService> = admin.clone();
            async move { Ok(admin) }
        })
        .run(args);
    let recorded = locked(&calls).clone();
    (result, recorded)
}

fn run(args: &[&str]) -> (Captured, Vec<String>) {
    run_with(Answers::default(), args)
}

fn count(text: &str) -> usize {
    text.matches(TOKEN).count()
}

fn only_on_stdout_once(result: &Captured) {
    assert_eq!(count(&result.stdout), 1, "{}", result.stdout);
    assert_eq!(count(&result.stderr), 0);
    assert_eq!(result.log.iter().map(|line| count(line)).sum::<usize>(), 0);
}

fn json(result: &Captured) -> Value {
    serde_json::from_str(&result.stdout).expect("one JSON object")
}

#[test]
fn setup_token_prints_the_token_once_with_the_hint_on_each_run() {
    // Act
    let first = run(&["admin", "setup-token"]);
    let second = run(&["admin", "setup-token"]);

    // Assert
    for (result, calls) in [first, second] {
        assert_eq!(result.exit, 0, "{}", result.stderr);
        assert_eq!(calls, ["ensure_setup_token"]);
        only_on_stdout_once(&result);
        assert!(result.stdout.contains("hint: sign in with this token"));
    }
}

#[test]
fn setup_token_with_an_account_present_exits_2_with_the_code_and_no_token() {
    // Arrange
    let answers = Answers {
        setup: || Ok(None),
        ..Answers::default()
    };

    // Act
    let (text, _) = run_with(answers, &["admin", "setup-token"]);
    let (object, _) = run_with(answers, &["--json", "admin", "setup-token"]);

    // Assert
    assert_eq!(text.exit, 2);
    assert_eq!(text.stdout, "");
    assert_eq!(object.exit, 2);
    assert_eq!(json(&object)["code"], "SetupNotAvailable");
    assert_eq!(count(&object.stdout), 0);
}

#[test]
fn create_makes_one_call_and_prints_the_token_once_in_text_and_json() {
    // Act
    let (text, calls) = run(&["admin", "create"]);
    let (object, _) = run(&["--json", "admin", "create"]);

    // Assert
    assert_eq!(text.exit, 0, "{}", text.stderr);
    assert_eq!(calls, ["create_admin_invite"]);
    only_on_stdout_once(&text);
    only_on_stdout_once(&object);
    assert_eq!(json(&object)["data"]["token"], TOKEN);
}

#[test]
fn reset_prints_a_fresh_invite_once_and_maps_the_refusals() {
    // Arrange
    let account = ACCOUNT.to_string();
    let not_found = Answers {
        reset: || Err(OperatorIdentityError::NotFound),
        ..Answers::default()
    };
    let not_admin = Answers {
        reset: || Err(OperatorIdentityError::NotAnAdmin),
        ..Answers::default()
    };

    // Act
    let (ok, calls) = run(&["admin", "reset", &account]);
    let (missing, _) = run_with(not_found, &["--json", "admin", "reset", &account]);
    let (user, _) = run_with(not_admin, &["--json", "admin", "reset", &account]);
    let (bad, bad_calls) = run(&["admin", "reset", "alice"]);

    // Assert
    assert_eq!(ok.exit, 0, "{}", ok.stderr);
    assert_eq!(calls, [format!("reset_admin {ACCOUNT}")]);
    only_on_stdout_once(&ok);
    assert_eq!(
        (missing.exit, json(&missing)["code"].clone()),
        (2, "not_found".into())
    );
    assert_eq!(
        (user.exit, json(&user)["code"].clone()),
        (2, "not_an_admin".into())
    );
    assert_eq!(bad.exit, 3);
    assert!(bad_calls.is_empty());
}

#[test]
fn promote_and_demote_call_update_user_as_the_operator_and_the_last_admin_exits_2() {
    // Arrange
    let account = ACCOUNT.to_string();
    let last = Answers {
        role: || Err(AdminError::LastAdmin),
        ..Answers::default()
    };

    // Act
    let (promoted, promote_calls) = run(&["admin", "promote", &account]);
    let (refused, _) = run_with(last, &["--json", "admin", "demote", &account]);
    let (_, demote_calls) = run(&["admin", "demote", &account]);

    // Assert
    assert_eq!(promoted.exit, 0, "{}", promoted.stderr);
    assert_eq!(
        promote_calls,
        [format!("update_user true {ACCOUNT} Some(Admin) None")]
    );
    assert_eq!(
        demote_calls,
        [format!("update_user true {ACCOUNT} Some(User) None")]
    );
    assert_eq!(refused.exit, 2);
    assert_eq!(json(&refused)["code"], "last_admin");
}

#[test]
fn invite_create_list_and_revoke_make_one_call_each_and_list_shows_no_token() {
    // Arrange
    let admin = ADMIN.to_string();
    let invite = Uuid::from_u128(1).to_string();

    // Act
    let (created, create_calls) = run(&[
        "admin", "invite", "--admin", &admin, "create", "--note", "for bob",
    ]);
    let (listed, list_calls) = run(&["--json", "admin", "invite", "--admin", &admin, "list"]);
    let (revoked, revoke_calls) = run(&["admin", "invite", "--admin", &admin, "revoke", &invite]);

    // Assert
    assert_eq!(created.exit, 0, "{}", created.stderr);
    assert_eq!(create_calls, [format!("create_invite {ADMIN} [for bob]")]);
    only_on_stdout_once(&created);
    assert_eq!(list_calls, [format!("list {ADMIN}")]);
    let rows = json(&listed)["data"]["invites"]
        .as_array()
        .expect("rows")
        .clone();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["note"], "for bob");
    assert!(rows[0]["redeemed_at_micros"].is_null());
    assert_eq!(rows[1]["redeemed_at_micros"], 3_500);
    assert_eq!(count(&listed.stdout), 0);
    assert_eq!(revoked.exit, 0, "{}", revoked.stderr);
    assert_eq!(
        revoke_calls,
        [format!("revoke {ADMIN} {}", Uuid::from_u128(1))]
    );
}

#[test]
fn revoking_an_unknown_invite_exits_2_and_bad_ids_exit_3_without_a_call() {
    // Arrange
    let admin = ADMIN.to_string();
    let invite = Uuid::from_u128(1).to_string();
    let unknown = Answers {
        revoke: || Err(InviteError::NotFound),
        ..Answers::default()
    };

    // Act
    let (missing, _) = run_with(
        unknown,
        &[
            "--json", "admin", "invite", "--admin", &admin, "revoke", &invite,
        ],
    );
    let (bad_invite, calls_a) = run(&["admin", "invite", "--admin", &admin, "revoke", "inv-1"]);
    let (bad_admin, calls_b) = run(&["admin", "invite", "--admin", "bob", "list"]);

    // Assert
    assert_eq!(
        (missing.exit, json(&missing)["code"].clone()),
        (2, "not_found".into())
    );
    assert_eq!(bad_invite.exit, 3);
    assert_eq!(bad_admin.exit, 3);
    assert!(calls_a.is_empty() && calls_b.is_empty());
}

#[test]
fn an_unexpected_failure_shows_an_opaque_message_and_never_the_token() {
    // Arrange
    let broken = Answers {
        setup: || {
            Err(OperatorIdentityError::Unexpected(Fault::from(
                "db down at_7f3a9c1e2b4d6f80a1b2c3d4e5f60718",
            )))
        },
        ..Answers::default()
    };

    // Act
    let (result, _) = run_with(broken, &["admin", "setup-token"]);

    // Assert
    assert_eq!(result.exit, 2);
    assert!(!result.stderr.contains("db down"), "{}", result.stderr);
}
