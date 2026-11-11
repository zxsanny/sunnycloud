//! Recording stubs of the identity, account and admin ports. Each call is one line of the ordered log of the
//! [`Script`]. A token, a code or a callback parameter prints a placeholder in `Debug`, so the ports that receive
//! one write it out here: the log shows what reached the port.

use std::net::IpAddr;
use std::sync::Arc;

use contracts::*;
use sc_ids::{RawToken, UserCode};
use uuid::Uuid;

use super::recording::recording_port;
use super::script::{Script, unscripted};

recording_port!(RecordingAccounts: AccountService, AccountServiceError, {
    async fn view(&self, principal: &Principal) -> Result<AccountView, AccountServiceError>;
    async fn quota_view(&self, principal: &Principal) -> Result<QuotaView, AccountServiceError>;
});

recording_port!(RecordingDeletion: AccountDeletionService, DeletionError, {
    async fn request(&self, principal: &Principal, confirmation: &str)
        -> Result<DeletionAccepted, DeletionError>;
});

recording_port!(RecordingInvites: InviteService, InviteError, {
    async fn create_invite(&self, admin_account: Uuid, note: &str)
        -> Result<InviteToken, InviteError>;
    async fn list(&self, admin_account: Uuid) -> Result<Vec<InviteInfo>, InviteError>;
    async fn revoke(&self, admin_account: Uuid, invite_id: Uuid) -> Result<(), InviteError>;
});

/// 01's sign-in port.
pub struct RecordingSignIn(Script);

impl RecordingSignIn {
    pub fn new(script: &Script) -> Arc<Self> {
        Arc::new(Self(script.clone()))
    }

    fn next<T: 'static>(&self, key: &'static str) -> Result<T, SignInError> {
        self.0
            .take::<Result<T, SignInError>>(key)
            .unwrap_or_else(|| Err(SignInError::Unexpected(unscripted(key))))
    }
}

fn token_text(token: &Option<RawToken>) -> String {
    token
        .as_ref()
        .map_or("none".to_owned(), |token| token.expose().to_owned())
}

#[contracts::async_trait]
impl SignInService for RecordingSignIn {
    async fn begin(&self, request: BeginSignIn) -> Result<AuthorizationRedirect, SignInError> {
        self.0.record(format!(
            "SignInService::begin provider={} return_to={} kind={:?} invite={} setup={} challenge={:?} host={}",
            request.provider,
            request.return_to,
            request.client_kind,
            token_text(&request.invite_token),
            token_text(&request.setup_token),
            request.code_challenge,
            request.host
        ));
        self.next("SignInService::begin")
    }

    async fn complete(
        &self,
        provider: &str,
        callback_params: CallbackParams,
        browser_binding: &BrowserBinding,
        host: &str,
    ) -> Result<SignInOutcome, SignInError> {
        let params: Vec<String> = callback_params
            .0
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect();
        self.0.record(format!(
            "SignInService::complete provider={provider} params={} binding={} host={host}",
            params.join("&"),
            browser_binding.0
        ));
        self.next("SignInService::complete")
    }

    async fn begin_link(
        &self,
        principal: &Principal,
        provider: &str,
    ) -> Result<AuthorizationRedirect, SignInError> {
        self.0.record(format!(
            "SignInService::begin_link {} {provider}",
            principal.account_id
        ));
        self.next("SignInService::begin_link")
    }

    async fn complete_link(
        &self,
        principal: &Principal,
        provider: &str,
        _callback_params: CallbackParams,
        _browser_binding: &BrowserBinding,
        _host: &str,
    ) -> Result<Linked, SignInError> {
        self.0.record(format!(
            "SignInService::complete_link {} {provider}",
            principal.account_id
        ));
        self.next("SignInService::complete_link")
    }

    async fn exchange_mobile_code(
        &self,
        code: &RawToken,
        code_verifier: &str,
    ) -> Result<SessionTokens, SignInError> {
        self.0.record(format!(
            "SignInService::exchange_mobile_code {} {code_verifier}",
            code.expose()
        ));
        self.next("SignInService::exchange_mobile_code")
    }

    async fn list_providers(&self) -> Result<ProviderList, SignInError> {
        self.0.record("SignInService::list_providers".to_owned());
        self.next("SignInService::list_providers")
    }
}

/// 01's session port.
pub struct RecordingSessions(Script);

impl RecordingSessions {
    pub fn new(script: &Script) -> Arc<Self> {
        Arc::new(Self(script.clone()))
    }

    fn next<T: 'static>(&self, key: &'static str) -> Result<T, SessionError> {
        self.0
            .take::<Result<T, SessionError>>(key)
            .unwrap_or_else(|| Err(SessionError::Unexpected(unscripted(key))))
    }
}

#[contracts::async_trait]
impl SessionService for RecordingSessions {
    async fn refresh(&self, refresh_token: &RawToken) -> Result<SessionTokens, SessionError> {
        self.0.record(format!(
            "SessionService::refresh {}",
            refresh_token.expose()
        ));
        self.next("SessionService::refresh")
    }

    async fn revoke(
        &self,
        account: Uuid,
        session_id: Uuid,
        reason: RevokeReason,
    ) -> Result<(), SessionError> {
        self.0.record(format!(
            "SessionService::revoke ({account}, {session_id}, {reason:?})"
        ));
        self.next("SessionService::revoke")
    }

    async fn revoke_all(&self, account: Uuid, reason: RevokeReason) -> Result<(), SessionError> {
        self.0.record(format!(
            "SessionService::revoke_all ({account}, {reason:?})"
        ));
        self.next("SessionService::revoke_all")
    }

    async fn list(&self, account: Uuid) -> Result<Vec<SessionInfo>, SessionError> {
        self.0.record(format!("SessionService::list ({account})"));
        self.next("SessionService::list")
    }

    async fn touch(&self, principal: &Principal) -> Result<(), SessionError> {
        self.0
            .record(format!("SessionService::touch ({})", principal.account_id));
        self.next("SessionService::touch")
    }
}

/// 01's device-grant port.
pub struct RecordingDeviceGrants(Script);

impl RecordingDeviceGrants {
    pub fn new(script: &Script) -> Arc<Self> {
        Arc::new(Self(script.clone()))
    }

    fn next<T: 'static>(&self, key: &'static str) -> Result<T, DeviceGrantError> {
        self.0
            .take::<Result<T, DeviceGrantError>>(key)
            .unwrap_or_else(|| Err(DeviceGrantError::Unexpected(unscripted(key))))
    }
}

#[contracts::async_trait]
impl DeviceGrantService for RecordingDeviceGrants {
    async fn request(
        &self,
        device_name: &str,
        platform: &str,
        requester_ip: IpAddr,
    ) -> Result<DeviceAuthorization, DeviceGrantError> {
        self.0.record(format!(
            "DeviceGrantService::request {device_name} {platform} {requester_ip}"
        ));
        self.next("DeviceGrantService::request")
    }

    async fn poll(&self, device_code: &RawToken) -> Result<DeviceToken, DeviceGrantError> {
        self.0
            .record(format!("DeviceGrantService::poll {}", device_code.expose()));
        self.next("DeviceGrantService::poll")
    }

    async fn lookup_for_approval(
        &self,
        principal: &Principal,
        user_code: &UserCode,
        approver_ip: IpAddr,
    ) -> Result<ApprovalView, DeviceGrantError> {
        self.0.record(format!(
            "DeviceGrantService::lookup_for_approval {} {} {approver_ip}",
            principal.account_id,
            user_code.as_str()
        ));
        self.next("DeviceGrantService::lookup_for_approval")
    }

    async fn decide(
        &self,
        principal: &Principal,
        grant_id: Uuid,
        decision: Decision,
    ) -> Result<(), DeviceGrantError> {
        self.0.record(format!(
            "DeviceGrantService::decide {} {grant_id} {decision:?}",
            principal.account_id
        ));
        self.next("DeviceGrantService::decide")
    }

    async fn list_devices(&self, account: Uuid) -> Result<Vec<DeviceInfo>, DeviceGrantError> {
        self.0
            .record(format!("DeviceGrantService::list_devices ({account})"));
        self.next("DeviceGrantService::list_devices")
    }

    async fn revoke_device(&self, account: Uuid, device_id: Uuid) -> Result<(), DeviceGrantError> {
        self.0.record(format!(
            "DeviceGrantService::revoke_device ({account}, {device_id})"
        ));
        self.next("DeviceGrantService::revoke_device")
    }

    async fn revoke_self(&self, principal: &Principal) -> Result<(), DeviceGrantError> {
        self.0.record(format!(
            "DeviceGrantService::revoke_self {} {:?} {}",
            principal.account_id, principal.kind, principal.session_or_device_id
        ));
        self.next("DeviceGrantService::revoke_self")
    }
}

/// 07's admin port. `health` has no error channel, so an unscripted call reports a down item naming the call.
pub struct RecordingAdmin(Script);

impl RecordingAdmin {
    pub fn new(script: &Script) -> Arc<Self> {
        Arc::new(Self(script.clone()))
    }

    fn next<T: 'static>(&self, key: &'static str) -> Result<T, AdminError> {
        self.0
            .take::<Result<T, AdminError>>(key)
            .unwrap_or_else(|| Err(AdminError::Unexpected(unscripted(key))))
    }
}

fn actor_text(actor: &Actor) -> String {
    match actor {
        Actor::Admin(principal) => format!("admin {}", principal.account_id),
        Actor::Operator => "operator".to_owned(),
    }
}

#[contracts::async_trait]
impl AdminService for RecordingAdmin {
    async fn list_users(
        &self,
        actor: &Actor,
        page: Page,
    ) -> Result<Vec<AdminUserView>, AdminError> {
        self.0.record(format!(
            "AdminService::list_users {} offset={} limit={}",
            actor_text(actor),
            page.offset,
            page.limit
        ));
        self.next("AdminService::list_users")
    }

    async fn update_user(
        &self,
        actor: &Actor,
        account: Uuid,
        role: Option<Role>,
        state: Option<AccountState>,
    ) -> Result<(), AdminError> {
        self.0.record(format!(
            "AdminService::update_user {} {account} {role:?} {state:?}",
            actor_text(actor)
        ));
        self.next("AdminService::update_user")
    }

    async fn set_quota(
        &self,
        actor: &Actor,
        account: Uuid,
        limit: Option<i64>,
    ) -> Result<(), AdminError> {
        self.0.record(format!(
            "AdminService::set_quota {} {account} {limit:?}",
            actor_text(actor)
        ));
        self.next("AdminService::set_quota")
    }

    async fn health(&self, actor: &Actor) -> HealthReport {
        self.0
            .record(format!("AdminService::health {}", actor_text(actor)));
        let key = "AdminService::health";
        self.0.take(key).unwrap_or_else(|| HealthReport {
            items: vec![(
                key.to_owned(),
                HealthItem::new(HealthStatus::Down, "unscripted port call"),
            )],
        })
    }
}
