use crate::commands::Confirmation;
use crate::error::CliError;
use crate::terminal::Terminal;

/// The destructive-command guard: runs before any business call.
///
/// With `--yes` (or `cli_confirm_required = off`) it passes. Without, a non-TTY stdin is refused and a TTY gets
/// a prompt that names the target; only `y` or `yes` passes.
pub fn confirm(
    terminal: &Terminal,
    confirmation: &Confirmation,
    required: bool,
) -> Result<(), CliError> {
    if confirmation.yes || !required {
        return Ok(());
    }
    if !terminal.stdin_is_tty() {
        return Err(CliError::Usage(format!(
            "{}: confirmation required; stdin is not a terminal, pass --yes",
            confirmation.action
        )));
    }
    terminal
        .write_stderr(&format!("{}. Continue? [y/N] ", confirmation.action))
        .map_err(|error| CliError::Usage(format!("cannot ask for confirmation: {error}")))?;
    let answer = terminal
        .read_line()
        .map_err(|error| CliError::Usage(format!("cannot read the confirmation: {error}")))?;
    match answer
        .as_deref()
        .map(|answer| answer.trim().to_ascii_lowercase())
    {
        Some(answer) if answer == "y" || answer == "yes" => Ok(()),
        _ => Err(CliError::Usage(
            "not confirmed, nothing was done".to_owned(),
        )),
    }
}
