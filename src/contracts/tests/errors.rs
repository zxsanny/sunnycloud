//! AC-6: typed outcomes only. Each `match` below has no wildcard arm, so it stops compiling when a variant is
//! added or removed; the expected lists are the error columns of the contract tables.

use std::time::Duration;

use contracts::{
    AccountAdminError, AccountGuardError, AccountServiceError, AdminError, AuthenticatorError,
    DeletionError, DevSeedError, DeviceGrantError, InviteError, OperatorIdentityError,
    QuotaAdminError, QuotaLedgerError, RevalidatorError, SessionError, SignInError,
};
use sc_error::ErrorCode;

mod common;

use common::fault;

fn authenticator(e: &AuthenticatorError) -> &'static str {
    match e {
        AuthenticatorError::Unauthenticated => "Unauthenticated",
        AuthenticatorError::Forbidden => "Forbidden",
        AuthenticatorError::Unexpected(_) => "Unexpected",
    }
}

fn guard(e: &AccountGuardError) -> &'static str {
    match e {
        AccountGuardError::AccountNotActive => "AccountNotActive",
        AccountGuardError::Unexpected(_) => "Unexpected",
    }
}

fn sign_in(e: &SignInError) -> &'static str {
    match e {
        SignInError::UnknownProvider => "UnknownProvider",
        SignInError::InvalidReturnPath => "InvalidReturnPath",
        SignInError::HostMismatch => "HostMismatch",
        SignInError::RateLimited { .. } => "RateLimited",
        SignInError::UntrustedState => "UntrustedState",
        SignInError::TrustedFailure { .. } => "TrustedFailure",
        SignInError::InvitationRequired => "InvitationRequired",
        SignInError::ProviderUnavailable => "ProviderUnavailable",
        SignInError::IdentityTaken => "IdentityTaken",
        SignInError::InvalidCode => "InvalidCode",
        SignInError::Unexpected(_) => "Unexpected",
    }
}

fn session(e: &SessionError) -> &'static str {
    match e {
        SessionError::ReuseDetected => "ReuseDetected",
        SessionError::Unauthenticated => "Unauthenticated",
        SessionError::Unexpected(_) => "Unexpected",
    }
}

fn device(e: &DeviceGrantError) -> &'static str {
    match e {
        DeviceGrantError::AuthorizationPending => "AuthorizationPending",
        DeviceGrantError::SlowDown => "SlowDown",
        DeviceGrantError::AccessDenied => "AccessDenied",
        DeviceGrantError::ExpiredToken => "ExpiredToken",
        DeviceGrantError::UnknownCode => "UnknownCode",
        DeviceGrantError::RateLimited { .. } => "RateLimited",
        DeviceGrantError::Expired => "Expired",
        DeviceGrantError::AlreadyDecided => "AlreadyDecided",
        DeviceGrantError::NotFound => "NotFound",
        DeviceGrantError::Forbidden => "Forbidden",
        DeviceGrantError::Unexpected(_) => "Unexpected",
    }
}

fn admin(e: &AdminError) -> &'static str {
    match e {
        AdminError::LastAdmin => "LastAdmin",
        AdminError::NotFound => "NotFound",
        AdminError::InvalidLimit => "InvalidLimit",
        AdminError::Unexpected(_) => "Unexpected",
    }
}

fn deletion(e: &DeletionError) -> &'static str {
    match e {
        DeletionError::Forbidden => "Forbidden",
        DeletionError::LastAdmin => "LastAdmin",
        DeletionError::ConfirmationMismatch => "ConfirmationMismatch",
        DeletionError::Unexpected(_) => "Unexpected",
    }
}

fn dev_seed(e: &DevSeedError) -> &'static str {
    match e {
        DevSeedError::RefusedInProduction => "RefusedInProduction",
        DevSeedError::Unexpected(_) => "Unexpected",
    }
}

fn ledger(e: &QuotaLedgerError) -> &'static str {
    match e {
        QuotaLedgerError::QuotaExceeded { .. } => "QuotaExceeded",
        QuotaLedgerError::InvariantViolation => "InvariantViolation",
        QuotaLedgerError::Unexpected(_) => "Unexpected",
    }
}

fn quota_admin(e: &QuotaAdminError) -> &'static str {
    match e {
        QuotaAdminError::NotFound => "NotFound",
        QuotaAdminError::InvalidLimit => "InvalidLimit",
        QuotaAdminError::Unexpected(_) => "Unexpected",
    }
}

fn only_unexpected() -> [&'static str; 5] {
    [
        match RevalidatorError::Unexpected(fault()) {
            RevalidatorError::Unexpected(_) => "Unexpected",
        },
        match InviteError::Unexpected(fault()) {
            InviteError::Unexpected(_) => "Unexpected",
        },
        match OperatorIdentityError::Unexpected(fault()) {
            OperatorIdentityError::Unexpected(_) => "Unexpected",
        },
        match AccountAdminError::Unexpected(fault()) {
            AccountAdminError::Unexpected(_) => "Unexpected",
        },
        match AccountServiceError::Unexpected(fault()) {
            AccountServiceError::Unexpected(_) => "Unexpected",
        },
    ]
}

#[test]
fn identity_error_variants_match_the_contract_tables() {
    let names = [
        authenticator(&AuthenticatorError::Unauthenticated),
        authenticator(&AuthenticatorError::Forbidden),
        authenticator(&AuthenticatorError::Unexpected(fault())),
        guard(&AccountGuardError::AccountNotActive),
        guard(&AccountGuardError::Unexpected(fault())),
        session(&SessionError::ReuseDetected),
        session(&SessionError::Unauthenticated),
        session(&SessionError::Unexpected(fault())),
    ];

    assert_eq!(
        names,
        [
            "Unauthenticated",
            "Forbidden",
            "Unexpected",
            "AccountNotActive",
            "Unexpected",
            "ReuseDetected",
            "Unauthenticated",
            "Unexpected"
        ]
    );
}

#[test]
fn sign_in_variants_are_the_begin_complete_link_and_exchange_failures() {
    let variants = [
        SignInError::UnknownProvider,
        SignInError::InvalidReturnPath,
        SignInError::HostMismatch,
        SignInError::RateLimited {
            retry_after: Duration::from_secs(1),
        },
        SignInError::UntrustedState,
        SignInError::TrustedFailure {
            code: ErrorCode::InvalidRequest,
        },
        SignInError::InvitationRequired,
        SignInError::ProviderUnavailable,
        SignInError::IdentityTaken,
        SignInError::InvalidCode,
        SignInError::Unexpected(fault()),
    ];

    let names: Vec<_> = variants.iter().map(sign_in).collect();

    assert_eq!(
        names,
        [
            "UnknownProvider",
            "InvalidReturnPath",
            "HostMismatch",
            "RateLimited",
            "UntrustedState",
            "TrustedFailure",
            "InvitationRequired",
            "ProviderUnavailable",
            "IdentityTaken",
            "InvalidCode",
            "Unexpected"
        ]
    );
}

#[test]
fn device_grant_variants_are_the_poll_lookup_decide_and_revoke_failures() {
    let variants = [
        DeviceGrantError::AuthorizationPending,
        DeviceGrantError::SlowDown,
        DeviceGrantError::AccessDenied,
        DeviceGrantError::ExpiredToken,
        DeviceGrantError::UnknownCode,
        DeviceGrantError::RateLimited {
            retry_after: Duration::from_secs(1),
        },
        DeviceGrantError::Expired,
        DeviceGrantError::AlreadyDecided,
        DeviceGrantError::NotFound,
        DeviceGrantError::Forbidden,
        DeviceGrantError::Unexpected(fault()),
    ];

    let names: Vec<_> = variants.iter().map(device).collect();

    assert_eq!(
        names,
        [
            "AuthorizationPending",
            "SlowDown",
            "AccessDenied",
            "ExpiredToken",
            "UnknownCode",
            "RateLimited",
            "Expired",
            "AlreadyDecided",
            "NotFound",
            "Forbidden",
            "Unexpected"
        ]
    );
}

#[test]
fn admin_and_quota_variants_match_the_contract_tables() {
    let admin_names = [
        admin(&AdminError::LastAdmin),
        admin(&AdminError::NotFound),
        admin(&AdminError::InvalidLimit),
        admin(&AdminError::Unexpected(fault())),
    ];
    let deletion_names = [
        deletion(&DeletionError::Forbidden),
        deletion(&DeletionError::LastAdmin),
        deletion(&DeletionError::ConfirmationMismatch),
        deletion(&DeletionError::Unexpected(fault())),
    ];
    let seed_names = [
        dev_seed(&DevSeedError::RefusedInProduction),
        dev_seed(&DevSeedError::Unexpected(fault())),
    ];
    let ledger_names = [
        ledger(&QuotaLedgerError::QuotaExceeded {
            limit: 1,
            used: 1,
            reserved: 0,
        }),
        ledger(&QuotaLedgerError::InvariantViolation),
        ledger(&QuotaLedgerError::Unexpected(fault())),
    ];
    let quota_names = [
        quota_admin(&QuotaAdminError::NotFound),
        quota_admin(&QuotaAdminError::InvalidLimit),
        quota_admin(&QuotaAdminError::Unexpected(fault())),
    ];

    assert_eq!(
        admin_names,
        ["LastAdmin", "NotFound", "InvalidLimit", "Unexpected"]
    );
    assert_eq!(
        deletion_names,
        [
            "Forbidden",
            "LastAdmin",
            "ConfirmationMismatch",
            "Unexpected"
        ]
    );
    assert_eq!(seed_names, ["RefusedInProduction", "Unexpected"]);
    assert_eq!(
        ledger_names,
        ["QuotaExceeded", "InvariantViolation", "Unexpected"]
    );
    assert_eq!(quota_names, ["NotFound", "InvalidLimit", "Unexpected"]);
    assert_eq!(only_unexpected(), ["Unexpected"; 5]);
}

#[test]
fn the_unexpected_fault_keeps_its_source_for_the_log() {
    let error = AdminError::Unexpected(fault());

    let source = std::error::Error::source(&error).map(ToString::to_string);

    assert_eq!(source.as_deref(), Some("planted fault"));
    assert_eq!(error.to_string(), "unexpected failure");
}
