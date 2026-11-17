//! `key-check`, `admin keycheck-init` and `admin rewrap` (E-22/c11): one call or one loop on 04's keyring port.
//! Key bytes never leave 04: the commands pass ids and the path of the configured key file, and print key ids
//! and counts only. The destructive-command guard has already run.
//!
//! Rotation is config-first (owner decision 2026-11-17): the operator adds the new key to `master_key_ring`,
//! sets `master_key_current` and restarts; `admin rewrap --from-key-id N` then moves every data key wrapped under
//! `N` to the current key.

use std::io::ErrorKind;

use contracts::{KeyError, MasterKey, MasterKeyring};
use sc_observability::log_error;
use sunnycloud_blobstore::operator_message;
use tokio::time::{MissedTickBehavior, interval};

use crate::command_config::ServeSettings;
use crate::commands::RewrapArgs;
use crate::context::ServiceContext;
use crate::error::CliError;
use crate::exit::Exit;
use crate::handler::Call;
use crate::outcome::{Class, Outcome};
use crate::settings::CliSettings;

/// Rows per `rewrap_batch` call; the spec leaves the size open (owner decision 2026-11-17).
const REWRAP_BATCH: u32 = 100;

/// The variant name of a key refusal; it carries no key material.
pub(crate) fn key_kind(error: &KeyError) -> &'static str {
    match error {
        KeyError::FileMissing => "FileMissing",
        KeyError::WrongLength => "WrongLength",
        KeyError::KeyCheckMismatch => "KeyCheckMismatch",
        KeyError::KeyCheckAbsentButBlobsExist => "KeyCheckAbsentButBlobsExist",
        KeyError::KeyCheckSampleFailed { .. } => "KeyCheckSampleFailed",
        KeyError::Unexpected(_) => "Unexpected",
    }
}

pub(crate) async fn key_check(services: &ServiceContext) -> Result<Outcome, CliError> {
    let keyring = services.get::<dyn MasterKeyring>().await?;
    let ring = keyring.load_and_verify().await.map_err(refusal)?;
    Ok(Outcome::success()
        .field("current_key_id", ring.current_key_id)
        .field("key_ids", ring.key_ids))
}

/// The candidate is the configured `master_key_file` (key 1), which 04's `init_key_check` documents.
pub(crate) async fn keycheck_init(services: &ServiceContext) -> Result<Outcome, CliError> {
    let keyring = services.get::<dyn MasterKeyring>().await?;
    let settings = services.get::<ServeSettings>().await?;
    let bytes = tokio::fs::read(settings.master_key_file.expose())
        .await
        .map_err(|error| match error.kind() {
            ErrorKind::NotFound => refusal(KeyError::FileMissing),
            kind => CliError::Operator {
                code: "key_file_unreadable",
                detail: format!("cannot read master_key_file: {kind}"),
                exit: Exit::Failed,
            },
        })?;
    keyring
        .init_key_check(&MasterKey(bytes))
        .await
        .map_err(refusal)?;
    Ok(Outcome::success().field("key_check", "written"))
}

pub(crate) async fn rewrap(args: &RewrapArgs, call: &Call<'_>) -> Result<Outcome, CliError> {
    let keyring = call.services.get::<dyn MasterKeyring>().await?;
    let progress = call.services.get::<CliSettings>().await?.progress_interval;
    let ring = keyring.load_and_verify().await.map_err(refusal)?;
    let (from, to) = (args.from_key_id, ring.current_key_id);
    if from == to || !ring.key_ids.contains(&from) {
        return Err(CliError::Usage(format!(
            "--from-key-id {from} must be an old key of the ring; the ring holds {:?} and the current key is {to}",
            ring.key_ids
        )));
    }
    let mut rewrapped = 0u64;
    let mut ticker = interval(progress);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ticker.tick().await;
    loop {
        if call.cancel.is_cancelled() {
            return Ok(outcome(Class::Interrupted, rewrapped, from, to));
        }
        let batch = keyring.rewrap_batch(from, to, REWRAP_BATCH);
        tokio::pin!(batch);
        let handled = loop {
            tokio::select! {
                result = &mut batch => break result.map_err(refusal)?,
                _ = ticker.tick() => report(call, rewrapped),
            }
        };
        if handled == 0 {
            return Ok(outcome(Class::Success, rewrapped, from, to));
        }
        rewrapped = rewrapped.saturating_add(handled);
        report(call, rewrapped);
    }
}

fn outcome(class: Class, rewrapped: u64, from: i16, to: i16) -> Outcome {
    let note = match class {
        Class::Success => format!(
            "keep key {from} until every backup that needs it is gone; it is no longer used by any blob"
        ),
        _ => format!("interrupted: run the command again to continue; keep key {from}"),
    };
    Outcome::new(class)
        .field("rewrapped", rewrapped)
        .field("from_key_id", from)
        .field("to_key_id", to)
        .field("note", note)
}

fn report(call: &Call<'_>, rewrapped: u64) {
    let line = format!("rewrap: {rewrapped} data keys moved\n");
    if let Err(error) = call.terminal.write_stderr(&line) {
        log_error!("rewrap: cannot write the progress line", err = &error);
    }
}

fn refusal(error: KeyError) -> CliError {
    CliError::Operator {
        code: key_kind(&error),
        detail: operator_message(&error),
        exit: Exit::Failed,
    }
}
