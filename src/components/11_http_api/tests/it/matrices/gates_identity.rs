//! Per-account fakes of 01's device-grant and session ports (see [`super::gates`]).

use std::net::IpAddr;

use contracts::*;
use sc_ids::{RawToken, UserCode};
use uuid::Uuid;

use super::gates::Gate;
use crate::harness::recording_identity::{RecordingDeviceGrants, RecordingSessions};

#[contracts::async_trait]
impl DeviceGrantService for Gate<RecordingDeviceGrants> {
    async fn request(
        &self,
        device_name: &str,
        platform: &str,
        requester_ip: IpAddr,
    ) -> Result<DeviceAuthorization, DeviceGrantError> {
        self.inner
            .request(device_name, platform, requester_ip)
            .await
    }

    async fn poll(&self, device_code: &RawToken) -> Result<DeviceToken, DeviceGrantError> {
        self.inner.poll(device_code).await
    }

    async fn lookup_for_approval(
        &self,
        principal: &Principal,
        user_code: &UserCode,
        approver_ip: IpAddr,
    ) -> Result<ApprovalView, DeviceGrantError> {
        self.inner
            .lookup_for_approval(principal, user_code, approver_ip)
            .await
    }

    async fn decide(
        &self,
        principal: &Principal,
        grant_id: Uuid,
        decision: Decision,
    ) -> Result<(), DeviceGrantError> {
        if !self.admit(
            "DeviceGrantService::decide",
            principal.account_id,
            &[grant_id],
        ) {
            return Err(DeviceGrantError::NotFound);
        }
        self.inner.decide(principal, grant_id, decision).await
    }

    async fn list_devices(&self, account: Uuid) -> Result<Vec<DeviceInfo>, DeviceGrantError> {
        self.inner.list_devices(account).await
    }

    async fn revoke_device(&self, account: Uuid, device_id: Uuid) -> Result<(), DeviceGrantError> {
        if !self.admit("DeviceGrantService::revoke_device", account, &[device_id]) {
            return Err(DeviceGrantError::NotFound);
        }
        self.inner.revoke_device(account, device_id).await
    }

    async fn revoke_self(&self, principal: &Principal) -> Result<(), DeviceGrantError> {
        self.inner.revoke_self(principal).await
    }
}

#[contracts::async_trait]
impl SessionService for Gate<RecordingSessions> {
    async fn refresh(&self, refresh_token: &RawToken) -> Result<SessionTokens, SessionError> {
        self.inner.refresh(refresh_token).await
    }

    /// 01's `revoke` has no not-found outcome: a session the account does not own is left alone, and the call
    /// succeeds as for any other id.
    async fn revoke(
        &self,
        account: Uuid,
        session_id: Uuid,
        reason: RevokeReason,
    ) -> Result<(), SessionError> {
        if !self.admit("SessionService::revoke", account, &[session_id]) {
            return Ok(());
        }
        self.inner.revoke(account, session_id, reason).await
    }

    async fn revoke_all(&self, account: Uuid, reason: RevokeReason) -> Result<(), SessionError> {
        self.inner.revoke_all(account, reason).await
    }

    async fn list(&self, account: Uuid) -> Result<Vec<SessionInfo>, SessionError> {
        self.inner.list(account).await
    }

    async fn touch(&self, principal: &Principal) -> Result<(), SessionError> {
        self.inner.touch(principal).await
    }
}
