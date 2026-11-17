//! `admin epoch-bump`, `admin restore` and `admin gc-run` (E-22/c10, c11, c12): one call on the owner's port each.
//! The destructive-command guard has already run.

use contracts::{
    DoctorContext, DoctorTag, EpochAdmin, NodeOrPath, Placement, RestoreError, TreeRestore,
};
use sc_clock::Clock;
use sc_error::ErrorCode;
use sc_observability::log_error;
use sunnycloud_blobstore::GcSweeper;
use uuid::Uuid;

use crate::commands::RestoreArgs;
use crate::context::ServiceContext;
use crate::doctor_command::{CheckSelection, findings_value, run_checks};
use crate::error::CliError;
use crate::exit::Exit;
use crate::outcome::Outcome;
use crate::root::DoctorChecks;
use crate::utc_arg;

/// The bump, then the `restore`-tagged checks. The checks' findings never change the exit code; a bump that
/// returned an error exits 1 (G-12-5), and running the command again is always safe because 05 only raises the
/// epoch.
pub(crate) async fn epoch_bump(services: &ServiceContext) -> Result<Outcome, CliError> {
    let admin = services.get::<dyn EpochAdmin>().await?;
    let clock = services.get::<dyn Clock>().await?;
    // Built before the bump: a failure here must not be reported for an epoch that is already raised.
    let checks = services.get::<DoctorChecks>().await?;
    let epoch = admin.bump_epoch(clock.now_utc()).await.map_err(|error| {
        log_error!("epoch bump failed", err = &error);
        CliError::Operator {
            code: "epoch_bump_failed",
            detail: format!(
                "the epoch bump did not complete ({error}); run `admin epoch-bump` again, it is always safe"
            ),
            exit: Exit::Warnings,
        }
    })?;
    let findings = run_checks(
        &checks.0,
        CheckSelection::Tagged(DoctorTag::Restore),
        &DoctorContext::default(),
    )
    .await;
    Ok(Outcome::success()
        .field("epoch", epoch)
        .field("findings", findings_value(&findings)))
}

pub(crate) async fn restore(
    args: &RestoreArgs,
    services: &ServiceContext,
) -> Result<Outcome, CliError> {
    let account = Uuid::parse_str(&args.account).map_err(|_| {
        CliError::Usage(format!("--account `{}` is not an account id", args.account))
    })?;
    let at = utc_arg::parse("--at", &args.at)?;
    let restorer = services.get::<dyn TreeRestore>().await?;
    let target = NodeOrPath::Path(args.path.clone());
    let outcome = if args.dry_run {
        restorer.preview_restore(account, target, at).await
    } else {
        restorer.restore_revision(account, target, at).await
    }
    .map_err(refusal)?;
    Ok(Outcome::success()
        .field("changed", !args.dry_run)
        .field("revision", outcome.restored_revision.to_string())
        .field(
            "placement",
            match outcome.placement {
                Placement::Original => "original",
                Placement::Restored => "restored",
            },
        ))
}

pub(crate) async fn gc_run(services: &ServiceContext) -> Result<Outcome, CliError> {
    let gc = services.get::<GcSweeper>().await?;
    let clock = services.get::<dyn Clock>().await?;
    let stats = gc
        .run_cycle(clock.now_utc())
        .await
        .map_err(|error| CliError::Operator {
            code: ErrorCode::Internal.as_str(),
            detail: format!("gc: {error}"),
            exit: Exit::Failed,
        })?;
    Ok(Outcome::success()
        .field("removed", stats.removed)
        .field("remaining_due", stats.remaining_due)
        .field("budget_spent", stats.budget_spent))
}

fn refusal(error: RestoreError) -> CliError {
    let (code, detail) = match error {
        RestoreError::BeyondRetention => (
            "beyond_retention",
            "the time is beyond the retention window".to_owned(),
        ),
        RestoreError::NotFound => (
            ErrorCode::NotFound.as_str(),
            "no such path or revision".to_owned(),
        ),
        RestoreError::FileMissing => (
            "file_missing",
            "the file of that revision is missing from the volume".to_owned(),
        ),
        RestoreError::Unexpected(fault) => {
            log_error!("restore failed", err = fault.as_ref());
            (
                ErrorCode::Internal.as_str(),
                "the restore failed".to_owned(),
            )
        }
    };
    CliError::Operator {
        code,
        detail: format!("restore: {detail}"),
        exit: Exit::Failed,
    }
}
