//! The recording double of every 01 identity port.

use std::time::Duration;

use crate::common::{Calls, IP, NOW, id, principal};
use contracts::{
    AccountAdmin, AccountAdminError, AccountFilter, AccountGuard, AccountGuardError, AccountInfo,
    AccountState, ApprovalView, Authenticator, AuthenticatorError, AuthorizationRedirect,
    BeginSignIn, BrowserBinding, CallbackParams, Credential, Decision, DeviceAuthorization,
    DeviceGrantError, DeviceGrantService, DeviceInfo, DeviceToken, InviteError, InviteInfo,
    InviteService, InviteToken, Linked, OperatorIdentityError, OperatorIdentityOps, Page,
    Principal, PrincipalRevalidator, ProviderList, RevalidatorError, RevokeReason, Role, Scope,
    SessionError, SessionInfo, SessionService, SessionTokens, SignInError, SignInOutcome,
    SignInService, async_trait,
};
use sc_db::DbTx;
use sc_ids::{RawToken, TokenKind, UserCode, new_token, new_user_code};
use uuid::Uuid;

#[derive(Default)]
pub struct Double(pub Calls);

pub fn token(kind: TokenKind) -> RawToken {
    new_token(kind).expect("token")
}

fn tokens() -> SessionTokens {
    SessionTokens {
        access: token(TokenKind::Session),
        refresh: token(TokenKind::Refresh),
        expires_in: Duration::from_secs(900),
    }
}

fn redirect() -> AuthorizationRedirect {
    AuthorizationRedirect {
        url: "https://idp.example/authorize".to_owned(),
        browser_binding: BrowserBinding("binding".to_owned()),
    }
}

#[async_trait]
impl Authenticator for Double {
    async fn authenticate(&self, _credential: Credential) -> Result<Principal, AuthenticatorError> {
        self.0.record("authenticate");
        Ok(principal())
    }

    fn require(&self, _principal: &Principal, _scope: Scope) -> Result<(), AuthenticatorError> {
        self.0.record("require");
        Ok(())
    }
}

#[async_trait]
impl PrincipalRevalidator for Double {
    async fn revalidate(&self, _principal: &Principal) -> Result<bool, RevalidatorError> {
        self.0.record("revalidate");
        Ok(true)
    }
}

#[async_trait]
impl AccountGuard for Double {
    async fn ensure_active(&self, _tx: &mut DbTx, _account: Uuid) -> Result<(), AccountGuardError> {
        self.0.record("ensure_active");
        Ok(())
    }
}

#[async_trait]
impl SignInService for Double {
    async fn begin(&self, _request: BeginSignIn) -> Result<AuthorizationRedirect, SignInError> {
        self.0.record("begin");
        Ok(redirect())
    }

    async fn complete(
        &self,
        _provider: &str,
        _callback_params: CallbackParams,
        _browser_binding: &BrowserBinding,
        _host: &str,
        _client_ip: std::net::IpAddr,
    ) -> Result<SignInOutcome, SignInError> {
        self.0.record("complete");
        Ok(SignInOutcome::Session {
            tokens: tokens(),
            return_to: "/".to_owned(),
        })
    }

    async fn begin_link(
        &self,
        _principal: &Principal,
        _provider: &str,
    ) -> Result<AuthorizationRedirect, SignInError> {
        self.0.record("begin_link");
        Ok(redirect())
    }

    async fn complete_link(
        &self,
        _principal: &Principal,
        _provider: &str,
        _callback_params: CallbackParams,
        _browser_binding: &BrowserBinding,
        _host: &str,
        _client_ip: std::net::IpAddr,
    ) -> Result<Linked, SignInError> {
        self.0.record("complete_link");
        Ok(Linked)
    }

    async fn exchange_mobile_code(
        &self,
        _code: &RawToken,
        _code_verifier: &str,
        _client_ip: std::net::IpAddr,
    ) -> Result<SessionTokens, SignInError> {
        self.0.record("exchange_mobile_code");
        Ok(tokens())
    }

    async fn list_providers(&self) -> Result<ProviderList, SignInError> {
        self.0.record("list_providers");
        Ok(ProviderList {
            providers: Vec::new(),
            setup_required: true,
        })
    }
}

#[async_trait]
impl SessionService for Double {
    async fn refresh(&self, _refresh_token: &RawToken) -> Result<SessionTokens, SessionError> {
        self.0.record("refresh");
        Ok(tokens())
    }

    async fn revoke(
        &self,
        _account: Uuid,
        _session_id: Uuid,
        _reason: RevokeReason,
    ) -> Result<(), SessionError> {
        self.0.record("revoke");
        Ok(())
    }

    async fn revoke_all(&self, _account: Uuid, _reason: RevokeReason) -> Result<(), SessionError> {
        self.0.record("revoke_all");
        Ok(())
    }

    async fn list(&self, _account: Uuid) -> Result<Vec<SessionInfo>, SessionError> {
        self.0.record("list");
        Ok(Vec::new())
    }

    async fn touch(&self, _principal: &Principal) -> Result<(), SessionError> {
        self.0.record("touch");
        Ok(())
    }
}

#[async_trait]
impl DeviceGrantService for Double {
    async fn request(
        &self,
        _device_name: &str,
        _platform: &str,
        _requester_ip: std::net::IpAddr,
    ) -> Result<DeviceAuthorization, DeviceGrantError> {
        self.0.record("request");
        Ok(DeviceAuthorization {
            device_code: token(TokenKind::DeviceCode),
            user_code: new_user_code().expect("user code"),
            interval: Duration::from_secs(5),
            expires_in: Duration::from_secs(600),
        })
    }

    async fn poll(&self, _device_code: &RawToken) -> Result<DeviceToken, DeviceGrantError> {
        self.0.record("poll");
        Ok(DeviceToken(token(TokenKind::DeviceToken)))
    }

    async fn lookup_for_approval(
        &self,
        _principal: &Principal,
        _user_code: &UserCode,
        approver_ip: std::net::IpAddr,
    ) -> Result<ApprovalView, DeviceGrantError> {
        self.0.record("lookup_for_approval");
        Ok(ApprovalView {
            grant_id: id(),
            expires_at: NOW,
            device: "laptop".to_owned(),
            requester_ip: IP,
            requester_location: None,
            approver_ip,
            approver_location: None,
            mismatch: false,
        })
    }

    async fn decide(
        &self,
        _principal: &Principal,
        _grant_id: Uuid,
        _decision: Decision,
    ) -> Result<(), DeviceGrantError> {
        self.0.record("decide");
        Ok(())
    }

    async fn list_devices(&self, _account: Uuid) -> Result<Vec<DeviceInfo>, DeviceGrantError> {
        self.0.record("list_devices");
        Ok(Vec::new())
    }

    async fn revoke_device(
        &self,
        _account: Uuid,
        _device_id: Uuid,
    ) -> Result<(), DeviceGrantError> {
        self.0.record("revoke_device");
        Ok(())
    }

    async fn revoke_self(&self, _principal: &Principal) -> Result<(), DeviceGrantError> {
        self.0.record("revoke_self");
        Ok(())
    }
}

#[async_trait]
impl InviteService for Double {
    async fn create_invite(
        &self,
        _admin_account: Uuid,
        _note: &str,
    ) -> Result<InviteToken, InviteError> {
        self.0.record("create_invite");
        Ok(InviteToken(token(TokenKind::Invite)))
    }

    async fn list(&self, _admin_account: Uuid) -> Result<Vec<InviteInfo>, InviteError> {
        self.0.record("invite_list");
        Ok(Vec::new())
    }

    async fn revoke(&self, _admin_account: Uuid, _invite_id: Uuid) -> Result<(), InviteError> {
        self.0.record("invite_revoke");
        Ok(())
    }
}

#[async_trait]
impl OperatorIdentityOps for Double {
    async fn ensure_setup_token(&self) -> Result<Option<RawToken>, OperatorIdentityError> {
        self.0.record("ensure_setup_token");
        Ok(Some(token(TokenKind::Setup)))
    }

    async fn create_admin_invite(&self) -> Result<InviteToken, OperatorIdentityError> {
        self.0.record("create_admin_invite");
        Ok(InviteToken(token(TokenKind::Invite)))
    }

    async fn reset_admin(&self, _account: Uuid) -> Result<InviteToken, OperatorIdentityError> {
        self.0.record("reset_admin");
        Ok(InviteToken(token(TokenKind::Invite)))
    }
}

#[async_trait]
impl AccountAdmin for Double {
    async fn list_accounts(
        &self,
        _filter: AccountFilter,
        _page: Page,
    ) -> Result<Vec<AccountInfo>, AccountAdminError> {
        self.0.record("list_accounts");
        Ok(Vec::new())
    }

    async fn lock_active_admins(&self, _tx: &mut DbTx) -> Result<Vec<Uuid>, AccountAdminError> {
        self.0.record("lock_active_admins");
        Ok(Vec::new())
    }

    async fn set_role(
        &self,
        _tx: &mut DbTx,
        _account: Uuid,
        _role: Role,
    ) -> Result<(), AccountAdminError> {
        self.0.record("set_role");
        Ok(())
    }

    async fn set_state(
        &self,
        _tx: &mut DbTx,
        _account: Uuid,
        _state: AccountState,
    ) -> Result<(), AccountAdminError> {
        self.0.record("set_state");
        Ok(())
    }

    async fn begin_deletion(
        &self,
        _tx: &mut DbTx,
        _account: Uuid,
    ) -> Result<(), AccountAdminError> {
        self.0.record("begin_deletion");
        Ok(())
    }

    async fn finalize_deletion(
        &self,
        _tx: &mut DbTx,
        _account: Uuid,
    ) -> Result<(), AccountAdminError> {
        self.0.record("finalize_deletion");
        Ok(())
    }
}
