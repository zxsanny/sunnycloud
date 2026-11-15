//! The part of `serve`'s shutdown after 11 returned (readiness 503, listeners stopped, long-polls woken and
//! connections drained are 11's, `http_server.md`): stop the scheduler, flush the upload progress checkpoints
//! (03), run the final group sync (04), close the pool. Every step runs even when an earlier one failed; the failures are logged
//! and returned together.

use sc_clock::Clock;
use sc_db::Db;
use sc_error::ErrorCode;
use sc_observability::{log_error, log_info};

use crate::context::ServiceContext;
use crate::error::CliError;
use crate::exit::Exit;
use crate::janitor::Scheduler;
use crate::root::{Blobstore, Shutdown, Transfer};

pub(super) async fn stop(services: &ServiceContext, scheduler: &Scheduler) -> Result<(), CliError> {
    let mut failures = Vec::new();

    // A GC cycle that runs ends between batches; the loops end after their running cycle.
    services.get::<Shutdown>().await?.janitors.cancel();
    scheduler.stop().await;
    log_info!("serve stop: scheduler stopped");

    let transfer = services.get::<Transfer>().await?;
    let now = services.get::<dyn Clock>().await?.now_utc();
    match transfer.service.flush_all(now).await {
        Ok(_) => log_info!("serve stop: upload checkpoints flushed"),
        Err(error) => {
            log_error!(
                "serve stop: upload checkpoint flush failed",
                err = error.as_ref()
            );
            failures.push(format!("upload checkpoint flush: {error}"));
        }
    }

    let blobs = services.get::<Blobstore>().await?;
    match blobs.barrier.sync_open_groups().await {
        Ok(()) => log_info!("serve stop: final group sync"),
        Err(error) => {
            log_error!("serve stop: final group sync failed", err = &error);
            failures.push(format!("final group sync: {error}"));
        }
    }

    // Last: the steps above may still write through the pool.
    services.get::<Db>().await?.close().await;
    log_info!("serve stop: pool closed");

    if failures.is_empty() {
        Ok(())
    } else {
        Err(CliError::Operator {
            code: ErrorCode::Internal.as_str(),
            detail: format!("shutdown incomplete: {}", failures.join("; ")),
            exit: Exit::Failed,
        })
    }
}
