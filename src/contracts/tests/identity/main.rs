//! Recording doubles of the 01 identity ports (AC-2). Methods that take `&mut DbTx` are in `tx_join.rs`.

#[path = "../common/mod.rs"]
mod common;
mod doubles;

use std::sync::Arc;

use common::{Calls, IP, id, principal, sorted};
use contracts::{
    AccountAdmin, AccountFilter, AccountGuard, Authenticator, BeginSignIn, BrowserBinding,
    CallbackParams, ClientKind, Credential, Decision, DeviceGrantService, InviteService,
    OperatorIdentityOps, Page, PrincipalRevalidator, RevokeReason, Scope, SessionService,
    SignInService,
};
use doubles::{Double, token};
use sc_ids::TokenKind;

fn start() -> BeginSignIn {
    BeginSignIn {
        provider: "github".to_owned(),
        return_to: "/".to_owned(),
        client_kind: ClientKind::Web,
        invite_token: None,
        setup_token: None,
        code_challenge: None,
        host: "cloud.example".to_owned(),
        client_ip: LOOPBACK,
    }
}

const LOOPBACK: std::net::IpAddr = std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);

fn params() -> CallbackParams {
    CallbackParams(vec![("code".to_owned(), "abc".to_owned())])
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn authenticator_and_revalidator_doubles_record_each_method_across_two_threads() {
    let calls = Calls::default();
    let auth: Arc<dyn Authenticator> = Arc::new(Double(calls.clone()));
    let revalidator: Arc<dyn PrincipalRevalidator> = Arc::new(Double(calls.clone()));

    let a = tokio::spawn(async move {
        let who = auth
            .authenticate(Credential::Session(token(TokenKind::Session)))
            .await
            .expect("authenticate");
        auth.require(&who, Scope::Admin).expect("require");
    });
    let b = tokio::spawn(async move { revalidator.revalidate(&principal()).await });
    a.await.expect("auth task");
    let valid = b.await.expect("revalidate task").expect("revalidate");

    assert!(valid);
    assert_eq!(
        calls.sorted(),
        sorted(vec!["authenticate", "require", "revalidate"])
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sign_in_double_records_each_method_across_two_threads() {
    let calls = Calls::default();
    let port: Arc<dyn SignInService> = Arc::new(Double(calls.clone()));
    let other = port.clone();
    let binding = BrowserBinding("binding".to_owned());

    let a = tokio::spawn(async move {
        port.begin(start()).await.expect("begin");
        port.complete("github", params(), &binding, "cloud.example", LOOPBACK)
            .await
            .expect("complete");
        port.begin_link(&principal(), "github")
            .await
            .expect("begin_link");
        port.complete_link(
            &principal(),
            "github",
            params(),
            &binding,
            "cloud.example",
            LOOPBACK,
        )
        .await
        .expect("complete_link");
    });
    let b = tokio::spawn(async move {
        other
            .exchange_mobile_code(&token(TokenKind::MobileLoginCode), "verifier", LOOPBACK)
            .await
            .expect("exchange_mobile_code");
        other.list_providers().await.expect("list_providers");
    });
    a.await.expect("first task");
    b.await.expect("second task");

    assert_eq!(
        calls.sorted(),
        sorted(vec![
            "begin",
            "begin_link",
            "complete",
            "complete_link",
            "exchange_mobile_code",
            "list_providers"
        ])
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_double_records_each_method_across_two_threads() {
    let calls = Calls::default();
    let port: Arc<dyn SessionService> = Arc::new(Double(calls.clone()));
    let other = port.clone();

    let a = tokio::spawn(async move {
        port.refresh(&token(TokenKind::Refresh))
            .await
            .expect("refresh");
        port.revoke(id(), id(), RevokeReason::User)
            .await
            .expect("revoke");
        port.revoke_all(id(), RevokeReason::Admin)
            .await
            .expect("revoke_all");
    });
    let b = tokio::spawn(async move {
        other.list(id()).await.expect("list");
        other.touch(&principal()).await.expect("touch");
    });
    a.await.expect("first task");
    b.await.expect("second task");

    assert_eq!(
        calls.sorted(),
        sorted(vec!["list", "refresh", "revoke", "revoke_all", "touch"])
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn device_grant_double_records_each_method_across_two_threads() {
    let calls = Calls::default();
    let port: Arc<dyn DeviceGrantService> = Arc::new(Double(calls.clone()));
    let other = port.clone();

    let a = tokio::spawn(async move {
        let grant = port.request("laptop", "linux", IP).await.expect("request");
        port.poll(&grant.device_code).await.expect("poll");
        port.lookup_for_approval(&principal(), &grant.user_code, IP)
            .await
            .expect("lookup");
        port.decide(&principal(), id(), Decision::Approve)
            .await
            .expect("decide");
    });
    let b = tokio::spawn(async move {
        other.list_devices(id()).await.expect("list_devices");
        other
            .revoke_device(id(), id())
            .await
            .expect("revoke_device");
        other.revoke_self(&principal()).await.expect("revoke_self");
    });
    a.await.expect("first task");
    b.await.expect("second task");

    assert_eq!(
        calls.sorted(),
        sorted(vec![
            "decide",
            "list_devices",
            "lookup_for_approval",
            "poll",
            "request",
            "revoke_device",
            "revoke_self"
        ])
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invite_and_operator_doubles_record_each_method_across_two_threads() {
    let calls = Calls::default();
    let invites: Arc<dyn InviteService> = Arc::new(Double(calls.clone()));
    let operator: Arc<dyn OperatorIdentityOps> = Arc::new(Double(calls.clone()));

    let a = tokio::spawn(async move {
        invites
            .create_invite(id(), "friend")
            .await
            .expect("create_invite");
        invites.list(id()).await.expect("list");
        invites.revoke(id(), id()).await.expect("revoke");
    });
    let b = tokio::spawn(async move {
        let setup = operator
            .ensure_setup_token()
            .await
            .expect("ensure_setup_token");
        operator
            .create_admin_invite()
            .await
            .expect("create_admin_invite");
        operator.reset_admin(id()).await.expect("reset_admin");
        setup.is_some()
    });
    a.await.expect("first task");
    let has_setup = b.await.expect("second task");

    assert!(has_setup);
    assert_eq!(
        calls.sorted(),
        sorted(vec![
            "create_admin_invite",
            "create_invite",
            "ensure_setup_token",
            "invite_list",
            "invite_revoke",
            "reset_admin"
        ])
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn account_admin_double_records_list_accounts_across_two_threads() {
    let calls = Calls::default();
    let port: Arc<dyn AccountAdmin> = Arc::new(Double(calls.clone()));
    let other = port.clone();
    let page = Page {
        offset: 0,
        limit: 50,
    };

    let a = tokio::spawn(async move { port.list_accounts(AccountFilter::default(), page).await });
    let b = tokio::spawn(async move { other.list_accounts(AccountFilter::default(), page).await });
    a.await.expect("first task").expect("list_accounts");
    b.await.expect("second task").expect("list_accounts");

    assert_eq!(calls.sorted(), ["list_accounts", "list_accounts"]);
}

/// The `Arc<dyn Port>` form of every identity trait, for the composition root.
#[test]
fn every_identity_port_is_object_safe_send_and_sync() {
    fn held<T: ?Sized + Send + Sync>(_: Option<Arc<T>>) {}
    held::<dyn Authenticator>(None);
    held::<dyn PrincipalRevalidator>(None);
    held::<dyn AccountGuard>(None);
    held::<dyn SignInService>(None);
    held::<dyn SessionService>(None);
    held::<dyn DeviceGrantService>(None);
    held::<dyn InviteService>(None);
    held::<dyn OperatorIdentityOps>(None);
    held::<dyn AccountAdmin>(None);
}
