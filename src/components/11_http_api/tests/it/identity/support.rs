use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use contracts::{
    AuthorizationRedirect, BrowserBinding, DeviceAuthorization, ProviderInfo, ProviderList,
    QuotaView, SessionTokens,
};
use sc_clock::Timestamp;
use sc_ids::{RawToken, TokenKind, UserCode};
use sc_observability::Metrics;
use sunnycloud_http_api::{ApiServices, HttpConfig, HttpServer};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::auth::support::{DEVICE_TOKEN, MEMBER_TOKEN, ScriptedAuthenticator, ScriptedWorkerAuth};
use crate::harness::recording_identity::{
    RecordingAccounts, RecordingAdmin, RecordingDeletion, RecordingDeviceGrants, RecordingInvites,
    RecordingSessions, RecordingSignIn,
};
use crate::harness::{
    BINDING_COOKIE, EVENT_BOUND, Script, default_config, new_metrics, stub_services,
};
use crate::worker::support::{Reply, send};

pub const GRANT: Uuid = Uuid::from_u128(0xC1);
pub const OTHER: Uuid = Uuid::from_u128(0xC2);
pub const INVITE: Uuid = Uuid::from_u128(0xC3);
pub const TARGET: Uuid = Uuid::from_u128(0xC4);

pub const WHEN: Timestamp = Timestamp::from_unix_micros(1_793_956_272_123_456);
pub const WHEN_TEXT: &str = "2026-11-06T09:11:12.123456Z";

pub const ACCESS: &str = "acc-ESS_token";
pub const REFRESH: &str = "ref-RESH_token";

pub fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

/// The whole server over the recording stubs; the authenticator knows the member, the admin and the device.
pub struct Stack {
    pub public: SocketAddr,
    pub script: Script,
    pub metrics: Arc<Metrics>,
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

impl Stack {
    pub async fn start() -> Stack {
        Stack::start_with(|_| {}).await
    }

    /// As [`Stack::start`], with `tune` free to change the configuration (a trusted proxy, for one).
    pub async fn start_with(tune: impl FnOnce(&mut HttpConfig)) -> Stack {
        let script = Script::default();
        let metrics = new_metrics();
        let services = ApiServices {
            authenticator: Arc::new(ScriptedAuthenticator::default()),
            worker_auth: Arc::new(ScriptedWorkerAuth::default()),
            sign_in: RecordingSignIn::new(&script),
            sessions: RecordingSessions::new(&script),
            device_grants: RecordingDeviceGrants::new(&script),
            invites: RecordingInvites::new(&script),
            admin: RecordingAdmin::new(&script),
            accounts: RecordingAccounts::new(&script),
            account_deletion: RecordingDeletion::new(&script),
            ..stub_services(&script, metrics.clone())
        };
        let mut config = default_config();
        tune(&mut config);
        let bound = HttpServer::new(services, config)
            .expect("server builds")
            .bind()
            .await
            .expect("listeners bind");
        let public = bound.public_addr();
        let (stop, signal) = oneshot::channel::<()>();
        let task = tokio::spawn(async move {
            bound
                .serve(async move {
                    signal.await.ok();
                })
                .await
                .expect("serve returns");
        });
        Stack {
            public,
            script,
            metrics,
            stop,
            task,
        }
    }

    pub async fn stop(self) {
        self.stop.send(()).expect("server is running");
        tokio::time::timeout(EVENT_BOUND, self.task)
            .await
            .expect("serve returns after the signal")
            .expect("serve does not panic");
    }

    pub async fn call(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> Reply {
        send(self.public, method, path, headers, body.as_bytes()).await
    }

    /// A call with no credential, as the browser or a daemon makes the public ones.
    pub async fn anonymous(&self, method: &str, path: &str, body: &str) -> Reply {
        self.call(method, path, &[], body).await
    }

    pub async fn with_token(&self, token: &str, method: &str, path: &str, body: &str) -> Reply {
        let auth = bearer(token);
        self.call(method, path, &[("authorization", auth.as_str())], body)
            .await
    }

    pub async fn as_member(&self, method: &str, path: &str, body: &str) -> Reply {
        self.with_token(MEMBER_TOKEN, method, path, body).await
    }

    pub async fn as_device(&self, method: &str, path: &str, body: &str) -> Reply {
        self.with_token(DEVICE_TOKEN, method, path, body).await
    }

    /// The log lines of the calls whose port method starts with `key`, in order.
    pub fn log(&self, key: &str) -> Vec<String> {
        self.script
            .log()
            .into_iter()
            .filter(|line| line.starts_with(key))
            .collect()
    }

    /// How many port calls were made in all.
    pub fn calls(&self) -> usize {
        self.script.log().len()
    }
}

pub fn token(kind: TokenKind, value: &str) -> RawToken {
    RawToken::from_presented(kind, value.to_owned())
}

pub fn session_tokens() -> SessionTokens {
    SessionTokens {
        access: token(TokenKind::Session, ACCESS),
        refresh: token(TokenKind::Refresh, REFRESH),
        expires_in: Duration::from_secs(900),
    }
}

pub fn redirect_to_provider() -> AuthorizationRedirect {
    AuthorizationRedirect {
        url: "https://idp.test/authorize?state=s1".to_owned(),
        browser_binding: BrowserBinding("bind-ING_1".to_owned()),
    }
}

pub fn providers() -> ProviderList {
    ProviderList {
        providers: vec![ProviderInfo {
            key: "github".to_owned(),
            display_name: "GitHub".to_owned(),
        }],
        setup_required: true,
    }
}

pub fn quota() -> QuotaView {
    QuotaView {
        limit_bytes: None,
        effective_limit: 1_000_000,
        used: 400,
        reserved: 25,
    }
}

pub fn device_grant() -> DeviceAuthorization {
    DeviceAuthorization {
        device_code: token(TokenKind::DeviceCode, "dc-DEVICE_code"),
        user_code: sc_ids::new_user_code().expect("a user code"),
        interval: Duration::from_secs(5),
        expires_in: Duration::from_secs(600),
    }
}

pub fn user_code_text(grant: &DeviceAuthorization) -> String {
    let code: &UserCode = &grant.user_code;
    code.as_str().to_owned()
}

pub fn binding_cookie_header(value: &str) -> String {
    format!("{BINDING_COOKIE}={value}")
}
