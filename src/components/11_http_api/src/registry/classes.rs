use axum::routing::MethodFilter;
use sc_error::ErrorCode;

/// The methods the surface uses; `Head` is a separate operation from `Get`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HttpMethod {
    Get,
    Head,
    Post,
    Put,
    Patch,
    Delete,
}

impl HttpMethod {
    pub const ALL: [HttpMethod; 6] = [
        HttpMethod::Get,
        HttpMethod::Head,
        HttpMethod::Post,
        HttpMethod::Put,
        HttpMethod::Patch,
        HttpMethod::Delete,
    ];

    /// The key of the operation in an OpenAPI path item.
    pub const fn key(self) -> &'static str {
        match self {
            HttpMethod::Get => "get",
            HttpMethod::Head => "head",
            HttpMethod::Post => "post",
            HttpMethod::Put => "put",
            HttpMethod::Patch => "patch",
            HttpMethod::Delete => "delete",
        }
    }

    pub fn from_key(key: &str) -> Option<HttpMethod> {
        HttpMethod::ALL
            .into_iter()
            .find(|method| method.key() == key)
    }

    pub const fn filter(self) -> MethodFilter {
        match self {
            HttpMethod::Get => MethodFilter::GET,
            HttpMethod::Head => MethodFilter::HEAD,
            HttpMethod::Post => MethodFilter::POST,
            HttpMethod::Put => MethodFilter::PUT,
            HttpMethod::Patch => MethodFilter::PATCH,
            HttpMethod::Delete => MethodFilter::DELETE,
        }
    }

    pub(crate) const fn mutates_by_default(self) -> bool {
        !matches!(self, HttpMethod::Get | HttpMethod::Head)
    }
}

/// Which listener serves an operation: the public one or the internal (worker) one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ListenerKind {
    Public,
    Internal,
}

impl ListenerKind {
    pub const fn name(self) -> &'static str {
        match self {
            ListenerKind::Public => "public",
            ListenerKind::Internal => "internal",
        }
    }
}

const ADMIN_SESSION: u8 = 1;
const USER_SESSION: u8 = 2;
const DEVICE: u8 = 4;
const GRANT: u8 = 8;
const WORKER: u8 = 16;

/// The credential an operation demands (`api_v1_surface.md`, auth classes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AuthClass {
    Public,
    Session,
    SessionOrDevice,
    DeviceOnly,
    SessionOnly,
    Admin,
    ShareGrant,
    Worker,
}

impl AuthClass {
    pub const ALL: [AuthClass; 8] = [
        AuthClass::Public,
        AuthClass::Session,
        AuthClass::SessionOrDevice,
        AuthClass::DeviceOnly,
        AuthClass::SessionOnly,
        AuthClass::Admin,
        AuthClass::ShareGrant,
        AuthClass::Worker,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            AuthClass::Public => "Public",
            AuthClass::Session => "Session",
            AuthClass::SessionOrDevice => "SessionOrDevice",
            AuthClass::DeviceOnly => "DeviceOnly",
            AuthClass::SessionOnly => "SessionOnly",
            AuthClass::Admin => "Admin",
            AuthClass::ShareGrant => "ShareGrant",
            AuthClass::Worker => "Worker",
        }
    }

    pub fn from_name(name: &str) -> Option<AuthClass> {
        AuthClass::ALL
            .into_iter()
            .find(|class| class.name() == name)
    }

    /// The principals the class lets through, as a set; a class that admits a superset of another is
    /// a loosening of it.
    const fn admits(self) -> u8 {
        match self {
            AuthClass::Public => u8::MAX,
            AuthClass::Session | AuthClass::SessionOnly => ADMIN_SESSION | USER_SESSION,
            AuthClass::SessionOrDevice => ADMIN_SESSION | USER_SESSION | DEVICE,
            AuthClass::DeviceOnly => DEVICE,
            AuthClass::Admin => ADMIN_SESSION,
            AuthClass::ShareGrant => GRANT,
            AuthClass::Worker => WORKER,
        }
    }

    /// True when `self` admits every principal `other` admits.
    pub fn loosens(self, other: AuthClass) -> bool {
        self.admits() & other.admits() == other.admits()
    }

    /// The OpenAPI security requirements: alternatives, each a scheme name.
    pub(crate) fn security_schemes(self) -> &'static [&'static str] {
        match self {
            AuthClass::Public | AuthClass::ShareGrant => &[],
            AuthClass::Session | AuthClass::SessionOnly | AuthClass::Admin => {
                &["cookieSession", "bearerSession"]
            }
            AuthClass::SessionOrDevice => &["cookieSession", "bearerSession", "bearerDevice"],
            AuthClass::DeviceOnly => &["bearerDevice"],
            AuthClass::Worker => &["bearerWorker"],
        }
    }

    /// Codes the credential check itself can answer with, on top of the route's own.
    pub(crate) fn implied_errors(self) -> &'static [ErrorCode] {
        match self {
            AuthClass::Public => &[],
            AuthClass::ShareGrant => &[ErrorCode::PasswordRequired, ErrorCode::UpgradeRequired],
            AuthClass::Worker => &[ErrorCode::Forbidden, ErrorCode::UpgradeRequired],
            AuthClass::Session
            | AuthClass::SessionOrDevice
            | AuthClass::DeviceOnly
            | AuthClass::SessionOnly
            | AuthClass::Admin => &[
                ErrorCode::Unauthenticated,
                ErrorCode::Forbidden,
                ErrorCode::UpgradeRequired,
            ],
        }
    }
}

/// The body limit class of an operation (`api_v1_surface.md`, body classes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BodyClass {
    Small,
    Commit,
    Batch,
    Block,
    Staged,
    Query,
    Auth,
    WorkerOutput,
    None,
}

const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;

impl BodyClass {
    pub const ALL: [BodyClass; 9] = [
        BodyClass::Small,
        BodyClass::Commit,
        BodyClass::Batch,
        BodyClass::Block,
        BodyClass::Staged,
        BodyClass::Query,
        BodyClass::Auth,
        BodyClass::WorkerOutput,
        BodyClass::None,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            BodyClass::Small => "Small",
            BodyClass::Commit => "Commit",
            BodyClass::Batch => "Batch",
            BodyClass::Block => "Block",
            BodyClass::Staged => "Staged",
            BodyClass::Query => "Query",
            BodyClass::Auth => "Auth",
            BodyClass::WorkerOutput => "WorkerOutput",
            BodyClass::None => "None",
        }
    }

    pub fn from_name(name: &str) -> Option<BodyClass> {
        BodyClass::ALL
            .into_iter()
            .find(|class| class.name() == name)
    }

    /// The largest body the class accepts, in bytes; a worker output is capped per kind by 08, so its
    /// class has no fixed ceiling.
    pub const fn limit_bytes(self) -> u64 {
        match self {
            BodyClass::None => 0,
            BodyClass::Auth => 16 * KIB,
            BodyClass::Small => 64 * KIB,
            BodyClass::Query => 128 * KIB,
            BodyClass::Block | BodyClass::Staged => 2 * MIB,
            BodyClass::Commit | BodyClass::Batch => 4 * MIB,
            BodyClass::WorkerOutput => u64::MAX,
        }
    }

    pub(crate) const fn media_type(self) -> &'static str {
        match self {
            BodyClass::Block | BodyClass::Staged | BodyClass::WorkerOutput => {
                "application/octet-stream"
            }
            _ => "application/json",
        }
    }

    pub(crate) fn implied_errors(self) -> &'static [ErrorCode] {
        match self {
            BodyClass::None => &[],
            _ => &[ErrorCode::InvalidRequest, ErrorCode::SizeTooLarge],
        }
    }
}
