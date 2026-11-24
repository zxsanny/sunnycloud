//! `admin dev-seed` (E-22/c09, AZ-3795 AC-4): one `DevSeed::seed` call. Production is refused by the owner before
//! any port is called; the command maps that refusal to exit 2 and prints nothing else.

use contracts::{DevSeed, DevSeedError};
use sc_error::ErrorCode;
use sc_observability::log_error;

use crate::context::ServiceContext;
use crate::error::CliError;
use crate::exit::Exit;
use crate::outcome::Outcome;

pub(crate) async fn run(services: &ServiceContext) -> Result<Outcome, CliError> {
    let seed = services.get::<dyn DevSeed>().await?;
    match seed.seed().await {
        Ok(()) => Ok(Outcome::success().field("seeded", true)),
        Err(DevSeedError::RefusedInProduction) => Err(CliError::Operator {
            code: ErrorCode::Forbidden.as_str(),
            detail: "admin dev-seed is refused when environment = production".to_owned(),
            exit: Exit::Failed,
        }),
        Err(DevSeedError::Unexpected(fault)) => {
            log_error!("dev seed failed", err = fault.as_ref());
            Err(CliError::Operator {
                code: ErrorCode::Internal.as_str(),
                detail: format!("the dev seed failed: {fault}"),
                exit: Exit::Failed,
            })
        }
    }
}
