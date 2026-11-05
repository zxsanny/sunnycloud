use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceAuthorization {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u64,
}

/// Token values are secrets: `Debug` never prints them.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionTokens {
    pub access: String,
    /// Absent for a desktop device token, which never rotates (13 §2).
    pub refresh: Option<String>,
    pub expires_in: u64,
}

impl fmt::Debug for SessionTokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionTokens")
            .field("expires_in", &self.expires_in)
            .finish_non_exhaustive()
    }
}

/// One answer of the device-grant poll.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DevicePoll {
    Token(SessionTokens),
    Pending,
    SlowDown,
    Denied,
    Expired,
}
