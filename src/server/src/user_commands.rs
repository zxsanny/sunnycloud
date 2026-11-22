//! `admin user list` and `admin user set-quota` (E-22/c09): one call, or one page loop, on 07's `AdminService`
//! with `Actor::Operator`. No token is ever part of an account row.

use contracts::{
    AccountDeletionService, AccountStatus, Actor, AdminError, AdminService, AdminUserView,
    DeletionError, Page, Role,
};
use sc_error::ErrorCode;
use sc_observability::log_error;
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::context::ServiceContext;
use crate::error::CliError;
use crate::exit::Exit;
use crate::outcome::Outcome;

const PAGE_ROWS: u32 = 100;

pub(crate) async fn list(services: &ServiceContext) -> Result<Outcome, CliError> {
    let admin = services.get::<dyn AdminService>().await?;
    let mut users = Vec::new();
    let mut offset = 0u64;
    loop {
        let page = Page {
            offset,
            limit: PAGE_ROWS,
        };
        let rows = admin
            .list_users(&Actor::Operator, page)
            .await
            .map_err(failure)?
            .items;
        if rows.is_empty() {
            break;
        }
        offset += rows.len() as u64;
        users.extend(rows.iter().map(row));
    }
    Ok(Outcome::success().field("users", users))
}

pub(crate) async fn set_quota(
    account: &str,
    size: u64,
    services: &ServiceContext,
) -> Result<Outcome, CliError> {
    let account = Uuid::parse_str(account)
        .map_err(|_| CliError::Usage(format!("`{account}` is not an account id")))?;
    let limit = i64::try_from(size)
        .map_err(|_| CliError::Usage(format!("SIZE {size} is above the largest limit")))?;
    let admin = services.get::<dyn AdminService>().await?;
    admin
        .set_quota(&Actor::Operator, account, Some(limit))
        .await
        .map_err(failure)?;
    Ok(Outcome::success()
        .field("account", account.to_string())
        .field("limit", limit))
}

/// `--yes` was checked before the handler ran: it is the operator's confirmation.
pub(crate) async fn delete(account: &str, services: &ServiceContext) -> Result<Outcome, CliError> {
    let account = Uuid::parse_str(account)
        .map_err(|_| CliError::Usage(format!("`{account}` is not an account id")))?;
    let deletion = services.get::<dyn AccountDeletionService>().await?;
    let accepted = deletion
        .request_as_operator(account)
        .await
        .map_err(deletion_failure)?;
    Ok(Outcome::success()
        .field("account", account.to_string())
        .field("deadline_micros", accepted.deadline.as_unix_micros()))
}

fn row(user: &AdminUserView) -> Value {
    let mut object = Map::new();
    object.insert("account".to_owned(), Value::from(user.account.to_string()));
    object.insert("role".to_owned(), Value::from(role(user.role)));
    object.insert("state".to_owned(), Value::from(state(user.state)));
    object.insert(
        "last_login_micros".to_owned(),
        Value::from(user.last_login.map(|at| at.as_unix_micros())),
    );
    object.insert("used".to_owned(), Value::from(user.used));
    object.insert("limit".to_owned(), Value::from(user.limit));
    Value::Object(object)
}

const fn role(role: Role) -> &'static str {
    match role {
        Role::User => "user",
        Role::Admin => "admin",
    }
}

const fn state(state: AccountStatus) -> &'static str {
    match state {
        AccountStatus::Active => "active",
        AccountStatus::Disabled => "disabled",
        AccountStatus::Deleting => "deleting",
    }
}

pub(crate) fn failure(error: AdminError) -> CliError {
    let (code, detail) = match error {
        AdminError::NotFound => (ErrorCode::NotFound.as_str(), "no such account".to_owned()),
        AdminError::InvalidLimit => ("invalid_limit", "the limit is not accepted".to_owned()),
        AdminError::LastAdmin => ("last_admin", "this is the last admin".to_owned()),
        other => (ErrorCode::Internal.as_str(), format!("admin: {other}")),
    };
    CliError::Operator {
        code,
        detail,
        exit: Exit::Failed,
    }
}

/// `Forbidden` and `ConfirmationMismatch` belong to the session path and cannot come from `request_as_operator`.
fn deletion_failure(error: DeletionError) -> CliError {
    match error {
        DeletionError::LastAdmin => failure(AdminError::LastAdmin),
        DeletionError::NotFound => failure(AdminError::NotFound),
        DeletionError::Unexpected(fault) => {
            log_error!("account deletion failed", err = fault.as_ref());
            internal_deletion_failure()
        }
        other @ (DeletionError::Forbidden | DeletionError::ConfirmationMismatch) => {
            log_error!("account deletion refused", err = &other);
            internal_deletion_failure()
        }
    }
}

fn internal_deletion_failure() -> CliError {
    CliError::Operator {
        code: ErrorCode::Internal.as_str(),
        detail: "the account deletion failed".to_owned(),
        exit: Exit::Failed,
    }
}
