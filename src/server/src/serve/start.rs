//! The start of `serve` (E-22/c05), in this order: configuration, database role, schema version, key check,
//! epoch marker, services, listeners, late lists, scheduler. Every refusal returns before the listeners are
//! bound, with the fix in its text; one INFO line marks each step that passed.

use std::sync::Arc;

use contracts::{EpochAdmin, KeyError, Marker, MarkerStatus};
use sc_db::{Db, DbError, MigrationError, Migrator, SchemaState};
use sc_error::ErrorCode;
use sc_observability::log_info;
use sunnycloud_blobstore::{MasterKeyring, operator_message};
use sunnycloud_http_api::{BoundServer, HttpServer, HttpServerError};
use sunnycloud_sync_feed::PgEpochAdmin;

use super::http::http_config;
use crate::command_config::ServeSettings;
use crate::context::ServiceContext;
use crate::error::CliError;
use crate::exit::Exit;
use crate::janitor::Scheduler;
use crate::key_commands::key_kind;
use crate::root::{self, AccountHooks, Janitors};

/// What a started `serve` holds until its shutdown.
pub(super) struct Started {
    pub server: BoundServer,
    pub scheduler: Arc<Scheduler>,
}

pub(super) async fn start(services: &ServiceContext) -> Result<Started, CliError> {
    let settings = services.get::<ServeSettings>().await?;
    let config = http_config(&settings);
    log_info!("serve start: configuration");

    let db = services.get::<Db>().await.map_err(database_refusal)?;
    log_info!("serve start: database role");

    schema_check(&db).await?;
    log_info!("serve start: schema version");

    let keyring = services.get::<MasterKeyring>().await?;
    keyring.load_and_verify().await.map_err(key_refusal)?;
    log_info!("serve start: key check");

    let epoch = services.get::<PgEpochAdmin>().await?;
    let status = epoch.marker_status().await.map_err(|error| {
        refusal(
            ErrorCode::Internal.as_str(),
            format!("epoch marker check failed: {error}"),
        )
    })?;
    epoch_check(status)?;
    log_info!("serve start: epoch marker");

    let api = root::api_services(services).await?;
    log_info!("serve start: services");

    let server = HttpServer::new(api, config)
        .map_err(http_refusal)?
        .bind()
        .await
        .map_err(http_refusal)?;
    log_info!("serve start: listeners");

    services.get::<AccountHooks>().await?;
    services.get::<Janitors>().await?;
    log_info!("serve start: late lists");

    let scheduler = root::start_scheduler(services).await?;
    log_info!("serve start: scheduler");
    Ok(Started { server, scheduler })
}

fn refusal(code: &'static str, detail: String) -> CliError {
    CliError::Operator {
        code,
        detail,
        exit: Exit::Failed,
    }
}

/// The pool builder connects with `db_api_url` and helper 07 verifies the role (`Db::connect`).
fn database_refusal(error: CliError) -> CliError {
    let CliError::Build { source, .. } = &error else {
        return error;
    };
    match source.downcast_ref::<DbError>() {
        Some(DbError::WrongRole(reason)) => refusal(
            "wrong_role",
            format!(
                "database role check failed: {reason}; db_api_url must connect as the `sunnycloud` API role, \
                 never the schema owner"
            ),
        ),
        Some(DbError::Unavailable) => refusal(
            ErrorCode::ServiceUnavailable.as_str(),
            "cannot connect with db_api_url: database unavailable".to_owned(),
        ),
        _ => error,
    }
}

/// `serve` never migrates: anything but `Current` refuses and names the command that does.
async fn schema_check(db: &Db) -> Result<(), CliError> {
    match Migrator::check_version(db).await {
        Ok(SchemaState::Current) => Ok(()),
        Ok(SchemaState::Pending { n }) => Err(refusal(
            "migration_pending",
            format!(
                "schema version: {n} migration(s) pending; run `sunnycloud migrate` before `serve`"
            ),
        )),
        Ok(SchemaState::UnknownNewer) => Err(refusal(
            "unknown_newer",
            "schema version: UnknownNewer, the database is newer than this binary; run the matching release, \
             or `sunnycloud migrate status` to see the versions (`sunnycloud migrate` never downgrades)"
                .to_owned(),
        )),
        Err(MigrationError::Db(error)) => Err(refusal(
            ErrorCode::Internal.as_str(),
            format!("schema version check failed: {error}"),
        )),
        Err(error) => Err(refusal(
            "schema_not_current",
            format!("schema version: {error}; see `sunnycloud migrate status`"),
        )),
    }
}

/// The variant name, then 04's operator text; neither carries key material.
fn key_refusal(error: KeyError) -> CliError {
    let kind = key_kind(&error);
    refusal(
        "master_key_refused",
        format!(
            "master key check: {kind}: {}; check master_key_file",
            operator_message(&error)
        ),
    )
}

/// A database epoch below the `.epoch` marker is a restore of an older database onto newer blobs.
fn epoch_check(status: MarkerStatus) -> Result<(), CliError> {
    match status.marker {
        Marker::Present(marker) if status.db_epoch < marker => Err(refusal(
            "epoch_below_marker",
            format!(
                "epoch marker: database epoch {} is below the .epoch marker {marker}; run `sunnycloud admin \
                 epoch-bump` after a restore",
                status.db_epoch
            ),
        )),
        Marker::Present(_) | Marker::Absent => Ok(()),
    }
}

fn http_refusal(error: HttpServerError) -> CliError {
    refusal(
        ErrorCode::Internal.as_str(),
        format!("http server: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detail(error: CliError) -> String {
        assert_eq!(error.exit(), Exit::Failed);
        error.to_string()
    }

    #[test]
    fn an_epoch_below_the_marker_refuses_and_names_the_bump() {
        // Act
        let error = epoch_check(MarkerStatus {
            db_epoch: 4,
            marker: Marker::Present(5),
        })
        .expect_err("refused");

        // Assert
        assert_eq!(error.code(), "epoch_below_marker");
        assert!(detail(error).contains("sunnycloud admin epoch-bump"));
    }

    #[test]
    fn an_epoch_at_or_above_the_marker_or_no_marker_passes() {
        for (db_epoch, marker) in [
            (5, Marker::Present(5)),
            (6, Marker::Present(5)),
            (0, Marker::Absent),
        ] {
            // Act
            let result = epoch_check(MarkerStatus { db_epoch, marker });

            // Assert
            assert!(result.is_ok(), "{db_epoch} {marker:?}");
        }
    }

    #[test]
    fn every_key_refusal_names_its_variant_and_the_operator_text() {
        for (error, kind) in [
            (KeyError::FileMissing, "FileMissing"),
            (KeyError::WrongLength, "WrongLength"),
            (KeyError::KeyCheckMismatch, "KeyCheckMismatch"),
            (
                KeyError::KeyCheckAbsentButBlobsExist,
                "KeyCheckAbsentButBlobsExist",
            ),
        ] {
            // Act
            let text = detail(key_refusal(error));

            // Assert
            assert!(text.contains(kind), "{text}");
            assert!(text.contains("master_key_file"), "{text}");
        }
    }

    #[test]
    fn a_wrong_role_names_the_role_check() {
        // Arrange
        let error = CliError::Build {
            service: "the API database pool",
            source: Box::new(DbError::WrongRole(
                "the API role owns the application schema",
            )),
        };

        // Act
        let error = database_refusal(error);

        // Assert
        assert_eq!(error.code(), "wrong_role");
        assert!(detail(error).contains("database role check"));
    }
}
