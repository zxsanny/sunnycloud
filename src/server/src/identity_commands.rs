//! `admin setup-token|create|reset|promote|demote` and `admin invite create|list|revoke` (E-22/c08): one call on
//! 01's operator and invite ports or on 07's `AdminService` each. A token is printed once, through the secret
//! writer, and never appears in a log or on stderr. The destructive-command guard has already run.

use contracts::{
    Actor, AdminService, InviteError, InviteInfo, InviteService, OperatorIdentityError,
    OperatorIdentityOps, Role,
};
use sc_error::ErrorCode;
use sc_observability::log_error;
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::commands::InviteCommand;
use crate::context::ServiceContext;
use crate::error::CliError;
use crate::exit::Exit;
use crate::outcome::Outcome;
use crate::secret::OneTimeSecret;
use crate::user_commands;

const HINT: &str = "sign in with this token; it can be used once";

pub(crate) async fn setup_token(services: &ServiceContext) -> Result<Outcome, CliError> {
    let ops = services.get::<dyn OperatorIdentityOps>().await?;
    match ops.ensure_setup_token().await.map_err(operator_failure)? {
        Some(token) => Ok(with_token(token.expose())),
        None => Err(CliError::Operator {
            code: "SetupNotAvailable",
            detail: "an account already exists, so there is no setup token; use `admin create`"
                .to_owned(),
            exit: Exit::Failed,
        }),
    }
}

pub(crate) async fn create(services: &ServiceContext) -> Result<Outcome, CliError> {
    let ops = services.get::<dyn OperatorIdentityOps>().await?;
    let invite = ops.create_admin_invite().await.map_err(operator_failure)?;
    Ok(with_token(invite.0.expose()))
}

pub(crate) async fn reset(account: &str, services: &ServiceContext) -> Result<Outcome, CliError> {
    let account = account_id(account)?;
    let ops = services.get::<dyn OperatorIdentityOps>().await?;
    let invite = ops.reset_admin(account).await.map_err(operator_failure)?;
    Ok(with_token(invite.0.expose()))
}

/// `promote` gives the admin role, `demote` the user role; the last-admin rule is 07's.
pub(crate) async fn set_role(
    account: &str,
    role: Role,
    services: &ServiceContext,
) -> Result<Outcome, CliError> {
    let account = account_id(account)?;
    let admin = services.get::<dyn AdminService>().await?;
    admin
        .update_user(&Actor::Operator, account, Some(role), None)
        .await
        .map_err(user_commands::failure)?;
    Ok(Outcome::success().field(
        "role",
        match role {
            Role::Admin => "admin",
            Role::User => "user",
        },
    ))
}

pub(crate) async fn invite(
    admin: &str,
    command: &InviteCommand,
    services: &ServiceContext,
) -> Result<Outcome, CliError> {
    let admin = account_id(admin)?;
    let invites = services.get::<dyn InviteService>().await?;
    match command {
        InviteCommand::Create { note } => {
            let note = note.as_deref().unwrap_or_default();
            let created = invites
                .create_invite(admin, note)
                .await
                .map_err(invite_failure)?;
            Ok(with_token(created.token.0.expose()))
        }
        InviteCommand::List => {
            let rows = invites.list(admin).await.map_err(invite_failure)?;
            Ok(Outcome::success().field("invites", Value::Array(rows.iter().map(row).collect())))
        }
        InviteCommand::Revoke { id } => {
            let id = Uuid::parse_str(id)
                .map_err(|_| CliError::Usage(format!("`{id}` is not an invite id")))?;
            invites.revoke(admin, id).await.map_err(invite_failure)?;
            Ok(Outcome::success().field("revoked", id.to_string()))
        }
    }
}

pub(crate) fn with_token(token: &str) -> Outcome {
    Outcome::success()
        .field("hint", HINT)
        .with_secret(OneTimeSecret::new("token", token.to_owned()))
}

fn account_id(text: &str) -> Result<Uuid, CliError> {
    Uuid::parse_str(text).map_err(|_| CliError::Usage(format!("`{text}` is not an account id")))
}

fn row(invite: &InviteInfo) -> Value {
    let mut object = Map::new();
    object.insert("id".to_owned(), Value::from(invite.id.to_string()));
    object.insert("note".to_owned(), Value::from(invite.note.as_str()));
    object.insert(
        "created_at_micros".to_owned(),
        Value::from(invite.created_at.as_unix_micros()),
    );
    object.insert(
        "expires_at_micros".to_owned(),
        Value::from(invite.expires_at.as_unix_micros()),
    );
    object.insert(
        "redeemed_at_micros".to_owned(),
        invite
            .redeemed_at
            .map_or(Value::Null, |at| Value::from(at.as_unix_micros())),
    );
    Value::Object(object)
}

pub(crate) fn operator_failure(error: OperatorIdentityError) -> CliError {
    let (code, detail) = match error {
        OperatorIdentityError::NotFound => {
            (ErrorCode::NotFound.as_str(), "no such account".to_owned())
        }
        OperatorIdentityError::NotAnAdmin => {
            ("not_an_admin", "the account is not an admin".to_owned())
        }
        OperatorIdentityError::Unexpected(fault) => {
            log_error!("identity operation failed", err = fault.as_ref());
            (
                ErrorCode::Internal.as_str(),
                "the identity operation failed".to_owned(),
            )
        }
    };
    CliError::Operator {
        code,
        detail,
        exit: Exit::Failed,
    }
}

fn invite_failure(error: InviteError) -> CliError {
    let (code, detail) = match error {
        InviteError::NotFound => (
            ErrorCode::NotFound.as_str(),
            "no such open invite".to_owned(),
        ),
        InviteError::Unexpected(fault) => {
            log_error!("invite operation failed", err = fault.as_ref());
            (
                ErrorCode::Internal.as_str(),
                "the invite operation failed".to_owned(),
            )
        }
    };
    CliError::Operator {
        code,
        detail,
        exit: Exit::Failed,
    }
}
