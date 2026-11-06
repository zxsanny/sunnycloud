//! Value types of 01 identity. A type that carries a token prints a placeholder in `Debug` (NFT-SEC-16).

use std::fmt;
use std::time::Duration;

use sc_clock::Timestamp;
use sc_ids::{RawToken, UserCode};
use uuid::Uuid;

/// What a caller presents; the token is a [`RawToken`], whose `Debug` is a placeholder.
#[derive(Debug)]
pub enum Credential {
    Session(RawToken),
    Device(RawToken),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrincipalKind {
    Session,
    Device,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Admin,
}

/// The authenticated caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    pub account_id: Uuid,
    pub kind: PrincipalKind,
    pub session_or_device_id: Uuid,
    pub role: Role,
    pub label: String,
}

/// What a route requires of its caller. Device tokens never satisfy `SessionOnly` or `Admin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Public,
    SessionOnly,
    SessionOrDevice,
    Admin,
}

/// The states `AccountAdmin::set_state` can apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountState {
    Active,
    Disabled,
}

/// The states a listed account can be in; `Deleting` is entered only through `begin_deletion`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountStatus {
    Active,
    Disabled,
    Deleting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientKind {
    Web,
    Android,
    Ios,
    Desktop,
}

impl ClientKind {
    /// Mobile kinds must send a `code_challenge` to `SignInService::begin`.
    pub fn is_mobile(self) -> bool {
        matches!(self, ClientKind::Android | ClientKind::Ios)
    }
}

#[derive(Debug)]
pub struct SessionTokens {
    pub access: RawToken,
    pub refresh: RawToken,
    pub expires_in: Duration,
}

/// The sync-only device token, shown once.
#[derive(Debug)]
pub struct DeviceToken(pub RawToken);

/// The plaintext invite or setup token, shown once.
#[derive(Debug)]
pub struct InviteToken(pub RawToken);

pub struct DeviceAuthorization {
    pub device_code: RawToken,
    pub user_code: UserCode,
    pub interval: Duration,
    pub expires_in: Duration,
}

impl fmt::Debug for DeviceAuthorization {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceAuthorization")
            .field("device_code", &"<redacted>")
            .field("user_code", &"<redacted>")
            .field("interval", &self.interval)
            .field("expires_in", &self.expires_in)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderInfo {
    pub key: String,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub id: Uuid,
    pub label: String,
    pub created_at: Timestamp,
    pub last_seen_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub id: Uuid,
    pub name: String,
    pub platform: String,
    pub created_at: Timestamp,
    pub last_seen_at: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InviteInfo {
    pub id: Uuid,
    pub note: String,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
    pub redeemed_at: Option<Timestamp>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountInfo {
    pub id: Uuid,
    pub display_name: String,
    pub role: Role,
    pub state: AccountStatus,
    pub last_login: Option<Timestamp>,
}

/// Filter of `AccountAdmin::list_accounts`; `None` matches any.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AccountFilter {
    pub role: Option<Role>,
    pub state: Option<AccountStatus>,
}

/// Why a session is revoked (the `revoke_reason` values of the data model).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevokeReason {
    User,
    Admin,
    RefreshReuse,
    AccountDeleted,
}
