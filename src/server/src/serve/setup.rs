//! The last step of the `serve` start (AZ-3654 AC-1): on an install without accounts, issue the setup token and
//! print it once on stdout, through the same one-time-secret writer as `admin setup-token`. The token never reaches
//! a log line. In `--json` mode nothing is printed (a token must not appear in JSON output); the operator runs
//! `admin setup-token` for one.

use contracts::OperatorIdentityOps;
use sc_error::ErrorCode;
use sc_observability::log_info;

use crate::context::ServiceContext;
use crate::error::CliError;
use crate::exit::Exit;
use crate::identity_commands;
use crate::output::{Mode, print_outcome};
use crate::terminal::Terminal;

pub(super) async fn announce(
    services: &ServiceContext,
    terminal: &Terminal,
    mode: Mode,
) -> Result<(), CliError> {
    if mode == Mode::Json {
        log_info!(
            "serve start: no setup token is printed in --json mode; run `sunnycloud admin setup-token`"
        );
    } else {
        let ops = services.get::<dyn OperatorIdentityOps>().await?;
        let issued = ops
            .ensure_setup_token()
            .await
            .map_err(identity_commands::operator_failure)?;
        if let Some(token) = issued {
            print_outcome(
                terminal,
                mode,
                identity_commands::with_token(token.expose()),
            )
            .map_err(|error| CliError::Operator {
                code: ErrorCode::Internal.as_str(),
                detail: format!("cannot print the setup token: {error}"),
                exit: Exit::Failed,
            })?;
        }
    }
    log_info!("serve start: setup token");
    Ok(())
}
