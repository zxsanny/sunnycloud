use std::net::IpAddr;
use std::time::Duration;

use async_trait::async_trait;
use sc_clock::Timestamp;
use sc_ids::{RawToken, UserCode};
use thiserror::Error;
use uuid::Uuid;

use super::model::{DeviceAuthorization, DeviceInfo, DeviceToken, Principal};
use crate::common::Fault;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Approve,
    Deny,
}

/// What the approver sees. `grant_id` is the id `decide` takes; `expires_at` is the code's expiry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalView {
    pub grant_id: Uuid,
    pub expires_at: Timestamp,
    pub device: String,
    pub requester_ip: IpAddr,
    pub requester_location: Option<String>,
    pub approver_ip: IpAddr,
    pub approver_location: Option<String>,
    pub mismatch: bool,
}

#[derive(Debug, Error)]
pub enum DeviceGrantError {
    #[error("authorization pending")]
    AuthorizationPending,
    #[error("slow down")]
    SlowDown,
    #[error("access denied")]
    AccessDenied,
    #[error("expired token")]
    ExpiredToken,
    #[error("unknown code")]
    UnknownCode,
    #[error("rate limited")]
    RateLimited { retry_after: Duration },
    #[error("expired")]
    Expired,
    #[error("already decided")]
    AlreadyDecided,
    #[error("not found")]
    NotFound,
    #[error("unexpected failure")]
    Unexpected(#[source] Fault),
}

/// RFC 8628 device grant for the desktop client, and the device list.
#[async_trait]
pub trait DeviceGrantService: Send + Sync {
    async fn request(
        &self,
        device_name: &str,
        platform: &str,
        requester_ip: IpAddr,
    ) -> Result<DeviceAuthorization, DeviceGrantError>;

    /// Yields the token once.
    async fn poll(&self, device_code: &RawToken) -> Result<DeviceToken, DeviceGrantError>;

    async fn lookup_for_approval(
        &self,
        principal: &Principal,
        user_code: &UserCode,
        approver_ip: IpAddr,
    ) -> Result<ApprovalView, DeviceGrantError>;

    async fn decide(
        &self,
        principal: &Principal,
        grant_id: Uuid,
        decision: Decision,
    ) -> Result<(), DeviceGrantError>;

    async fn list_devices(&self, account: Uuid) -> Result<Vec<DeviceInfo>, DeviceGrantError>;

    async fn revoke_device(&self, account: Uuid, device_id: Uuid) -> Result<(), DeviceGrantError>;

    /// A device token removes only its own row.
    async fn revoke_self(&self, principal: &Principal) -> Result<(), DeviceGrantError>;
}
