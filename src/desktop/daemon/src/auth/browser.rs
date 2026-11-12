use std::io;
use std::process::{Command, Stdio};

/// Opens a URL in the user's browser. A trait so tests never start a real browser.
pub trait BrowserLauncher: Send + Sync {
    fn open(&self, url: &str) -> io::Result<()>;
}

/// The desktop's default browser: `xdg-open` on Linux, the shell on Windows, `open` on macOS.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemBrowser;

impl BrowserLauncher for SystemBrowser {
    fn open(&self, url: &str) -> io::Result<()> {
        let mut child = launcher_command(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        // Reaped on its own thread: some launchers stay until the browser window closes.
        std::thread::Builder::new()
            .name("browser-launcher".to_owned())
            .spawn(move || match child.wait() {
                Ok(status) if status.success() => {}
                Ok(status) => tracing::warn!(?status, "the browser launcher failed"),
                Err(error) => tracing::warn!(%error, "the browser launcher could not be awaited"),
            })?;
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn launcher_command(url: &str) -> Command {
    let mut command = Command::new("xdg-open");
    command.arg(url);
    command
}

#[cfg(windows)]
fn launcher_command(url: &str) -> Command {
    let mut command = Command::new("explorer.exe");
    command.arg(url);
    command
}

#[cfg(target_os = "macos")]
fn launcher_command(url: &str) -> Command {
    let mut command = Command::new("open");
    command.arg(url);
    command
}
