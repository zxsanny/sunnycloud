use std::fmt;

use serde::{Deserialize, Serialize};

/// The answer of `POST /api/v1/device-grants`; `interval` and `expires_in` are seconds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct DeviceAuthorization {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u64,
    pub expires_in: u64,
}

/// The 200 answer of `POST /api/v1/auth/mobile-code-exchanges` and `POST /api/v1/auth/refresh-tokens`;
/// `expires_in` is seconds. Token values are secrets: `Debug` never prints them.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct SessionTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
}

impl fmt::Debug for SessionTokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionTokens")
            .field("expires_in", &self.expires_in)
            .finish_non_exhaustive()
    }
}

/// The 200 answer of `POST /api/v1/device-tokens`: the desktop device token, which never expires on its own
/// and never rotates. `Debug` never prints it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]
pub struct DeviceToken {
    pub access_token: String,
    pub token_type: TokenType,
}

impl fmt::Debug for DeviceToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceToken")
            .field("token_type", &self.token_type)
            .finish_non_exhaustive()
    }
}

/// The server answers `"Bearer"` only; any other value is not a device token answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum TokenType {
    Bearer,
}

/// The four RFC 8628 §3.5 refusals of a poll, answered as 400 with these words in `code` and `error`
/// (`api_v1_surface.md`, Devices).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum DeviceRefusal {
    AuthorizationPending,
    SlowDown,
    AccessDenied,
    ExpiredToken,
}

impl DeviceRefusal {
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "authorization_pending" => Some(Self::AuthorizationPending),
            "slow_down" => Some(Self::SlowDown),
            "access_denied" => Some(Self::AccessDenied),
            "expired_token" => Some(Self::ExpiredToken),
            _ => None,
        }
    }
}

/// One answer of the device-grant poll: the token, or the outcome of a refusal.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "uniffi", derive(uniffi::Enum))]
pub enum DevicePoll {
    Token(DeviceToken),
    Pending,
    SlowDown,
    Denied,
    Expired,
}

impl From<DeviceRefusal> for DevicePoll {
    fn from(refusal: DeviceRefusal) -> Self {
        match refusal {
            DeviceRefusal::AuthorizationPending => Self::Pending,
            DeviceRefusal::SlowDown => Self::SlowDown,
            DeviceRefusal::AccessDenied => Self::Denied,
            DeviceRefusal::ExpiredToken => Self::Expired,
        }
    }
}
