//! The whole server with its vault slot filled by a recording stub (a v1.2 build in Mode S); the authenticator
//! knows the member (a session) and the device.

use std::net::SocketAddr;
use std::sync::Arc;

use sunnycloud_http_api::{ApiServices, HttpServer};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::auth::support::{DEVICE_TOKEN, MEMBER_TOKEN, ScriptedAuthenticator};
use crate::harness::recording::RecordingVault;
use crate::harness::{
    EVENT_BOUND, SESSION_COOKIE, Script, default_config, new_metrics, stub_services,
};
use crate::worker::support::{Reply, send};

pub const VAULT: Uuid = Uuid::from_u128(0x7A);
pub const ROOT: Uuid = Uuid::from_u128(0x7B);
/// Tags the scripted port hands back for the headers H0 and H1 (64 hex digits; the stub hashes nothing, the real
/// derivation is checked against PostgreSQL in the server crate).
pub const H0_TAG: &str = "a3a8b5f6a1e7c4f7df8f8eb0f8c2f1f1a7c6d4f1d0d3d4f9f1b8a1e9f5c2d1a0";
pub const H1_TAG: &str = "b4b9c6a7b2f8d5a8e0a9a9c1a9d3a2a2b8d7e5a2e1e4e5a0a2c9b2f0a6d3e2b1";

pub struct Stack {
    pub public: SocketAddr,
    pub script: Script,
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

impl Stack {
    pub async fn start() -> Stack {
        let script = Script::default();
        let services = ApiServices {
            authenticator: Arc::new(ScriptedAuthenticator::default()),
            vault: Some(RecordingVault::new(&script)),
            ..stub_services(&script, new_metrics())
        };
        let bound = HttpServer::new(services, default_config())
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

    /// The member's session as a bearer, with `headers` added.
    pub async fn as_member(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> Reply {
        let auth = format!("Bearer {MEMBER_TOKEN}");
        let mut all = vec![("authorization", auth.as_str())];
        all.extend_from_slice(headers);
        self.call(method, path, &all, body).await
    }

    pub async fn as_device(&self, method: &str, path: &str, body: &str) -> Reply {
        let auth = format!("Bearer {DEVICE_TOKEN}");
        self.call(method, path, &[("authorization", auth.as_str())], body)
            .await
    }

    /// The member's session as the browser cookie, with `headers` added.
    pub async fn with_cookie(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> Reply {
        let cookie = format!("{SESSION_COOKIE}={MEMBER_TOKEN}");
        let mut all = vec![("cookie", cookie.as_str())];
        all.extend_from_slice(headers);
        self.call(method, path, &all, body).await
    }

    /// The log lines of the vault port, in order.
    pub fn vault_calls(&self) -> Vec<String> {
        self.script
            .log()
            .into_iter()
            .filter(|line| line.starts_with("VaultService::"))
            .collect()
    }
}

pub fn quoted(tag: &str) -> String {
    format!("\"{tag}\"")
}
